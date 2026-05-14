use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::header::CONTENT_LENGTH;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use qsl_attachments::{
    build_router, sha512_merkle_root, AppState, CommitRequest, Config, CreateSessionRequest,
    MissingRange, PartSizeClass, RetentionClass, SessionState, SessionStatusResponse, TestClock,
    TestDiskSpace,
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
    fn base_config() -> Config {
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
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
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

async fn commit_session_on(
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

async fn commit_one_part(
    fixture: &Fixture,
    seed: u64,
    body: &[u8],
    retention_class: RetentionClass,
) -> (String, String, String, String) {
    let (parts, request) = one_part_payload(seed, body, retention_class);
    let (session_id, resume_token) = create_session(fixture, &request).await;
    upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    let commit = commit_session_on(&fixture.app, &session_id, &resume_token, &request).await;
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
    let path = root.join(child);
    if !path.exists() {
        return 0;
    }
    fs::read_dir(path).expect("read dir").count()
}

fn tree_stats(path: &Path) -> (usize, u64) {
    if !path.exists() {
        return (0, 0);
    }
    let metadata = fs::metadata(path).expect("metadata");
    if metadata.is_file() {
        return (1, metadata.len());
    }
    let mut files = 0;
    let mut bytes = 0;
    for entry in fs::read_dir(path).expect("read dir") {
        let entry = entry.expect("dir entry");
        let (child_files, child_bytes) = tree_stats(&entry.path());
        files += child_files;
        bytes += child_bytes;
    }
    (files, bytes)
}

fn session_dir(fixture: &Fixture, session_id: &str) -> PathBuf {
    fixture.storage_root.join("sessions").join(session_id)
}

fn part_path(fixture: &Fixture, session_id: &str, part_index: u32) -> PathBuf {
    session_dir(fixture, session_id)
        .join("parts")
        .join(format!("{part_index}.part"))
}

fn object_dir(fixture: &Fixture, locator_ref: &str) -> PathBuf {
    fixture.storage_root.join("objects").join(locator_ref)
}

fn object_bytes_path(fixture: &Fixture, locator_ref: &str) -> PathBuf {
    object_dir(fixture, locator_ref).join("ciphertext.bin")
}

async fn assert_quota_response(response: axum::response::Response) {
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = read_json(response).await;
    assert_eq!(body["reason_code"], "REJECT_QATTSVC_QUOTA");
}

#[tokio::test]
async fn quota_rejects_do_not_persist_objects_sessions_or_parts() {
    let mut config = Fixture::base_config();
    config.max_ciphertext_bytes = 8;
    let fixture = Fixture::with_config(config);
    let (_parts, request) = one_part_payload(280_301, b"quota-overflow", RetentionClass::Standard);

    let response = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    assert_quota_response(response).await;
    assert_eq!(entry_count(&fixture.storage_root, "sessions"), 0);
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);

    let (_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.resumable_sessions, 0);
    assert_eq!(recovery.recovered_committed_objects, 0);
    assert_eq!(entry_count(&fixture.storage_root, "sessions"), 0);
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);
}

#[tokio::test]
async fn open_session_quota_rejects_and_release_is_deterministic() {
    let mut config = Fixture::base_config();
    config.max_open_sessions = 1;
    let fixture = Fixture::with_config(config);
    let (_parts_a, request_a) = one_part_payload(280_302, b"quota-a", RetentionClass::Standard);
    let (session_a, resume_a) = create_session(&fixture, &request_a).await;

    let (_parts_b, request_b) = one_part_payload(280_303, b"quota-b", RetentionClass::Standard);
    let reject = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_b.clone()).unwrap(),
        )
        .await;
    assert_quota_response(reject).await;
    assert_eq!(entry_count(&fixture.storage_root, "sessions"), 1);
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);

    let abort = fixture
        .bytes_request(
            Method::DELETE,
            &format!("/v1/attachments/sessions/{session_a}"),
            &[("X-QATT-Resume-Token", &resume_a)],
            Vec::new(),
        )
        .await;
    assert_eq!(abort.status(), StatusCode::OK);

    let release_create = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request_b).unwrap(),
        )
        .await;
    assert_eq!(release_create.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn low_headroom_create_upload_commit_rejects_are_fail_closed() {
    let create_fixture = Fixture::new();
    let (_parts, request) = one_part_payload(280_304, b"create-pressure", RetentionClass::Standard);
    create_fixture.disk.set_available_bytes(
        request
            .ciphertext_len
            .saturating_mul(2)
            .saturating_add(create_fixture.config.storage_reserve_bytes)
            - 1,
    );
    let create_reject = create_fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(request).unwrap(),
        )
        .await;
    assert_quota_response(create_reject).await;
    assert_eq!(entry_count(&create_fixture.storage_root, "sessions"), 0);
    assert_eq!(entry_count(&create_fixture.storage_root, "objects"), 0);

    let upload_fixture = Fixture::new();
    let (upload_parts, upload_request) =
        one_part_payload(280_305, b"upload-pressure", RetentionClass::Standard);
    let (upload_session, upload_resume) = create_session(&upload_fixture, &upload_request).await;
    upload_fixture.disk.set_available_bytes(
        upload_parts[0].len().try_into().unwrap_or(u64::MAX)
            + upload_fixture.config.storage_reserve_bytes
            - 1,
    );
    let upload_reject = upload_fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{upload_session}/parts/0"),
            &[("X-QATT-Resume-Token", &upload_resume)],
            upload_parts[0].clone(),
        )
        .await;
    assert_quota_response(upload_reject).await;
    assert!(!part_path(&upload_fixture, &upload_session, 0).exists());
    let upload_status = upload_fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{upload_session}"),
            &[("X-QATT-Resume-Token", &upload_resume)],
            Vec::new(),
        )
        .await;
    let upload_body: SessionStatusResponse = read_json(upload_status).await;
    assert_eq!(upload_body.stored_part_count, 0);
    assert_eq!(entry_count(&upload_fixture.storage_root, "objects"), 0);

    let commit_fixture = Fixture::new();
    let (commit_parts, commit_request) =
        one_part_payload(280_306, b"commit-pressure", RetentionClass::Standard);
    let (commit_session, commit_resume) = create_session(&commit_fixture, &commit_request).await;
    upload_part(
        &commit_fixture.app,
        &commit_session,
        &commit_resume,
        0,
        &commit_parts[0],
    )
    .await;
    commit_fixture.disk.set_available_bytes(
        commit_request.ciphertext_len + commit_fixture.config.storage_reserve_bytes - 1,
    );
    let commit_reject = commit_session_on(
        &commit_fixture.app,
        &commit_session,
        &commit_resume,
        &commit_request,
    )
    .await;
    assert_quota_response(commit_reject).await;
    assert_eq!(entry_count(&commit_fixture.storage_root, "objects"), 0);
    let commit_status = commit_fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{commit_session}"),
            &[("X-QATT-Resume-Token", &commit_resume)],
            Vec::new(),
        )
        .await;
    let commit_body: SessionStatusResponse = read_json(commit_status).await;
    assert_eq!(commit_body.session_state, SessionState::Committable);
    assert_eq!(commit_body.stored_part_count, 1);
    assert!(part_path(&commit_fixture, &commit_session, 0).exists());
}

