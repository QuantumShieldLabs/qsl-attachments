use std::io::{self, Write};
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
    config: Config,
    clock: TestClock,
    disk: TestDiskSpace,
}

impl Fixture {
    fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let storage_root = tempdir.path().join("data");
        let config = Config {
            storage_root: storage_root.clone(),
            max_ciphertext_bytes: 1024 * 1024,
            max_open_sessions: 4,
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
        let disk = TestDiskSpace::new(u64::MAX / 4);
        let state = AppState::new_with_disk_space(
            config.clone(),
            Arc::new(clock.clone()),
            Arc::new(disk.clone()),
        )
        .expect("state");
        let app = build_router(state.clone());
        Self {
            _tempdir: tempdir,
            app,
            state,
            config,
            clock,
            disk,
        }
    }

    fn restart(&self) -> (axum::Router, AppState) {
        let state = AppState::new_with_disk_space(
            self.config.clone(),
            Arc::new(self.clock.clone()),
            Arc::new(self.disk.clone()),
        )
        .expect("restarted state");
        let app = build_router(state.clone());
        (app, state)
    }

    async fn json_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> axum::response::Response {
        raw_json_request_on(&self.app, method, uri, headers, body.to_string()).await
    }

    async fn raw_json_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> axum::response::Response {
        raw_json_request_on(&self.app, method, uri, headers, body).await
    }

    async fn bytes_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> axum::response::Response {
        bytes_request_on(&self.app, method, uri, headers, body).await
    }
}

async fn raw_json_request_on(
    app: &axum::Router,
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
    app.clone()
        .oneshot(builder.body(Body::from(body)).expect("request"))
        .await
        .expect("response")
}

async fn bytes_request_on(
    app: &axum::Router,
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
    app.clone()
        .oneshot(builder.body(Body::from(body)).expect("request"))
        .await
        .expect("response")
}

async fn read_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json body")
}

fn attachment_id(seed: u64) -> String {
    format!("{seed:064x}")
}

fn one_part_payload(
    seed: u64,
    body: &[u8],
    retention_class: RetentionClass,
) -> (Vec<Vec<u8>>, CreateSessionRequest) {
    let parts = vec![body.to_vec()];
    let request = CreateSessionRequest {
        attachment_id: attachment_id(seed),
        ciphertext_len: body.len() as u64,
        part_size_class: PartSizeClass::P64k,
        part_count: 1,
        integrity_alg: "sha512_merkle_v1".to_owned(),
        integrity_root: sha512_merkle_root(&parts),
        retention_class,
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
    let body: serde_json::Value = read_json(response).await;
    (
        body["session_id"].as_str().unwrap().to_owned(),
        body["resume_token"].as_str().unwrap().to_owned(),
    )
}

async fn commit_one_part(
    fixture: &Fixture,
    seed: u64,
    body: &[u8],
    retention_class: RetentionClass,
) -> (String, String, String, String, String) {
    let (parts, request) = one_part_payload(seed, body, retention_class);
    let attachment_id = request.attachment_id.clone();
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
    let commit_body: serde_json::Value = read_json(commit).await;
    (
        session_id,
        resume_token,
        commit_body["locator_ref"].as_str().unwrap().to_owned(),
        commit_body["fetch_capability"].as_str().unwrap().to_owned(),
        attachment_id,
    )
}

fn assert_absent(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "unexpected sensitive marker in output: {needle}"
    );
}

#[tokio::test]
async fn cleanup_recovery_logs_redact_capability_descriptor_ciphertext_plaintext() {
    let logs = log_capture();
    let fixture = Fixture::new();
    let descriptor_sentinel = "QATT_DESCRIPTOR_SENTINEL_SHOULD_NOT_LOG_NA0282";
    let plaintext_sentinel = "QATT_PLAINTEXT_SENTINEL_SHOULD_NOT_LOG_NA0282";
    let ciphertext_sentinel = b"QATT_CIPHERTEXT_SENTINEL_SHOULD_NOT_LOG_NA0282";

    let malformed = fixture
        .raw_json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            format!(
                "{{\"attachment_id\":\"{}\",\"descriptor\":\"{}\",\"plaintext\":\"{}\"",
                attachment_id(2001),
                descriptor_sentinel,
                plaintext_sentinel
            ),
        )
        .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    let (session_id, resume_token, locator_ref, fetch_capability, attachment_id) =
        commit_one_part(&fixture, 2002, ciphertext_sentinel, RetentionClass::Short).await;
    fixture.clock.advance(10);
    let expired = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref}"),
            &[("X-QATT-Fetch-Capability", &fetch_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(expired.status(), StatusCode::GONE);

    let (_restarted_app, restarted_state) = fixture.restart();

    let audit_json = serde_json::to_string(&fixture.state.audit_snapshot()).unwrap();
    let recovery_json = serde_json::to_string(&restarted_state.recovery_summary()).unwrap();
    let log_output = logs.contents();
    let ciphertext_sentinel = std::str::from_utf8(ciphertext_sentinel).unwrap();

    for output in [&log_output, &audit_json, &recovery_json] {
        assert_absent(output, &session_id);
        assert_absent(output, &resume_token);
        assert_absent(output, &locator_ref);
        assert_absent(output, &fetch_capability);
        assert_absent(output, &attachment_id);
        assert_absent(output, descriptor_sentinel);
        assert_absent(output, plaintext_sentinel);
        assert_absent(output, ciphertext_sentinel);
    }
    assert!(
        log_output.contains("object_expired") || audit_json.contains("object_expired"),
        "cleanup path should emit only redacted cleanup evidence"
    );
}
