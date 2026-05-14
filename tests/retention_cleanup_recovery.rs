use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::header::CONTENT_LENGTH;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use qsl_attachments::{
    build_router, sha512_merkle_root, AppState, CommitRequest, Config, CreateSessionRequest,
    PartSizeClass, RetentionClass, SessionStatusResponse, TestClock, TestDiskSpace,
};
use tempfile::TempDir;
use tower::ServiceExt;

struct Fixture {
    _tempdir: TempDir,
    app: axum::Router,
    config: Config,
    storage_root: PathBuf,
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
            max_open_sessions: 8,
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
        let app = build_router(state);
        Self {
            _tempdir: tempdir,
            app,
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
        json_request_on(&self.app, method, uri, headers, body).await
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

async fn json_request_on(
    app: &axum::Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: serde_json::Value,
) -> axum::response::Response {
    raw_json_request_on(app, method, uri, headers, body.to_string()).await
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

fn two_part_payload(
    seed: u64,
    first: &[u8],
    second: &[u8],
    retention_class: RetentionClass,
) -> (Vec<Vec<u8>>, CreateSessionRequest) {
    let mut first_part = vec![0u8; PartSizeClass::P64k.bytes() as usize];
    first_part[..first.len()].copy_from_slice(first);
    let second_part = second.to_vec();
    let parts = vec![first_part, second_part];
    let request = CreateSessionRequest {
        attachment_id: attachment_id(seed),
        ciphertext_len: (parts[0].len() + parts[1].len()) as u64,
        part_size_class: PartSizeClass::P64k,
        part_count: 2,
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

async fn upload_part(
    app: &axum::Router,
    session_id: &str,
    resume_token: &str,
    part_index: u32,
    body: &[u8],
) {
    let response = bytes_request_on(
        app,
        Method::PUT,
        &format!("/v1/attachments/sessions/{session_id}/parts/{part_index}"),
        &[("X-QATT-Resume-Token", resume_token)],
        body.to_vec(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

async fn commit_one_part(
    fixture: &Fixture,
    seed: u64,
    body: &[u8],
    retention_class: RetentionClass,
) -> (String, String, String, String) {
    let (parts, request) = one_part_payload(seed, body, retention_class);
    let (session_id, resume_token) = create_session(fixture, &request).await;
    upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
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
    let body: serde_json::Value = read_json(commit).await;
    (
        session_id,
        resume_token,
        body["locator_ref"].as_str().unwrap().to_owned(),
        body["fetch_capability"].as_str().unwrap().to_owned(),
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

fn object_dir(fixture: &Fixture, locator_ref: &str) -> PathBuf {
    fixture.storage_root.join("objects").join(locator_ref)
}

fn object_bytes_path(fixture: &Fixture, locator_ref: &str) -> PathBuf {
    object_dir(fixture, locator_ref).join("ciphertext.bin")
}

fn object_meta_path(fixture: &Fixture, locator_ref: &str) -> PathBuf {
    object_dir(fixture, locator_ref).join("object.json")
}

#[tokio::test]
async fn rejected_malformed_json_and_capability_requests_leave_no_recoverable_state() {
    let fixture = Fixture::new();
    let malformed = fixture
        .raw_json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            format!(
                "{{\"attachment_id\":\"{}\",\"descriptor\":\"reject-no-persist\"",
                attachment_id(1001)
            ),
        )
        .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let malformed_body: serde_json::Value = read_json(malformed).await;
    assert_eq!(
        malformed_body["reason_code"],
        "REJECT_QATTSVC_MALFORMED_JSON"
    );
    assert_eq!(entry_count(&fixture.storage_root, "sessions"), 0);
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);

    let (_restarted_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.resumable_sessions, 0);
    assert_eq!(recovery.recovered_committed_objects, 0);
    assert_eq!(entry_count(&fixture.storage_root, "sessions"), 0);
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);

    let (parts, request) = one_part_payload(1002, b"wrong-capability", RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let missing = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[],
            parts[0].clone(),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::FORBIDDEN);
    assert!(!part_path(&fixture, &session_id, 0).exists());

    let wrong = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[(
                "X-QATT-Resume-Token",
                "wrongwrongwrongwrongwrongwrongwrongwrong",
            )],
            parts[0].clone(),
        )
        .await;
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    assert!(!part_path(&fixture, &session_id, 0).exists());

    let (restarted_app, _restarted_state) = fixture.restart();
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
    assert_eq!(status_body.stored_part_count, 0);
    assert!(!part_path(&fixture, &session_id, 0).exists());
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);
}

#[tokio::test]
async fn expired_committed_object_is_removed_after_cleanup() {
    let fixture = Fixture::new();
    let (_session_id, _resume_token, locator_ref, fetch_capability) =
        commit_one_part(&fixture, 1101, b"expired-ciphertext", RetentionClass::Short).await;
    assert!(object_bytes_path(&fixture, &locator_ref).exists());
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
    assert!(!object_bytes_path(&fixture, &locator_ref).exists());
    let object_meta = fs::read_to_string(object_meta_path(&fixture, &locator_ref)).unwrap();
    assert!(object_meta.contains("\"expired_object\""));
    assert!(!object_meta.contains(&fetch_capability));

    let (restarted_app, _restarted_state) = fixture.restart();
    let still_gone = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{locator_ref}"),
        &[("X-QATT-Fetch-Capability", &fetch_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(still_gone.status(), StatusCode::GONE);
    assert!(!object_bytes_path(&fixture, &locator_ref).exists());
}

#[tokio::test]
async fn unexpired_committed_object_survives_cleanup() {
    let fixture = Fixture::new();
    let (_short_session, _short_resume, short_locator, short_capability) =
        commit_one_part(&fixture, 1201, b"short-lived", RetentionClass::Short).await;
    let (_standard_session, _standard_resume, standard_locator, standard_capability) =
        commit_one_part(&fixture, 1202, b"still-live", RetentionClass::Standard).await;

    fixture.clock.advance(10);
    let expired = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{short_locator}"),
            &[("X-QATT-Fetch-Capability", &short_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(expired.status(), StatusCode::GONE);
    assert!(!object_bytes_path(&fixture, &short_locator).exists());

    let still_live = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{standard_locator}"),
            &[("X-QATT-Fetch-Capability", &standard_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(still_live.status(), StatusCode::OK);
    assert_eq!(read_bytes(still_live).await, b"still-live".to_vec());
    assert!(object_bytes_path(&fixture, &standard_locator).exists());

    let (restarted_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.recovered_committed_objects, 1);
    let after_restart = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{standard_locator}"),
        &[("X-QATT-Fetch-Capability", &standard_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(after_restart.status(), StatusCode::OK);
    assert_eq!(read_bytes(after_restart).await, b"still-live".to_vec());
}

#[tokio::test]
async fn restart_recovery_preserves_only_contract_allowed_state() {
    let fixture = Fixture::new();

    let (coherent_parts, coherent_request) = two_part_payload(
        1301,
        b"coherent-prefix",
        b"coherent-tail",
        RetentionClass::Standard,
    );
    let (coherent_session, coherent_resume) = create_session(&fixture, &coherent_request).await;
    upload_part(
        &fixture.app,
        &coherent_session,
        &coherent_resume,
        0,
        &coherent_parts[0],
    )
    .await;

    let (missing_part_parts, missing_part_request) =
        one_part_payload(1302, b"journaled-part-missing", RetentionClass::Standard);
    let (missing_part_session, missing_part_resume) =
        create_session(&fixture, &missing_part_request).await;
    upload_part(
        &fixture.app,
        &missing_part_session,
        &missing_part_resume,
        0,
        &missing_part_parts[0],
    )
    .await;
    fs::remove_file(part_path(&fixture, &missing_part_session, 0)).unwrap();

    let (_keep_session, _keep_resume, keep_locator, keep_capability) = commit_one_part(
        &fixture,
        1303,
        b"recoverable-object",
        RetentionClass::Standard,
    )
    .await;
    let (_drop_session, _drop_resume, drop_locator, drop_capability) = commit_one_part(
        &fixture,
        1304,
        b"incoherent-object",
        RetentionClass::Standard,
    )
    .await;
    fs::remove_file(object_bytes_path(&fixture, &drop_locator)).unwrap();

    let (restarted_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.resumable_sessions, 1);
    assert_eq!(recovery.discarded_incoherent_sessions, 1);
    assert_eq!(recovery.recovered_committed_objects, 1);
    assert_eq!(recovery.discarded_incoherent_objects, 1);

    let coherent_status = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/sessions/{coherent_session}"),
        &[("X-QATT-Resume-Token", &coherent_resume)],
        Vec::new(),
    )
    .await;
    assert_eq!(coherent_status.status(), StatusCode::OK);
    let coherent_body: SessionStatusResponse = read_json(coherent_status).await;
    assert_eq!(coherent_body.stored_part_count, 1);

    let missing_status = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/sessions/{missing_part_session}"),
        &[("X-QATT-Resume-Token", &missing_part_resume)],
        Vec::new(),
    )
    .await;
    assert_eq!(missing_status.status(), StatusCode::CONFLICT);

    let keep_fetch = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{keep_locator}"),
        &[("X-QATT-Fetch-Capability", &keep_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(keep_fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(keep_fetch).await, b"recoverable-object".to_vec());

    let drop_fetch = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{drop_locator}"),
        &[("X-QATT-Fetch-Capability", &drop_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(drop_fetch.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_and_repeated_fetch_are_deterministic() {
    let fixture = Fixture::new();

    let (parts, request) =
        one_part_payload(1401, b"abort-removes-access", RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    let first_delete = fixture
        .bytes_request(
            Method::DELETE,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(first_delete.status(), StatusCode::OK);
    assert!(!part_path(&fixture, &session_id, 0).exists());

    let second_delete = fixture
        .bytes_request(
            Method::DELETE,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            Vec::new(),
        )
        .await;
    assert_eq!(second_delete.status(), StatusCode::FORBIDDEN);

    let (_expired_session, _expired_resume, locator_ref, fetch_capability) =
        commit_one_part(&fixture, 1402, b"repeat-fetch-gone", RetentionClass::Short).await;
    fixture.clock.advance(10);
    for _ in 0..2 {
        let fetch = fixture
            .bytes_request(
                Method::GET,
                &format!("/v1/attachments/objects/{locator_ref}"),
                &[("X-QATT-Fetch-Capability", &fetch_capability)],
                Vec::new(),
            )
            .await;
        assert_eq!(fetch.status(), StatusCode::GONE);
    }

    let (restarted_app, _restarted_state) = fixture.restart();
    let post_restart_delete = bytes_request_on(
        &restarted_app,
        Method::DELETE,
        &format!("/v1/attachments/sessions/{session_id}"),
        &[("X-QATT-Resume-Token", &resume_token)],
        Vec::new(),
    )
    .await;
    assert_eq!(post_restart_delete.status(), StatusCode::CONFLICT);
    let post_restart_delete_body: serde_json::Value = read_json(post_restart_delete).await;
    assert_eq!(
        post_restart_delete_body["reason_code"],
        "REJECT_QATTSVC_SESSION_STATE"
    );
    let post_restart_fetch = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{locator_ref}"),
        &[("X-QATT-Fetch-Capability", &fetch_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(post_restart_fetch.status(), StatusCode::GONE);
}

#[tokio::test]
async fn wrong_resource_capability_cannot_access_other_resource() {
    let fixture = Fixture::new();
    let (_session_a, _resume_a, locator_a, fetch_capability_a) =
        commit_one_part(&fixture, 1501, b"object-a", RetentionClass::Standard).await;
    let (_session_b, _resume_b, locator_b, fetch_capability_b) =
        commit_one_part(&fixture, 1502, b"object-b", RetentionClass::Standard).await;

    let wrong_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_b}"),
            &[("X-QATT-Fetch-Capability", &fetch_capability_a)],
            Vec::new(),
        )
        .await;
    assert_eq!(wrong_fetch.status(), StatusCode::FORBIDDEN);
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
    assert_eq!(read_bytes(good_b).await, b"object-b".to_vec());

    let (restarted_app, restarted_state) = fixture.restart();
    assert_eq!(
        restarted_state
            .recovery_summary()
            .recovered_committed_objects,
        2
    );
    let good_a = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{locator_a}"),
        &[("X-QATT-Fetch-Capability", &fetch_capability_a)],
        Vec::new(),
    )
    .await;
    assert_eq!(good_a.status(), StatusCode::OK);
    assert_eq!(read_bytes(good_a).await, b"object-a".to_vec());
}