#[tokio::test]
async fn quota_disk_rejected_writes_do_not_resurrect_after_restart() {
    let fixture = Fixture::new();
    let (_quota_parts, quota_request) =
        one_part_payload(280_307, b"quota-restart", RetentionClass::Standard);
    fixture.disk.set_available_bytes(0);
    let quota_reject = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(quota_request).unwrap(),
        )
        .await;
    assert_quota_response(quota_reject).await;

    fixture.disk.set_available_bytes(u64::MAX / 4);
    let (parts, request) = one_part_payload(280_308, b"restart-pressure", RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    fixture
        .disk
        .set_available_bytes(parts[0].len() as u64 + fixture.config.storage_reserve_bytes - 1);
    let upload_reject = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_token)],
            parts[0].clone(),
        )
        .await;
    assert_quota_response(upload_reject).await;

    let (restarted_app, restarted_state) = fixture.restart();
    assert_eq!(restarted_state.recovery_summary().resumable_sessions, 1);
    assert!(!part_path(&fixture, &session_id, 0).exists());
    let restarted_status = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/sessions/{session_id}"),
        &[("X-QATT-Resume-Token", &resume_token)],
        Vec::new(),
    )
    .await;
    let restarted_body: SessionStatusResponse = read_json(restarted_status).await;
    assert_eq!(restarted_body.stored_part_count, 0);

    fixture.disk.set_available_bytes(u64::MAX / 4);
    upload_part(&restarted_app, &session_id, &resume_token, 0, &parts[0]).await;
    fixture
        .disk
        .set_available_bytes(request.ciphertext_len + fixture.config.storage_reserve_bytes - 1);
    let commit_reject =
        commit_session_on(&restarted_app, &session_id, &resume_token, &request).await;
    assert_quota_response(commit_reject).await;
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);

    let (post_reject_app, post_reject_state) = fixture.restart();
    assert_eq!(
        post_reject_state
            .recovery_summary()
            .recovered_committed_objects,
        0
    );
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 0);
    let post_reject_status = bytes_request_on(
        &post_reject_app,
        Method::GET,
        &format!("/v1/attachments/sessions/{session_id}"),
        &[("X-QATT-Resume-Token", &resume_token)],
        Vec::new(),
    )
    .await;
    let post_reject_body: SessionStatusResponse = read_json(post_reject_status).await;
    assert_eq!(post_reject_body.session_state, SessionState::Committable);
    assert_eq!(post_reject_body.stored_part_count, 1);
}

