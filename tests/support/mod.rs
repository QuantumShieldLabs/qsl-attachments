#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

pub struct Fixture {
    _tempdir: TempDir,
    pub app: axum::Router,
    pub state: AppState,
    pub config: Config,
    pub storage_root: PathBuf,
    pub clock: TestClock,
    pub disk: TestDiskSpace,
}

impl Fixture {
    pub fn base_config() -> Config {
        Config {
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
        }
    }

    pub fn new() -> Self {
        Self::with_config(Self::base_config())
    }

    pub fn with_config(mut config: Config) -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        config.storage_root = tempdir.path().join("data");
        let storage_root = config.storage_root.clone();
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
            storage_root,
            clock,
            disk,
        }
    }

    pub fn restart(&self) -> (axum::Router, AppState) {
        let state = AppState::new_with_disk_space(
            self.config.clone(),
            Arc::new(self.clock.clone()),
            Arc::new(self.disk.clone()),
        )
        .expect("restarted state");
        let app = build_router(state.clone());
        (app, state)
    }

    pub async fn json_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> axum::response::Response {
        json_request_on(&self.app, method, uri, headers, body).await
    }

    pub async fn raw_json_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> axum::response::Response {
        raw_json_request_on(&self.app, method, uri, headers, body).await
    }

    pub async fn bytes_request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> axum::response::Response {
        bytes_request_on(&self.app, method, uri, headers, body).await
    }
}

pub async fn json_request_on(
    app: &axum::Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: serde_json::Value,
) -> axum::response::Response {
    raw_json_request_on(app, method, uri, headers, body.to_string()).await
}

pub async fn raw_json_request_on(
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

pub async fn bytes_request_on(
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

pub async fn read_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let body = read_bytes(response).await;
    serde_json::from_slice(&body).expect("json body")
}

pub async fn read_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes()
        .to_vec()
}

pub async fn assert_error(
    response: axum::response::Response,
    status: StatusCode,
    reason_code: &str,
) -> serde_json::Value {
    assert_eq!(response.status(), status);
    let body: serde_json::Value = read_json(response).await;
    assert_eq!(body["reason_code"], reason_code);
    body
}

pub fn attachment_id(seed: u64) -> String {
    format!("{seed:064x}")
}

pub fn one_part_payload(
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

pub async fn create_session(fixture: &Fixture, request: &CreateSessionRequest) -> (String, String) {
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

pub async fn upload_part(
    app: &axum::Router,
    session_id: &str,
    resume_token: &str,
    part_index: u32,
    body: &[u8],
) -> axum::response::Response {
    bytes_request_on(
        app,
        Method::PUT,
        &format!("/v1/attachments/sessions/{session_id}/parts/{part_index}"),
        &[("X-QATT-Resume-Token", resume_token)],
        body.to_vec(),
    )
    .await
}

pub async fn commit_session(
    app: &axum::Router,
    session_id: &str,
    resume_token: &str,
    request: &CreateSessionRequest,
) -> axum::response::Response {
    json_request_on(
        app,
        Method::POST,
        &format!("/v1/attachments/sessions/{session_id}/commit"),
        &[("X-QATT-Resume-Token", resume_token)],
        serde_json::to_value(CommitRequest {
            attachment_id: request.attachment_id.clone(),
            ciphertext_len: request.ciphertext_len,
            part_count: request.part_count,
            integrity_alg: request.integrity_alg.clone(),
            integrity_root: request.integrity_root.clone(),
            retention_class: request.retention_class,
        })
        .unwrap(),
    )
    .await
}

#[derive(Debug, Clone)]
pub struct CommittedObject {
    pub session_id: String,
    pub resume_token: String,
    pub locator_ref: String,
    pub fetch_capability: String,
    pub attachment_id: String,
    pub body: Vec<u8>,
}

pub async fn commit_one_part(
    fixture: &Fixture,
    seed: u64,
    body: &[u8],
    retention_class: RetentionClass,
) -> CommittedObject {
    let (parts, request) = one_part_payload(seed, body, retention_class);
    let (session_id, resume_token) = create_session(fixture, &request).await;
    let upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(upload.status(), StatusCode::OK);
    let commit = commit_session(&fixture.app, &session_id, &resume_token, &request).await;
    assert_eq!(commit.status(), StatusCode::OK);
    let body_json: serde_json::Value = read_json(commit).await;
    CommittedObject {
        session_id,
        resume_token,
        locator_ref: body_json["locator_ref"].as_str().unwrap().to_owned(),
        fetch_capability: body_json["fetch_capability"].as_str().unwrap().to_owned(),
        attachment_id: body_json["attachment_id"].as_str().unwrap().to_owned(),
        body: body.to_vec(),
    }
}

pub async fn fetch_object(
    app: &axum::Router,
    locator_ref: &str,
    fetch_capability: &str,
) -> axum::response::Response {
    jsonless_get(
        app,
        &format!("/v1/attachments/objects/{locator_ref}"),
        &[("X-QATT-Fetch-Capability", fetch_capability)],
    )
    .await
}

pub async fn jsonless_get(
    app: &axum::Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    let mut builder = Request::builder().method(Method::GET).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response")
}

pub fn session_part_count(storage_root: &Path, session_id: &str) -> usize {
    let parts_dir = storage_root.join("sessions").join(session_id).join("parts");
    fs::read_dir(parts_dir)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}

pub fn object_ciphertext(storage_root: &Path, locator_ref: &str) -> Vec<u8> {
    fs::read(
        storage_root
            .join("objects")
            .join(locator_ref)
            .join("ciphertext.bin"),
    )
    .expect("object ciphertext")
}

pub fn dir_size(path: &Path) -> u64 {
    fn walk(path: &Path, total: &mut u64) {
        let Ok(metadata) = fs::metadata(path) else {
            return;
        };
        if metadata.is_file() {
            *total += metadata.len();
            return;
        }
        if !metadata.is_dir() {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            walk(&entry.path(), total);
        }
    }

    let mut total = 0;
    walk(path, &mut total);
    total
}
