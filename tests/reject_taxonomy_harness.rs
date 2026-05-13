use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};

use axum::body::Body;
use axum::http::header::CONTENT_LENGTH;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use qsl_attachments::{
    build_router, sha512_merkle_root, AppState, CommitRequest, Config, CreateSessionRequest,
    PartSizeClass, RetentionClass, TestClock, TestDiskSpace,
};
use tempfile::TempDir;
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;

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

struct Fixture {
    _tempdir: TempDir,
    app: axum::Router,
    state: AppState,
    storage_root: PathBuf,
    clock: TestClock,
}

impl Fixture {
    fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let storage_root = tempdir.path().join("data");
        let config = Config {
            storage_root: storage_root.clone(),
            max_ciphertext_bytes: 1024 * 1024,
            max_open_sessions: 2,
            storage_reserve_bytes: 1024,
            session_ttl_secs: 5,
            short_retention_ttl_secs: 5,
            standard_retention_ttl_secs: 30,
            extended_retention_ttl_secs: 60,
            invalid_secret_attempt_limit: 2,
            invalid_range_attempt_limit: 2,
            ..Config::default()
        };
        let clock = TestClock::new(1_700_000_000);
        let state = AppState::new_with_disk_space(
            config,
            Arc::new(clock.clone()),
            Arc::new(TestDiskSpace::new(u64::MAX / 4)),
        )
        .expect("state");
        let app = build_router(state.clone());
        Self {
            _tempdir: tempdir,
            app,
            state,
            storage_root,
            clock,
        }
    }

    async fn json_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> axum::response::Response {
        self.raw_json_request(method, uri, headers, body.to_string())
            .await
    }

    async fn raw_json_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> axum::response::Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        self.app
            .clone()
            .oneshot(builder.body(Body::from(body)).expect("request"))
            .await
            .expect("response")
    }

    async fn bytes_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> axum::response::Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(CONTENT_LENGTH, body.len().to_string());
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        self.app
            .clone()
            .oneshot(builder.body(Body::from(body)).expect("request"))
            .await
            .expect("response")
    }
}

async fn read_json(response: axum::response::Response) -> serde_json::Value {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json body")
}

async fn read_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes()
        .to_vec()
}

fn attachment_id(seed: u64) -> String {
    format!("{seed:064x}")
}

fn one_part_payload(seed: u64, body: &[u8]) -> (Vec<Vec<u8>>, CreateSessionRequest) {
    let parts = vec![body.to_vec()];
    let request = CreateSessionRequest {
        attachment_id: attachment_id(seed),
        ciphertext_len: body.len() as u64,
        part_size_class: PartSizeClass::P64k,
        part_count: 1,
        integrity_alg: "sha512_merkle_v1".to_owned(),
        integrity_root: sha512_merkle_root(&parts),
        retention_class: RetentionClass::Short,
    };
    (parts, request)
}

async fn create_session(fixture: &Fixture, request: &CreateSessionRequest) -> (String, String) {
    let response = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = read_json(response).await;
    (
        body["session_id"].as_str().unwrap().to_owned(),
        body["resume_token"].as_str().unwrap().to_owned(),
    )
}

async fn commit_one_part(
    fixture: &Fixture,
    seed: u64,
    body: &[u8],
) -> (String, String, String, String) {
    let (parts, request) = one_part_payload(seed, body);
    let (session_id, resume_token) = create_session(fixture, &request).await;
    let upload = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_eq!(upload.status(), StatusCode::OK);
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", &resume_token)],
            serde_json::to_value(CommitRequest {
                attachment_id: request.attachment_id,
                ciphertext_len: request.ciphertext_len,
                part_count: request.part_count,
                integrity_alg: request.integrity_alg,
                integrity_root: request.integrity_root,
                retention_class: request.retention_class,
            })
            .unwrap(),
        )
        .await;
    assert_eq!(commit.status(), StatusCode::OK);
    let commit_body = read_json(commit).await;
    (
        session_id,
        resume_token,
        commit_body["locator_ref"].as_str().unwrap().to_owned(),
        commit_body["fetch_capability"].as_str().unwrap().to_owned(),
    )
}

fn entry_count(root: &Path, child: &str) -> usize {
    fs::read_dir(root.join(child))
        .map(|entries| entries.count())
        .unwrap_or(0)
}

