use std::{fs, path::PathBuf, sync::Arc};

use axum::body::Body;
use axum::http::header::{CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use qsl_attachments::{
    build_router, sha512_merkle_root, AppState, CommitRequest, Config, CreateSessionRequest,
    MissingRange, PartSizeClass, RetentionClass, SessionStatusResponse, TestClock, TestDiskSpace,
    UploadPartResponse,
};
use tempfile::TempDir;
use tower::ServiceExt;

struct Fixture {
    _tempdir: TempDir,
    app: axum::Router,
    state: AppState,
    config: Config,
    storage_root: PathBuf,
    clock: TestClock,
    disk: TestDiskSpace,
}

impl Fixture {
    fn base_config() -> Config {
        Config {
            max_ciphertext_bytes: 101 * 1024 * 1024,
            max_open_sessions: 1,
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

    fn new() -> Self {
        Self::with_config(Self::base_config())
    }

    fn with_config(mut config: Config) -> Self {
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
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder = builder.header("content-type", "application/json");
        self.app
            .clone()
            .oneshot(builder.body(Body::from(body.to_string())).expect("request"))
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
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder = builder.header(CONTENT_LENGTH, body.len().to_string());
        self.app
            .clone()
            .oneshot(builder.body(Body::from(body)).expect("request"))
            .await
            .expect("response")
    }
}

async fn json_request_on(
    app: &axum::Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: serde_json::Value,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder = builder.header("content-type", "application/json");
    app.clone()
        .oneshot(builder.body(Body::from(body.to_string())).expect("request"))
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
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder = builder.header(CONTENT_LENGTH, body.len().to_string());
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

fn two_part_payload() -> (Vec<Vec<u8>>, CreateSessionRequest) {
    let first = vec![b'A'; 65_536];
    let second = b"tail".to_vec();
    let parts = vec![first, second];
    let request = CreateSessionRequest {
        attachment_id: attachment_id(1),
        ciphertext_len: (parts[0].len() + parts[1].len()) as u64,
        part_size_class: PartSizeClass::P64k,
        part_count: 2,
        integrity_alg: "sha512_merkle_v1".to_owned(),
        integrity_root: sha512_merkle_root(&parts),
        retention_class: RetentionClass::Standard,
    };
    (parts, request)
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

#[tokio::test]
async fn operator_policy_surface_is_explicit_and_truthful() {
    let fixture = Fixture::new();
    let surface = fixture.state.operator_policy_surface();
    assert_eq!(surface.service_policy_subject, "operator_scoped_deployment");
    assert_eq!(
        surface.authorization_model,
        "deployment_policy_plus_resource_capability"
    );
    assert_eq!(surface.authorization_header, "reserved_undefined");
    assert_eq!(surface.quota_scope, "deployment_global");
    assert_eq!(surface.resume_token_scope, "single_session");
    assert_eq!(surface.fetch_capability_scope, "single_object");
    assert_eq!(surface.resource_ref_model, "resource_refs_not_principals");
    assert_eq!(surface.principal_model, "no_end_user_service_principal");
    assert_eq!(
        surface.transfer_model,
        "many_transfers_subject_to_deployment_policy_quota"
    );
    assert_eq!(
        surface.max_open_sessions,
        fixture.state.config().max_open_sessions
    );
    assert_eq!(
        surface.max_ciphertext_bytes,
        fixture.state.config().max_ciphertext_bytes
    );
}

#[tokio::test]
async fn create_session_success() {
    let fixture = Fixture::new();
    let (_parts, request) = two_part_payload();
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
    assert_eq!(body["session_state"], "created");
    assert!(body["resume_token"].as_str().unwrap().len() >= 32);
}

#[tokio::test]
async fn upload_part_success() {
    let fixture = Fixture::new();
    let (parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();

    let response = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: UploadPartResponse = read_json(response).await;
    assert_eq!(body.session_state, qsl_attachments::SessionState::Uploading);
    assert_eq!(body.stored_part_count, 1);
}

#[tokio::test]
async fn status_resume_state_visibility() {
    let fixture = Fixture::new();
    let (parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;

    let response = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: SessionStatusResponse = read_json(response).await;
    assert_eq!(body.stored_part_count, 1);
    assert_eq!(
        body.missing_part_ranges,
        vec![MissingRange { start: 1, end: 1 }]
    );
}

#[tokio::test]
async fn commit_success_after_complete_parts() {
    let fixture = Fixture::new();
    let (parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    for (idx, part) in parts.iter().enumerate() {
        let response = fixture
            .bytes_request(
                Method::PUT,
                &format!("/v1/attachments/sessions/{session_id}/parts/{idx}"),
                &[("X-QATT-Resume-Token", resume_token)],
                part.clone(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let commit_request = CommitRequest {
        attachment_id: request.attachment_id,
        ciphertext_len: request.ciphertext_len,
        part_count: request.part_count,
        integrity_alg: request.integrity_alg,
        integrity_root: request.integrity_root,
        retention_class: request.retention_class,
    };
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", resume_token)],
            serde_json::to_value(commit_request).unwrap(),
        )
        .await;
    assert_eq!(commit.status(), StatusCode::OK);
    let body: serde_json::Value = read_json(commit).await;
    assert_eq!(body["object_state"], "committed_object");
    assert_eq!(body["locator_kind"], "service_ref_v1");
}

#[tokio::test]
async fn abort_success_and_post_abort_rejects() {
    let fixture = Fixture::new();
    let (_parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    let abort = fixture
        .bytes_request(
            Method::DELETE,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(abort.status(), StatusCode::OK);
    let status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(status.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = read_json(status).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_RESUME_TOKEN");
}

#[tokio::test]
async fn retrieval_success_only_after_commit() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(2, b"ciphertext", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    let unknown_fetch = fixture
        .bytes_request(
            Method::GET,
            "/v1/attachments/objects/unknownref",
            &[(
                "X-QATT-Fetch-Capability",
                "invalidcapabilityvalueinvalidcapabilityv",
            )],
            Vec::new(),
        )
        .await;
    assert_eq!(unknown_fetch.status(), StatusCode::NOT_FOUND);

    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    let commit_request = CommitRequest {
        attachment_id: request.attachment_id,
        ciphertext_len: request.ciphertext_len,
        part_count: request.part_count,
        integrity_alg: request.integrity_alg,
        integrity_root: request.integrity_root,
        retention_class: request.retention_class,
    };
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", resume_token)],
            serde_json::to_value(commit_request).unwrap(),
        )
        .await;
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap();
    let fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref}"),
            &[("X-QATT-Fetch-Capability", fetch_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(fetch).await, b"ciphertext".to_vec());
}

#[tokio::test]
async fn missing_invalid_resume_token_rejects_without_mutation() {
    let fixture = Fixture::new();
    let (parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    let bad = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/1"),
            &[(
                "X-QATT-Resume-Token",
                "wrongwrongwrongwrongwrongwrongwrongwrong",
            )],
            parts[1].clone(),
        )
        .await;
    assert_eq!(bad.status(), StatusCode::FORBIDDEN);
    let status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    let body: SessionStatusResponse = read_json(status).await;
    assert_eq!(body.stored_part_count, 1);
    assert_eq!(
        body.missing_part_ranges,
        vec![MissingRange { start: 1, end: 1 }]
    );
}

#[tokio::test]
async fn missing_invalid_fetch_capability_rejects_without_mutation() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(3, b"opaque-ciphertext", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", resume_token)],
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
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap();
    let bad_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref}"),
            &[(
                "X-QATT-Fetch-Capability",
                "wrongwrongwrongwrongwrongwrongwrongwrong",
            )],
            Vec::new(),
        )
        .await;
    assert_eq!(bad_fetch.status(), StatusCode::FORBIDDEN);
    let good_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref}"),
            &[("X-QATT-Fetch-Capability", fetch_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(good_fetch.status(), StatusCode::OK);
}

#[tokio::test]
async fn mismatched_part_index_and_shape_reject_without_mutation() {
    let fixture = Fixture::new();
    let (parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    let invalid_index = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/7"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_eq!(invalid_index.status(), StatusCode::BAD_REQUEST);
    let status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    let body: SessionStatusResponse = read_json(status).await;
    assert_eq!(body.stored_part_count, 0);
}

#[tokio::test]
async fn expired_session_and_object_behavior() {
    let fixture = Fixture::new();
    let (_parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture.clock.advance(10);
    let expired = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(expired.status(), StatusCode::GONE);

    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(4, b"expiry-bytes", RetentionClass::Short);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", resume_token)],
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
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap();
    fixture.clock.advance(10);
    let fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref}"),
            &[("X-QATT-Fetch-Capability", fetch_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(fetch.status(), StatusCode::GONE);
}

#[tokio::test]
async fn quota_limit_rejects() {
    let fixture = Fixture::new();
    let (_parts, mut request) = one_part_payload(5, b"01234567890", RetentionClass::Standard);
    request.ciphertext_len = fixture.state.config().max_ciphertext_bytes + 1;
    let response = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = read_json(response).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_QUOTA");
}

#[tokio::test]
async fn deployment_global_open_session_quota_is_shared_across_attachments() {
    let fixture = Fixture::new();
    let (_parts_a, request_a) = one_part_payload(50, b"quota-a", RetentionClass::Standard);
    let create_a = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_a).unwrap(),
        )
        .await;
    assert_eq!(create_a.status(), StatusCode::CREATED);

    let (_parts_b, request_b) = one_part_payload(51, b"quota-b", RetentionClass::Standard);
    let create_b = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_b).unwrap(),
        )
        .await;
    assert_eq!(create_b.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = read_json(create_b).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_QUOTA");
}

#[tokio::test]
async fn deployment_policy_allows_many_transfers_when_quota_allows_them() {
    let mut config = Fixture::base_config();
    config.max_open_sessions = 2;
    let fixture = Fixture::with_config(config);
    let (parts_a, request_a) = one_part_payload(52, b"alpha-transfer", RetentionClass::Standard);
    let create_a = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_a.clone()).unwrap(),
        )
        .await;
    let create_a_body: serde_json::Value = read_json(create_a).await;
    let session_id_a = create_a_body["session_id"].as_str().unwrap();
    let resume_token_a = create_a_body["resume_token"].as_str().unwrap();

    let (parts_b, request_b) = one_part_payload(53, b"bravo-transfer", RetentionClass::Standard);
    let create_b = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_b.clone()).unwrap(),
        )
        .await;
    let create_b_body: serde_json::Value = read_json(create_b).await;
    let session_id_b = create_b_body["session_id"].as_str().unwrap();
    let resume_token_b = create_b_body["resume_token"].as_str().unwrap();

    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id_a}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token_a)],
            parts_a[0].clone(),
        )
        .await;
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id_b}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token_b)],
            parts_b[0].clone(),
        )
        .await;

    let commit_a = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id_a}/commit"),
            &[("X-QATT-Resume-Token", resume_token_a)],
            serde_json::to_value(CommitRequest {
                attachment_id: request_a.attachment_id,
                ciphertext_len: request_a.ciphertext_len,
                part_count: request_a.part_count,
                integrity_alg: request_a.integrity_alg,
                integrity_root: request_a.integrity_root,
                retention_class: request_a.retention_class,
            })
            .unwrap(),
        )
        .await;
    assert_eq!(commit_a.status(), StatusCode::OK);

    let commit_b = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id_b}/commit"),
            &[("X-QATT-Resume-Token", resume_token_b)],
            serde_json::to_value(CommitRequest {
                attachment_id: request_b.attachment_id,
                ciphertext_len: request_b.ciphertext_len,
                part_count: request_b.part_count,
                integrity_alg: request_b.integrity_alg,
                integrity_root: request_b.integrity_root,
                retention_class: request_b.retention_class,
            })
            .unwrap(),
        )
        .await;
    assert_eq!(commit_b.status(), StatusCode::OK);
}

