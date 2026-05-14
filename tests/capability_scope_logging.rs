mod support;

use std::io::{self, Write};
use std::sync::{Arc, Mutex, Once, OnceLock};

use axum::http::{Method, StatusCode};
use qsl_attachments::RetentionClass;
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

fn assert_absent(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "unexpected secret/sentinel leak: {needle}"
    );
}

#[tokio::test]
async fn capability_abuse_logs_redact_capabilities_descriptor_ciphertext_plaintext() {
    let logs = log_capture();
    let fixture = Fixture::new();

    let descriptor_sentinel = "QATT_DESCRIPTOR_SENTINEL_SHOULD_NOT_LOG_NA0284";
    let plaintext_sentinel = "QATT_PLAINTEXT_SENTINEL_SHOULD_NOT_LOG_NA0284";
    let ciphertext_sentinel = b"QATT_CIPHERTEXT_SENTINEL_SHOULD_NOT_LOG_NA0284";
    let wrong_resume_sentinel = "NA0284WRONGRESUMELOGSENTINELAAAAA";
    let wrong_fetch_sentinel = "NA0284WRONGFETCHLOGSENTINELAAAAAA";

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

    let (parts, request) = one_part_payload(284_060, ciphertext_sentinel, RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;

    let wrong_resume = upload_part(
        &fixture.app,
        &session_id,
        wrong_resume_sentinel,
        0,
        &parts[0],
    )
    .await;
    let wrong_resume_body = assert_error(
        wrong_resume,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_RESUME_TOKEN",
    )
    .await;

    let upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(upload.status(), StatusCode::OK);
    let commit = commit_session(&fixture.app, &session_id, &resume_token, &request).await;
    assert_eq!(commit.status(), StatusCode::OK);
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap().to_owned();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap().to_owned();

    let wrong_fetch = fetch_object(&fixture.app, &locator_ref, wrong_fetch_sentinel).await;
    let wrong_fetch_body = assert_error(
        wrong_fetch,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_FETCH_CAPABILITY",
    )
    .await;

    let correct_fetch = fetch_object(&fixture.app, &locator_ref, &fetch_capability).await;
    assert_eq!(correct_fetch.status(), StatusCode::OK);

    let audit_json = serde_json::to_string(&fixture.state.audit_snapshot()).unwrap();
    let log_output = logs.contents();
    let ciphertext_sentinel = std::str::from_utf8(ciphertext_sentinel).unwrap();
    let error_json = [
        malformed_body.to_string(),
        wrong_resume_body.to_string(),
        wrong_fetch_body.to_string(),
    ]
    .join("\n");

    for output in [&log_output, &audit_json, &error_json] {
        assert_absent(output, &resume_token);
        assert_absent(output, &fetch_capability);
        assert_absent(output, wrong_resume_sentinel);
        assert_absent(output, wrong_fetch_sentinel);
        assert_absent(output, descriptor_sentinel);
        assert_absent(output, plaintext_sentinel);
        assert_absent(output, ciphertext_sentinel);
    }

    assert!(
        log_output.contains("session_created") || audit_json.contains("session_created"),
        "test should exercise redacted success audit evidence"
    );
    assert!(
        log_output.contains("object_fetched") || audit_json.contains("object_fetched"),
        "test should exercise redacted fetch audit evidence"
    );
}