fn part_path(fixture: &Fixture, session_id: &str, part_index: u32) -> PathBuf {
    fixture
        .storage_root
        .join("sessions")
        .join(session_id)
        .join("parts")
        .join(format!("{part_index}.part"))
}

fn object_bytes_path(fixture: &Fixture, locator_ref: &str) -> PathBuf {
    fixture
        .storage_root
        .join("objects")
        .join(locator_ref)
        .join("ciphertext.bin")
}

fn assert_absent(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "unexpected sensitive marker in log/audit output: {needle}"
    );
}

#[tokio::test]
async fn malformed_json_create_rejects_with_reason_code_and_no_persistence_or_leakage() {
    let logs = log_capture();
    let fixture = Fixture::new();
    let descriptor_sentinel = "QATT_DESCRIPTOR_SENTINEL_SHOULD_NOT_LOG";
    let plaintext_sentinel = "QATT_PLAINTEXT_SENTINEL_SHOULD_NOT_LOG";
    let malformed_body = format!(
        "{{\"attachment_id\":\"{}\",\"descriptor\":\"{}\",\"plaintext\":\"{}\"",
        attachment_id(901),
        descriptor_sentinel,
        plaintext_sentinel
    );

    let response = fixture
        .raw_json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            malformed_body,
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = read_json(response).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_MALFORMED_JSON");
    assert_eq!(body["message"], "malformed JSON request body");
    assert_eq!(entry_count(&fixture.storage_root, "sessions"), 0);
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);
    assert!(fixture.state.audit_snapshot().is_empty());

    let log_output = logs.contents();
    assert_absent(&log_output, descriptor_sentinel);
    assert_absent(&log_output, plaintext_sentinel);
}

#[tokio::test]
async fn malformed_json_commit_rejects_without_promoting_object_or_mutating_session() {
    let logs = log_capture();
    let fixture = Fixture::new();
    let ciphertext_sentinel = b"QATT_COMMIT_OPAQUE_CIPHERTEXT_SENTINEL";
    let descriptor_sentinel = "QATT_COMMIT_DESCRIPTOR_SENTINEL_SHOULD_NOT_LOG";
    let (parts, request) = one_part_payload(902, ciphertext_sentinel);
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let upload = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_eq!(upload.status(), StatusCode::OK);

    let response = fixture
        .raw_json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", &resume_token)],
            format!("{{\"descriptor\":\"{}\"", descriptor_sentinel),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = read_json(response).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_MALFORMED_JSON");
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);

    let status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body = read_json(status).await;
    assert_eq!(status_body["session_state"], "committable");
    assert_eq!(status_body["stored_part_count"], 1);

    let log_output = logs.contents();
    assert_absent(&log_output, descriptor_sentinel);
    assert_absent(&log_output, &resume_token);
    assert_absent(
        &log_output,
        std::str::from_utf8(ciphertext_sentinel).expect("sentinel utf8"),
    );
}

#[tokio::test]
async fn capability_rejects_fail_closed_without_part_mutation() {
    let logs = log_capture();
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(903, b"capability-reject-body");
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let wrong_resume_token = format!("{}01", "WRONG_".repeat(6));

    let missing = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[],
            parts[0].clone(),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::FORBIDDEN);
    let missing_body = read_json(missing).await;
    assert_eq!(missing_body["reason_code"], "REJECT_QATTSVC_RESUME_TOKEN");
    assert!(!part_path(&fixture, &session_id, 0).exists());

    let wrong = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &wrong_resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    let wrong_body = read_json(wrong).await;
    assert_eq!(wrong_body["reason_code"], "REJECT_QATTSVC_RESUME_TOKEN");
    assert!(!part_path(&fixture, &session_id, 0).exists());

    let status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body = read_json(status).await;
    assert_eq!(status_body["session_state"], "created");
    assert_eq!(status_body["stored_part_count"], 0);

    let log_output = logs.contents();
    assert_absent(&log_output, &resume_token);
    assert_absent(&log_output, &wrong_resume_token);
    assert_absent(&log_output, "capability-reject-body");
}