#[tokio::test]
async fn hundred_mib_target_class_create_session_succeeds() {
    let fixture = Fixture::new();
    let plaintext_len = 100_u64 * 1024 * 1024;
    let part_count = plaintext_len.div_ceil((1_048_576_u64) - 16) as u32;
    let ciphertext_len = plaintext_len + (u64::from(part_count) * 16);
    assert!(ciphertext_len <= fixture.state.config().max_ciphertext_bytes);
    let request = CreateSessionRequest {
        attachment_id: attachment_id(13),
        ciphertext_len,
        part_size_class: PartSizeClass::P1024k,
        part_count,
        integrity_alg: "sha512_merkle_v1".to_owned(),
        integrity_root: "0".repeat(128),
        retention_class: RetentionClass::Standard,
    };
    let response = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn create_session_rejects_when_two_copy_disk_headroom_is_missing() {
    let fixture = Fixture::new();
    let (_parts, request) = one_part_payload(10, b"0123456789", RetentionClass::Standard);
    fixture
        .disk
        .set_available_bytes((request.ciphertext_len * 2) - 1);
    let response = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = read_json(response).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_QUOTA");
}

#[tokio::test]
async fn upload_part_disk_pressure_rejects_without_mutation() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(11, b"opaque", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture.disk.set_available_bytes(
        (parts[0].len() as u64) + fixture.state.config().storage_reserve_bytes - 1,
    );
    let bad = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_eq!(bad.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    let body: SessionStatusResponse = read_json(status).await;
    assert_eq!(body.stored_part_count, 0);
}

#[tokio::test]
async fn commit_disk_pressure_rejects_without_mutation() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(12, b"opaque-commit", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    fixture.disk.set_available_bytes(
        request.ciphertext_len + fixture.state.config().storage_reserve_bytes - 1,
    );
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", resume_token)],
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
    assert_eq!(commit.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", resume_token)],
            Vec::new(),
        )
        .await;
    let body: SessionStatusResponse = read_json(status).await;
    assert_eq!(
        body.session_state,
        qsl_attachments::SessionState::Committable
    );
    assert_eq!(body.stored_part_count, 1);
}

#[tokio::test]
async fn valid_single_range_retrieval() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(6, b"abcdefghij", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", resume_token)],
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
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/v1/attachments/objects/{locator_ref}"))
                .header("X-QATT-Fetch-Capability", fetch_capability)
                .header(RANGE, "bytes=2-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers().get(CONTENT_RANGE).unwrap(),
        "bytes 2-5/10"
    );
    assert_eq!(read_bytes(response).await, b"cdef".to_vec());
}

#[tokio::test]
async fn audit_log_redacts_secrets_plaintext_and_full_identifiers() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(7, b"ciphertext-not-logged", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap().to_owned();
    let resume_token = create_body["resume_token"].as_str().unwrap().to_owned();
    let attachment_id = request.attachment_id.clone();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_token)],
            parts[0].clone(),
        )
        .await;
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", &resume_token)],
            serde_json::to_value(CommitRequest {
                attachment_id,
                ciphertext_len: request.ciphertext_len,
                part_count: request.part_count,
                integrity_alg: request.integrity_alg,
                integrity_root: request.integrity_root,
                retention_class: request.retention_class,
            })
            .unwrap(),
        )
        .await;
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap().to_owned();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap().to_owned();
    let audit_json = serde_json::to_string(&fixture.state.audit_snapshot()).unwrap();
    assert!(!audit_json.contains(&resume_token));
    assert!(!audit_json.contains(&fetch_capability));
    assert!(!audit_json.contains("ciphertext-not-logged"));
    assert!(!audit_json.contains(&session_id));
    assert!(!audit_json.contains(request.attachment_id.as_str()));
    assert!(!audit_json.contains(&locator_ref));
    assert!(audit_json.contains("\"session_handle\""), "{audit_json}");
    assert!(audit_json.contains("\"attachment_handle\""), "{audit_json}");
    assert!(audit_json.contains("\"locator_handle\""), "{audit_json}");
}

