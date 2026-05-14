mod support;

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, Once, OnceLock};

use axum::http::{Method, StatusCode};
use qsl_attachments::{build_router, AppState, Config, RetentionClass, TestClock, TestDiskSpace};
use tempfile::TempDir;
use tracing_subscriber::fmt::MakeWriter;

use support::{
    assert_error, commit_session, create_session, fetch_object, one_part_payload, read_json,
    upload_part, Fixture,
};

#[derive(Clone)]
struct LogCapture {
    bytes: Arc<Mutex<Vec<u8>>>,
}

struct LogWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.lock().expect("log lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogCapture {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter {
            bytes: Arc::clone(&self.bytes),
        }
    }
}

impl LogCapture {
    fn clear(&self) {
        self.bytes.lock().expect("log lock").clear();
    }

    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.bytes.lock().expect("log lock")).into_owned()
    }
}

static LOG_CAPTURE: OnceLock<LogCapture> = OnceLock::new();
static LOG_INIT: Once = Once::new();

fn log_capture() -> LogCapture {
    let capture = LOG_CAPTURE
        .get_or_init(|| LogCapture {
            bytes: Arc::new(Mutex::new(Vec::new())),
        })
        .clone();
    LOG_INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(capture.clone())
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
    capture.clear();
    capture
}

struct RestoredFixture {
    _tempdir: TempDir,
    app: axum::Router,
    state: AppState,
}

fn full_root_restore(fixture: &Fixture) -> RestoredFixture {
    restored_from_builder(&fixture.config, &fixture.clock, &fixture.disk, |root| {
        copy_dir_all(&fixture.storage_root, root);
    })
}

fn metadata_only_restore(fixture: &Fixture, locator_ref: &str) -> RestoredFixture {
    restored_from_builder(&fixture.config, &fixture.clock, &fixture.disk, |root| {
        let destination = root.join("objects").join(locator_ref);
        fs::create_dir_all(&destination).expect("partial restore object dir");
        fs::copy(
            fixture
                .storage_root
                .join("objects")
                .join(locator_ref)
                .join("object.json"),
            destination.join("object.json"),
        )
        .expect("copy metadata only");
    })
}

fn restored_from_builder(
    config: &Config,
    clock: &TestClock,
    disk: &TestDiskSpace,
    build_root: impl FnOnce(&Path),
) -> RestoredFixture {
    let tempdir = TempDir::new().expect("restore tempdir");
    let storage_root = tempdir.path().join("data");
    build_root(&storage_root);

    let mut config = config.clone();
    config.storage_root = storage_root;
    let state =
        AppState::new_with_disk_space(config, Arc::new(clock.clone()), Arc::new(disk.clone()))
            .expect("restored state");
    let app = build_router(state.clone());
    RestoredFixture {
        _tempdir: tempdir,
        app,
        state,
    }
}

fn copy_dir_all(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create destination dir");
    for entry in fs::read_dir(source).expect("read source dir") {
        let entry = entry.expect("source entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().expect("entry type").is_dir() {
            copy_dir_all(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy file");
        }
    }
}

fn assert_absent(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "unexpected secret/sentinel leak: {needle}"
    );
}

#[tokio::test]
async fn backup_restore_logs_redact_capability_descriptor_ciphertext_plaintext() {
    let logs = log_capture();
    let fixture = Fixture::new();

    let descriptor_sentinel = "QATT_DESCRIPTOR_SENTINEL_SHOULD_NOT_LOG_NA0286";
    let plaintext_sentinel = "QATT_PLAINTEXT_SENTINEL_SHOULD_NOT_LOG_NA0286";
    let ciphertext_sentinel = b"QATT_CIPHERTEXT_SENTINEL_SHOULD_NOT_LOG_NA0286";
    let wrong_fetch_sentinel = "NA0286WRONGFETCHLOGSENTINELAAAAAA";

    let malformed = fixture
        .raw_json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            format!(
                "{{\"attachment_id\":\"{}\",\"descriptor\":\"{}\",\"plaintext\":\"{}\"",
                "0".repeat(64),
                descriptor_sentinel,
                plaintext_sentinel
            ),
        )
        .await;
    let malformed_body = assert_error(
        malformed,
        StatusCode::BAD_REQUEST,
        "REJECT_QATTSVC_MALFORMED_JSON",
    )
    .await;

    let (parts, request) = one_part_payload(286_060, ciphertext_sentinel, RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(upload.status(), StatusCode::OK);
    let commit = commit_session(&fixture.app, &session_id, &resume_token, &request).await;
    assert_eq!(commit.status(), StatusCode::OK);
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap().to_owned();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap().to_owned();

    let restored = full_root_restore(&fixture);
    let restored_fetch = fetch_object(&restored.app, &locator_ref, &fetch_capability).await;
    assert_eq!(restored_fetch.status(), StatusCode::OK);

    let wrong_fetch = fetch_object(&restored.app, &locator_ref, wrong_fetch_sentinel).await;
    let wrong_fetch_body = assert_error(
        wrong_fetch,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_FETCH_CAPABILITY",
    )
    .await;

    let partial = metadata_only_restore(&fixture, &locator_ref);
    let partial_fetch = fetch_object(&partial.app, &locator_ref, &fetch_capability).await;
    let partial_fetch_body = assert_error(
        partial_fetch,
        StatusCode::NOT_FOUND,
        "REJECT_QATTSVC_LOCATOR_UNKNOWN",
    )
    .await;

    let ciphertext_sentinel = std::str::from_utf8(ciphertext_sentinel).unwrap();
    let original_audit = serde_json::to_string(&fixture.state.audit_snapshot()).unwrap();
    let restored_audit = serde_json::to_string(&restored.state.audit_snapshot()).unwrap();
    let restored_recovery = serde_json::to_string(&restored.state.recovery_summary()).unwrap();
    let partial_recovery = serde_json::to_string(&partial.state.recovery_summary()).unwrap();
    let error_json = [
        malformed_body.to_string(),
        wrong_fetch_body.to_string(),
        partial_fetch_body.to_string(),
    ]
    .join("\n");
    let log_output = logs.contents();

    for output in [
        &log_output,
        &original_audit,
        &restored_audit,
        &restored_recovery,
        &partial_recovery,
        &error_json,
    ] {
        assert_absent(output, &resume_token);
        assert_absent(output, &fetch_capability);
        assert_absent(output, wrong_fetch_sentinel);
        assert_absent(output, descriptor_sentinel);
        assert_absent(output, plaintext_sentinel);
        assert_absent(output, ciphertext_sentinel);
    }

    assert!(
        log_output.contains("session_committed") || original_audit.contains("session_committed"),
        "test should exercise redacted commit evidence"
    );
    assert!(
        log_output.contains("object_fetched") || restored_audit.contains("object_fetched"),
        "test should exercise redacted restored fetch evidence"
    );
    assert!(
        partial_recovery.contains("discarded_incoherent_objects"),
        "test should exercise redacted partial-restore recovery evidence"
    );
}