#[tokio::test]
async fn cleanup_under_pressure_preserves_unexpired_valid_committed_object() {
    let fixture = Fixture::new();
    let (_short_session, _short_resume, short_locator, short_capability) = commit_one_part(
        &fixture,
        280_309,
        b"expired-under-pressure",
        RetentionClass::Short,
    )
    .await;
    let (_live_session, _live_resume, live_locator, live_capability) = commit_one_part(
        &fixture,
        280_310,
        b"live-under-pressure",
        RetentionClass::Standard,
    )
    .await;

    fixture.clock.advance(10);
    fixture.disk.set_available_bytes(0);
    let (_parts, pressure_request) =
        one_part_payload(280_311, b"pressure-trigger", RetentionClass::Standard);
    let pressure_reject = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(pressure_request).unwrap(),
        )
        .await;
    assert_quota_response(pressure_reject).await;

    assert!(!object_bytes_path(&fixture, &short_locator).exists());
    let short_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{short_locator}"),
            &[("X-QATT-Fetch-Capability", &short_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(short_fetch.status(), StatusCode::GONE);

    let live_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{live_locator}"),
            &[("X-QATT-Fetch-Capability", &live_capability)],
            Vec::new(),
        )
        .await;
    assert_eq!(live_fetch.status(), StatusCode::OK);
    assert_eq!(
        read_bytes(live_fetch).await,
        b"live-under-pressure".to_vec()
    );
    assert!(object_bytes_path(&fixture, &live_locator).exists());
}