#[tokio::test]
async fn repeated_invalid_fetch_capability_becomes_abuse_reject() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(8, b"abuse-bytes", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap();
    let resume_token = create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token)],
            parts[0].clone(),
        )
        .await;
    let commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id}/commit"),
            &[("X-QATT-Resume-Token", resume_token)],
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
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap();
    for attempt in 0..3 {
        let response = fixture
            .bytes_request(
                Method::GET,
                &format!("/v1/attachments/objects/{locator_ref}"),
                &[(
                    "X-QATT-Fetch-Capability",
                    "wrongwrongwrongwrongwrongwrongwrongwrong",
                )],
                Vec::new(),
            )
            .await;
        if attempt < 2 {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        } else {
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }
    }
}

#[tokio::test]
async fn canonical_urls_reject_query_string_secret_carriage() {
    let fixture = Fixture::new();
    let (_parts, request) = one_part_payload(9, b"url-secret-check", RetentionClass::Standard);
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/attachments/sessions?resume_token=badidea")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = read_json(response).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_SECRET_URL_PLACEMENT");
}

#[tokio::test]
async fn resume_token_is_scoped_to_one_session() {
    let mut config = Fixture::base_config();
    config.max_open_sessions = 2;
    let fixture = Fixture::with_config(config);
    let (_parts_a, request_a) = one_part_payload(60, b"session-a", RetentionClass::Standard);
    let create_a = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_a).unwrap(),
        )
        .await;
    let create_a_body: serde_json::Value = read_json(create_a).await;
    let resume_token_a = create_a_body["resume_token"].as_str().unwrap();

    let (_parts_b, request_b) = one_part_payload(61, b"session-b", RetentionClass::Standard);
    let create_b = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_b).unwrap(),
        )
        .await;
    let create_b_body: serde_json::Value = read_json(create_b).await;
    let session_id_b = create_b_body["session_id"].as_str().unwrap();
    let resume_token_b = create_b_body["resume_token"].as_str().unwrap();

    let wrong_status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id_b}"),
            &[("X-QATT-Resume-Token", resume_token_a)],
            Vec::new(),
        )
        .await;
    assert_eq!(wrong_status.status(), StatusCode::FORBIDDEN);
    let wrong_body: serde_json::Value = read_json(wrong_status).await;
    assert_eq!(wrong_body["reason_code"], "REJECT_QATTSVC_RESUME_TOKEN");

    let good_status = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id_b}"),
            &[("X-QATT-Resume-Token", resume_token_b)],
            Vec::new(),
        )
        .await;
    assert_eq!(good_status.status(), StatusCode::OK);
}