#[tokio::test]
async fn fetch_capability_mismatch_rejects_without_exposing_or_mutating_objects() {
    let logs = log_capture();
    let fixture = Fixture::new();
    let (_, _, locator_a, fetch_capability_a) =
        commit_one_part(&fixture, 904, b"opaque-object-a").await;
    let (_, _, locator_b, fetch_capability_b) =
        commit_one_part(&fixture, 905, b"opaque-object-b").await;

    let missing = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_b}"),
            &[],
            Vec::new(),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::FORBIDDEN);
    let missing_body = read_json(missing).await;
    assert_eq!(
        missing_body["reason_code"],
        "REJECT_QATTSVC_FETCH_CAPABILITY"
    );

    let wrong_resource = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_b}"),
            &[("X-QATT-Fetch-Capability", &fetch_capability_a)],
            Vec::new(),
        )
        .await;
    assert_eq!(wrong_resource.status(), StatusCode::FORBIDDEN);
    let wrong_resource_body = read_json(wrong_resource).await;
    assert_eq!(
        wrong_resource_body["reason_code"],
        "REJECT_QATTSVC_FETCH_CAPABILITY"
    );
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 2);

    let good_b = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_b}"),
            &[("X-QATT-Fetch-Capability", &fetch_capability_b)],
            Vec::new(),
        )
        .await;
    assert_eq!(good_b.status(), StatusCode::OK);
    assert_eq!(read_bytes(good_b).await, b"opaque-object-b".to_vec());

    let good_a = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_a}"),
            &[("X-QATT-Fetch-Capability", &fetch_capability_a)],
            Vec::new(),
        )
        .await;
    assert_eq!(good_a.status(), StatusCode::OK);
    assert_eq!(read_bytes(good_a).await, b"opaque-object-a".to_vec());

    let log_output = logs.contents();
    assert_absent(&log_output, &fetch_capability_a);
    assert_absent(&log_output, &fetch_capability_b);
    assert_absent(&log_output, "opaque-object-a");
    assert_absent(&log_output, "opaque-object-b");
}

#[tokio::test]
async fn opaque_ciphertext_round_trip_preserves_bytes_and_hides_material_from_logs() {
    let logs = log_capture();
    let fixture = Fixture::new();
    let ciphertext_sentinel = b"QATT_OPAQUE_CIPHERTEXT_SENTINEL_DO_NOT_LOG";
    let plaintext_sentinel = "QATT_PLAINTEXT_SENTINEL_NEVER_SUBMITTED";
    let (_, resume_token, locator_ref, fetch_capability) =
        commit_one_part(&fixture, 906, ciphertext_sentinel).await;

    let fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref}"),
            &[("X-QATT-Fetch-Capability", &fetch_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(fetch).await, ciphertext_sentinel.to_vec());

    let audit_json = serde_json::to_string(&fixture.state.audit_snapshot()).unwrap();
    let log_output = logs.contents();
    for output in [&audit_json, &log_output] {
        assert_absent(output, &resume_token);
        assert_absent(output, &fetch_capability);
        assert_absent(output, &locator_ref);
        assert_absent(
            output,
            std::str::from_utf8(ciphertext_sentinel).expect("sentinel utf8"),
        );
        assert_absent(output, plaintext_sentinel);
    }
}

#[tokio::test]
async fn expiry_cleanup_rejects_fail_closed_and_removes_stale_material() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(907, b"stale-session-bytes");
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let upload = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_eq!(upload.status(), StatusCode::OK);
    fixture.clock.advance(10);

    let expired_status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(expired_status.status(), StatusCode::GONE);
    let expired_status_body = read_json(expired_status).await;
    assert_eq!(expired_status_body["reason_code"], "REJECT_QATTSVC_EXPIRED");
    assert!(!part_path(&fixture, &session_id, 0).exists());

    let fixture = Fixture::new();
    let (_, _, locator_ref, fetch_capability) =
        commit_one_part(&fixture, 908, b"stale-object-bytes").await;
    assert!(object_bytes_path(&fixture, &locator_ref).exists());
    fixture.clock.advance(10);
    let expired_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref}"),
            &[("X-QATT-Fetch-Capability", &fetch_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(expired_fetch.status(), StatusCode::GONE);
    let expired_fetch_body = read_json(expired_fetch).await;
    assert_eq!(expired_fetch_body["reason_code"], "REJECT_QATTSVC_EXPIRED");
    assert!(!object_bytes_path(&fixture, &locator_ref).exists());

    let audit_json = serde_json::to_string(&fixture.state.audit_snapshot()).unwrap();
    assert!(audit_json.contains("session_expired") || audit_json.contains("object_expired"));
    assert_absent(&audit_json, &resume_token);
    assert_absent(&audit_json, &fetch_capability);
}