#[tokio::test]
async fn partial_write_recovery_boundary_is_explicit() {
    let fixture = Fixture::new();
    let (parts, request) = two_part_payload(
        280_312,
        b"partial-session-prefix",
        b"partial-session-tail",
        RetentionClass::Standard,
    );
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let orphan_part = part_path(&fixture, &session_id, 0);
    fs::create_dir_all(orphan_part.parent().unwrap()).unwrap();
    fs::write(&orphan_part, &parts[0]).unwrap();

    let (_good_session, _good_resume, bad_locator, bad_capability) = commit_one_part(
        &fixture,
        280_313,
        b"incomplete-object",
        RetentionClass::Standard,
    )
    .await;
    fs::write(object_bytes_path(&fixture, &bad_locator), b"short").unwrap();

    let orphan_locator = "orphanPartialObjectNA0283";
    fs::create_dir_all(object_dir(&fixture, orphan_locator)).unwrap();
    fs::write(
        object_bytes_path(&fixture, orphan_locator),
        b"orphan-ciphertext",
    )
    .unwrap();

    let (restarted_app, restarted_state) = fixture.restart();
    let recovery = restarted_state.recovery_summary();
    assert_eq!(recovery.resumable_sessions, 1);
    assert_eq!(recovery.discarded_orphan_part_files, 1);
    assert_eq!(recovery.discarded_incoherent_objects, 1);
    assert_eq!(recovery.discarded_orphan_object_dirs, 1);
    assert!(!orphan_part.exists());
    assert!(!object_dir(&fixture, orphan_locator).exists());
    assert!(!object_dir(&fixture, &bad_locator).exists());

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
    assert_eq!(status_body.session_state, SessionState::Created);
    assert_eq!(status_body.stored_part_count, 0);
    assert_eq!(
        status_body.missing_part_ranges,
        vec![MissingRange { start: 0, end: 1 }]
    );

    let bad_fetch = bytes_request_on(
        &restarted_app,
        Method::GET,
        &format!("/v1/attachments/objects/{bad_locator}"),
        &[("X-QATT-Fetch-Capability", &bad_capability)],
        Vec::new(),
    )
    .await;
    assert_eq!(bad_fetch.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn wrong_resource_capability_cannot_bypass_quota_or_fetch_other_object() {
    let mut config = Fixture::base_config();
    config.max_open_sessions = 2;
    let fixture = Fixture::with_config(config);
    let (parts_a, request_a) = one_part_payload(280_314, b"object-a", RetentionClass::Standard);
    let (parts_b, request_b) = one_part_payload(280_315, b"object-b", RetentionClass::Standard);
    let (session_a, resume_a) = create_session(&fixture, &request_a).await;
    let (session_b, resume_b) = create_session(&fixture, &request_b).await;

    fixture.disk.set_available_bytes(0);
    let wrong_upload = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_b}/parts/0"),
            &[("X-QATT-Resume-Token", &resume_a)],
            parts_b[0].clone(),
        )
        .await;
    assert_eq!(wrong_upload.status(), StatusCode::FORBIDDEN);
    let wrong_upload_body: serde_json::Value = read_json(wrong_upload).await;
    assert_eq!(
        wrong_upload_body["reason_code"],
        "REJECT_QATTSVC_RESUME_TOKEN"
    );
    assert!(!part_path(&fixture, &session_b, 0).exists());

    fixture.disk.set_available_bytes(u64::MAX / 4);
    upload_part(&fixture.app, &session_a, &resume_a, 0, &parts_a[0]).await;
    let commit_a = commit_session_on(&fixture.app, &session_a, &resume_a, &request_a).await;
    assert_eq!(commit_a.status(), StatusCode::OK);
    let commit_a_body: serde_json::Value = read_json(commit_a).await;
    let capability_a = commit_a_body["fetch_capability"].as_str().unwrap();

    upload_part(&fixture.app, &session_b, &resume_b, 0, &parts_b[0]).await;
    let commit_b = commit_session_on(&fixture.app, &session_b, &resume_b, &request_b).await;
    assert_eq!(commit_b.status(), StatusCode::OK);
    let commit_b_body: serde_json::Value = read_json(commit_b).await;
    let locator_b = commit_b_body["locator_ref"].as_str().unwrap();
    let capability_b = commit_b_body["fetch_capability"].as_str().unwrap();

    let wrong_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_b}"),
            &[("X-QATT-Fetch-Capability", capability_a)],
            Vec::new(),
        )
        .await;
    assert_eq!(wrong_fetch.status(), StatusCode::FORBIDDEN);
    let wrong_fetch_body: serde_json::Value = read_json(wrong_fetch).await;
    assert_eq!(
        wrong_fetch_body["reason_code"],
        "REJECT_QATTSVC_FETCH_CAPABILITY"
    );

    let good_fetch = fixture
        .bytes_request(
            Method::GET,
            &format!("/v1/attachments/objects/{locator_b}"),
            &[("X-QATT-Fetch-Capability", capability_b)],
            Vec::new(),
        )
        .await;
    assert_eq!(good_fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(good_fetch).await, b"object-b".to_vec());
}

#[tokio::test]
async fn bounded_abuse_loop_has_no_panic_and_no_unbounded_growth() {
    let fixture = Fixture::new();
    let (_session_id, _resume_token, locator_ref, _fetch_capability) = commit_one_part(
        &fixture,
        280_316,
        b"bounded-abuse",
        RetentionClass::Standard,
    )
    .await;
    let before = tree_stats(&fixture.storage_root);

    fixture.disk.set_available_bytes(0);
    for idx in 0..20 {
        let (_parts, request) = one_part_payload(
            280_400 + idx,
            format!("quota-loop-{idx}").as_bytes(),
            RetentionClass::Standard,
        );
        let response = fixture
            .json_request(
                Method::POST,
                "/v1/attachments/sessions",
                &[],
                serde_json::to_value(request).unwrap(),
            )
            .await;
        assert_quota_response(response).await;
    }

    for attempt in 0..20 {
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

    let after = tree_stats(&fixture.storage_root);
    assert_eq!(after, before);
    assert_eq!(entry_count(&fixture.storage_root, "sessions"), 0);
    assert_eq!(entry_count(&fixture.storage_root, "objects"), 1);
}

#[test]
fn existing_reject_taxonomy_and_retention_harnesses_remain_green() {
    let reject_taxonomy = include_str!("reject_taxonomy_harness.rs");
    let retention_recovery = include_str!("retention_cleanup_recovery.rs");
    let retention_logging = include_str!("retention_cleanup_logging.rs");
    assert!(reject_taxonomy
        .contains("malformed_json_create_rejects_with_reason_code_and_no_persistence_or_leakage"));
    assert!(retention_recovery.contains("restart_recovery_preserves_only_contract_allowed_state"));
    assert!(retention_logging
        .contains("cleanup_recovery_logs_redact_capability_descriptor_ciphertext_plaintext"));
}
