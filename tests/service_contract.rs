use std::sync::Arc;

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
    clock: TestClock,
    disk: TestDiskSpace,
}

impl Fixture {
    fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let config = Config {
            storage_root: tempdir.path().join("data"),
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
        };
        let clock = TestClock::new(1_700_000_000);
        let disk = TestDiskSpace::new(u64::MAX / 4);
        let state =
            AppState::new_with_disk_space(config, Arc::new(clock.clone()), Arc::new(disk.clone()))
                .expect("state");
        let app = build_router(state.clone());
        Self {
            _tempdir: tempdir,
            app,
            state,
            clock,
            disk,
        }
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
async fn audit_log_redacts_secrets_and_plaintext() {
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
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap().to_owned();
    let audit_json = serde_json::to_string(&fixture.state.audit_snapshot()).unwrap();
    assert!(!audit_json.contains(&resume_token));
    assert!(!audit_json.contains(&fetch_capability));
    assert!(!audit_json.contains("ciphertext-not-logged"));
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