#[tokio::test]
async fn fetch_capability_is_scoped_to_one_object() {
    let mut config = Fixture::base_config();
    config.max_open_sessions = 2;
    let fixture = Fixture::with_config(config);
    let (parts_a, request_a) = one_part_payload(62, b"object-a", RetentionClass::Standard);
    let create_a = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_a.clone()).unwrap(),
        )
        .await;
    let create_a_body: serde_json::Value = read_json(create_a).await;
    let session_id_a = create_a_body["session_id"].as_str().unwrap();
    let resume_token_a = create_a_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id_a}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token_a)],
            parts_a[0].clone(),
        )
        .await;
    let commit_a = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id_a}/commit"),
            &[("X-QATT-Resume-Token", resume_token_a)],
            serde_json::to_value(CommitRequest {
                attachment_id: request_a.attachment_id,
                ciphertext_len: request_a.ciphertext_len,
                part_count: request_a.part_count,
                integrity_alg: request_a.integrity_alg,
                integrity_root: request_a.integrity_root,
                retention_class: request_a.retention_class,
            })
            .unwrap(),
        )
        .await;
    let commit_a_body: serde_json::Value = read_json(commit_a).await;
    let fetch_capability_a = commit_a_body["fetch_capability"].as_str().unwrap();

    let (parts_b, request_b) = one_part_payload(63, b"object-b", RetentionClass::Standard);
    let create_b = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_b.clone()).unwrap(),
        )
        .await;
    let create_b_body: serde_json::Value = read_json(create_b).await;
    let session_id_b = create_b_body["session_id"].as_str().unwrap();
    let resume_token_b = create_b_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id_b}/parts/0"),
            &[("X-QATT-Resume-Token", resume_token_b)],
            parts_b[0].clone(),
        )
        .await;
    let commit_b = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{session_id_b}/commit"),
            &[("X-QATT-Resume-Token", resume_token_b)],
            serde_json::to_value(CommitRequest {
                attachment_id: request_b.attachment_id,
                ciphertext_len: request_b.ciphertext_len,
                part_count: request_b.part_count,
                integrity_alg: request_b.integrity_alg,
                integrity_root: request_b.integrity_root,
                retention_class: request_b.retention_class,
            })
            .unwrap(),
        )
        .await;
    let commit_b_body: serde_json::Value = read_json(commit_b).await;
    let locator_ref_b = commit_b_body["locator_ref"].as_str().unwrap();
    let fetch_capability_b = commit_b_body["fetch_capability"].as_str().unwrap();

    let wrong_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref_b}"),
            &[("X-QATT-Fetch-Capability", fetch_capability_a)],
            Vec::new(),
        )
        .await;
    assert_eq!(wrong_fetch.status(), StatusCode::FORBIDDEN);
    let wrong_body: serde_json::Value = read_json(wrong_fetch).await;
    assert_eq!(wrong_body["reason_code"], "REJECT_QATTSVC_FETCH_CAPABILITY");

    let good_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_ref_b}"),
            &[("X-QATT-Fetch-Capability", fetch_capability_b)],
            Vec::new(),
        )
        .await;
    assert_eq!(good_fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(good_fetch).await, b"object-b".to_vec());
}

#[tokio::test]
async fn graceful_same_root_restart_recovers_coherent_session_and_discards_orphan_parts() {
    let fixture = Fixture::new();
    let (parts, request) = two_part_payload();
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request.clone()).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap().to_owned();
    let resume_token = create_body["resume_token"].as_str().unwrap().to_owned();

    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_token)],
            parts[0].clone(),
        )
        .await;

    let orphan_path = fixture
        .storage_root
        .join("sessions")
        .join(&session_id)
        .join("parts")
        .join("1.part");
    fs::write(&orphan_path, b"orphaned-staged-bytes").unwrap();

    let (restarted_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.resumable_sessions, 1);
    assert_eq!(recovery.discarded_orphan_part_files, 1);
    assert!(!orphan_path.exists());

    let status = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/sessions/{session_id}"),
        &[("X-QATT-Resume-Token", &resume_token)],
        Vec::new(),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body: SessionStatusResponse = read_json(status).await;
    assert_eq!(
        status_body.session_state,
        qsl_attachments::SessionState::Uploading
    );
    assert_eq!(status_body.stored_part_count, 1);
    assert_eq!(
        status_body.missing_part_ranges,
        vec![MissingRange { start: 1, end: 1 }]
    );

    let upload = bytes_request_on(
        &restarted_app,
        Method::PUT,
        &format!("/v1/attachments/sessions/{session_id}/parts/1"),
        &[("X-QATT-Resume-Token", &resume_token)],
        parts[1].clone(),
    )
    .await;
    assert_eq!(upload.status(), StatusCode::OK);

    let commit = json_request_on(
        &restarted_app,
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
}

#[tokio::test]
async fn restart_discards_incoherent_session_when_journaled_part_is_missing() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(70, b"resume-me", RetentionClass::Standard);
    let create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    let create_body: serde_json::Value = read_json(create).await;
    let session_id = create_body["session_id"].as_str().unwrap().to_owned();
    let resume_token = create_body["resume_token"].as_str().unwrap().to_owned();

    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_token)],
            parts[0].clone(),
        )
        .await;

    let staged_part = fixture
        .storage_root
        .join("sessions")
        .join(&session_id)
        .join("parts")
        .join("0.part");
    fs::remove_file(&staged_part).unwrap();

    let (restarted_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.discarded_incoherent_sessions, 1);

    let status = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/sessions/{session_id}"),
        &[("X-QATT-Resume-Token", &resume_token)],
        Vec::new(),
    )
    .await;
    assert_eq!(status.status(), StatusCode::CONFLICT);
    let status_body: serde_json::Value = read_json(status).await;
    assert_eq!(status_body["reason_code"], "REJECT_QATTSVC_SESSION_STATE");
}

#[tokio::test]
async fn committed_object_recovery_requires_object_json_and_ciphertext_bin() {
    let fixture = Fixture::new();

    let (keep_parts, keep_request) = one_part_payload(71, b"keep-object", RetentionClass::Standard);
    let keep_create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(keep_request.clone()).unwrap(),
        )
        .await;
    let keep_create_body: serde_json::Value = read_json(keep_create).await;
    let keep_session_id = keep_create_body["session_id"].as_str().unwrap();
    let keep_resume_token = keep_create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{keep_session_id}/parts/0"),
            &[("X-QATT-Resume-Token", keep_resume_token)],
            keep_parts[0].clone(),
        )
        .await;
    let keep_commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{keep_session_id}/commit"),
            &[("X-QATT-Resume-Token", keep_resume_token)],
            serde_json::to_value(CommitRequest {
                attachment_id: keep_request.attachment_id,
                ciphertext_len: keep_request.ciphertext_len,
                part_count: keep_request.part_count,
                integrity_alg: keep_request.integrity_alg,
                integrity_root: keep_request.integrity_root,
                retention_class: keep_request.retention_class,
            })
            .unwrap(),
        )
        .await;
    let keep_commit_body: serde_json::Value = read_json(keep_commit).await;
    let keep_locator = keep_commit_body["locator_ref"].as_str().unwrap().to_owned();
    let keep_fetch_capability = keep_commit_body["fetch_capability"]
        .as_str()
        .unwrap()
        .to_owned();

    let (drop_bytes_parts, drop_bytes_request) =
        one_part_payload(72, b"missing-bytes", RetentionClass::Standard);
    let drop_bytes_create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(drop_bytes_request.clone()).unwrap(),
        )
        .await;
    let drop_bytes_create_body: serde_json::Value = read_json(drop_bytes_create).await;
    let drop_bytes_session_id = drop_bytes_create_body["session_id"].as_str().unwrap();
    let drop_bytes_resume_token = drop_bytes_create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{drop_bytes_session_id}/parts/0"),
            &[("X-QATT-Resume-Token", drop_bytes_resume_token)],
            drop_bytes_parts[0].clone(),
        )
        .await;
    let drop_bytes_commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{drop_bytes_session_id}/commit"),
            &[("X-QATT-Resume-Token", drop_bytes_resume_token)],
            serde_json::to_value(CommitRequest {
                attachment_id: drop_bytes_request.attachment_id,
                ciphertext_len: drop_bytes_request.ciphertext_len,
                part_count: drop_bytes_request.part_count,
                integrity_alg: drop_bytes_request.integrity_alg,
                integrity_root: drop_bytes_request.integrity_root,
                retention_class: drop_bytes_request.retention_class,
            })
            .unwrap(),
        )
        .await;
    let drop_bytes_commit_body: serde_json::Value = read_json(drop_bytes_commit).await;
    let drop_bytes_locator = drop_bytes_commit_body["locator_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let drop_bytes_fetch_capability = drop_bytes_commit_body["fetch_capability"]
        .as_str()
        .unwrap()
        .to_owned();

    let (drop_meta_parts, drop_meta_request) =
        one_part_payload(73, b"missing-meta", RetentionClass::Standard);
    let drop_meta_create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(drop_meta_request.clone()).unwrap(),
        )
        .await;
    let drop_meta_create_body: serde_json::Value = read_json(drop_meta_create).await;
    let drop_meta_session_id = drop_meta_create_body["session_id"].as_str().unwrap();
    let drop_meta_resume_token = drop_meta_create_body["resume_token"].as_str().unwrap();
    fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{drop_meta_session_id}/parts/0"),
            &[("X-QATT-Resume-Token", drop_meta_resume_token)],
            drop_meta_parts[0].clone(),
        )
        .await;
    let drop_meta_commit = fixture
        .json_request(
            Method::POST,
            &format!("/v1/attachments/sessions/{drop_meta_session_id}/commit"),
            &[("X-QATT-Resume-Token", drop_meta_resume_token)],
            serde_json::to_value(CommitRequest {
                attachment_id: drop_meta_request.attachment_id,
                ciphertext_len: drop_meta_request.ciphertext_len,
                part_count: drop_meta_request.part_count,
                integrity_alg: drop_meta_request.integrity_alg,
                integrity_root: drop_meta_request.integrity_root,
                retention_class: drop_meta_request.retention_class,
            })
            .unwrap(),
        )
        .await;
    let drop_meta_commit_body: serde_json::Value = read_json(drop_meta_commit).await;
    let drop_meta_locator = drop_meta_commit_body["locator_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let drop_meta_fetch_capability = drop_meta_commit_body["fetch_capability"]
        .as_str()
        .unwrap()
        .to_owned();

    fs::remove_file(
        fixture
            .storage_root
            .join("objects")
            .join(&drop_bytes_locator)
            .join("ciphertext.bin"),
    )
    .unwrap();
    fs::remove_file(
        fixture
            .storage_root
            .join("objects")
            .join(&drop_meta_locator)
            .join("object.json"),
    )
    .unwrap();

    let (restarted_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.recovered_committed_objects, 1);
    assert_eq!(recovery.discarded_incoherent_objects, 1);
    assert_eq!(recovery.discarded_orphan_object_dirs, 1);

    let keep_fetch = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{keep_locator}"),
        &[("X-QATT-Fetch-Capability", &keep_fetch_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(keep_fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(keep_fetch).await, b"keep-object".to_vec());

    let drop_bytes_fetch = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{drop_bytes_locator}"),
        &[("X-QATT-Fetch-Capability", &drop_bytes_fetch_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(drop_bytes_fetch.status(), StatusCode::NOT_FOUND);
    let drop_bytes_body: serde_json::Value = read_json(drop_bytes_fetch).await;
    assert_eq!(
        drop_bytes_body["reason_code"],
        "REJECT_QATTSVC_LOCATOR_UNKNOWN"
    );

    let drop_meta_fetch = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{drop_meta_locator}"),
        &[("X-QATT-Fetch-Capability", &drop_meta_fetch_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(drop_meta_fetch.status(), StatusCode::NOT_FOUND);
    let drop_meta_body: serde_json::Value = read_json(drop_meta_fetch).await;
    assert_eq!(
        drop_meta_body["reason_code"],
        "REJECT_QATTSVC_LOCATOR_UNKNOWN"
    );
}

#[test]
fn durability_docs_and_validation_evidence_state_restart_backup_and_unsupported_cases_truthfully() {
    let readme = include_str!("../README.md");
    let start_here = include_str!("../START_HERE.md");
    let contract = include_str!("../docs/NA-0009_durability_recovery_contract.md");
    let evidence = include_str!("./NA-0010A_durability_recovery_validation_evidence.md");

    assert!(readme.contains("graceful same-root restart is in scope"));
    assert!(readme.contains("cold full-root backup/restore plus matching service configuration"));
    assert!(readme.contains("hot/live backup and partial restore remain unsupported"));
    assert!(start_here.contains("graceful same-root restart is in scope"));
    assert!(
        start_here.contains("cold full-root backup/restore plus matching service configuration")
    );
    assert!(start_here.contains("hot/live backup and partial restore remain unsupported"));
    assert!(contract.contains(
        "A committed object is recoverable only when both `object.json` and `ciphertext.bin` are present"
    ));
    assert!(contract.contains("Hot/live backup while mutations continue is unsupported"));
    assert!(contract.contains("Partial restore of only sessions"));
    assert!(contract.contains("Restored open sessions are best-effort only"));
    assert!(evidence.contains("graceful same-root restart"));
    assert!(evidence.contains("`object.json` and `ciphertext.bin`"));
    assert!(evidence.contains("Hot/live backup and partial restore remain unsupported"));
    assert!(evidence.contains("audit_log_redacts_secrets_plaintext_and_full_identifiers"));
}
