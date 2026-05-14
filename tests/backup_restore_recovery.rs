mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::{Method, StatusCode};
use qsl_attachments::{
    build_router, sha512_merkle_root, AppState, Config, CreateSessionRequest, MissingRange,
    PartSizeClass, RetentionClass, SessionState, SessionStatusResponse, TestClock, TestDiskSpace,
};
use tempfile::TempDir;

use support::{
    assert_error, bytes_request_on, commit_one_part, commit_session, create_session, fetch_object,
    jsonless_get, one_part_payload, read_bytes, read_json, upload_part, Fixture,
};

struct RestoredFixture {
    _tempdir: TempDir,
    app: axum::Router,
    state: AppState,
    storage_root: PathBuf,
}

fn full_root_restore(fixture: &Fixture) -> RestoredFixture {
    restored_from_builder(&fixture.config, &fixture.clock, &fixture.disk, |root| {
        copy_dir_all(&fixture.storage_root, root);
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
    config.storage_root = storage_root.clone();
    let state =
        AppState::new_with_disk_space(config, Arc::new(clock.clone()), Arc::new(disk.clone()))
            .expect("restored state");
    let app = build_router(state.clone());
    RestoredFixture {
        _tempdir: tempdir,
        app,
        state,
        storage_root,
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

fn two_part_payload(seed: u64) -> (Vec<Vec<u8>>, CreateSessionRequest) {
    let first = vec![b'R'; 65_536];
    let second = format!("restore-tail-{seed}").into_bytes();
    let parts = vec![first, second];
    let request = CreateSessionRequest {
        attachment_id: format!("{seed:064x}"),
        ciphertext_len: (parts[0].len() + parts[1].len()) as u64,
        part_size_class: PartSizeClass::P64k,
        part_count: 2,
        integrity_alg: "sha512_merkle_v1".to_owned(),
        integrity_root: sha512_merkle_root(&parts),
        retention_class: RetentionClass::Standard,
    };
    (parts, request)
}

fn session_dir(root: &Path, session_id: &str) -> PathBuf {
    root.join("sessions").join(session_id)
}

fn session_meta_path(root: &Path, session_id: &str) -> PathBuf {
    session_dir(root, session_id).join("session.json")
}

fn session_part_path(root: &Path, session_id: &str, part_index: u32) -> PathBuf {
    session_dir(root, session_id)
        .join("parts")
        .join(format!("{part_index}.part"))
}

fn object_dir(root: &Path, locator_ref: &str) -> PathBuf {
    root.join("objects").join(locator_ref)
}

fn object_meta_path(root: &Path, locator_ref: &str) -> PathBuf {
    object_dir(root, locator_ref).join("object.json")
}

fn object_bytes_path(root: &Path, locator_ref: &str) -> PathBuf {
    object_dir(root, locator_ref).join("ciphertext.bin")
}

#[tokio::test]
async fn cold_full_root_restore_recovers_only_coherent_committed_state() {
    let fixture = Fixture::new();
    let committed_body = b"NA0286_FULL_ROOT_OPAQUE_CIPHERTEXT";
    let committed =
        commit_one_part(&fixture, 286_001, committed_body, RetentionClass::Standard).await;

    let (open_parts, open_request) = two_part_payload(286_002);
    let (open_session_id, open_resume_token) = create_session(&fixture, &open_request).await;
    let upload = upload_part(
        &fixture.app,
        &open_session_id,
        &open_resume_token,
        0,
        &open_parts[0],
    )
    .await;
    assert_eq!(upload.status(), StatusCode::OK);

    let restored = full_root_restore(&fixture);
    let recovery = restored.state.recovery_summary();
    assert_eq!(recovery.recovered_committed_objects, 1);
    assert_eq!(recovery.resumable_sessions, 1);
    assert_eq!(recovery.discarded_incoherent_objects, 0);
    assert_eq!(recovery.discarded_incoherent_sessions, 0);

    let fetch = fetch_object(
        &restored.app,
        &committed.locator_ref,
        &committed.fetch_capability,
    )
    .await;
    assert_eq!(fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(fetch).await, committed_body.to_vec());

    let status = jsonless_get(
        &restored.app,
        &format!("/v1/attachments/sessions/{open_session_id}"),
        &[("X-QATT-Resume-Token", &open_resume_token)],
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body: SessionStatusResponse = read_json(status).await;
    assert_eq!(status_body.session_state, SessionState::Uploading);
    assert_eq!(status_body.stored_part_count, 1);
    assert_eq!(
        status_body.missing_part_ranges,
        vec![MissingRange { start: 1, end: 1 }]
    );

    let final_upload = upload_part(
        &restored.app,
        &open_session_id,
        &open_resume_token,
        1,
        &open_parts[1],
    )
    .await;
    assert_eq!(final_upload.status(), StatusCode::OK);
    let commit = commit_session(
        &restored.app,
        &open_session_id,
        &open_resume_token,
        &open_request,
    )
    .await;
    assert_eq!(commit.status(), StatusCode::OK);
}

#[tokio::test]
async fn partial_restore_object_json_without_ciphertext_fails_closed() {
    let fixture = Fixture::new();
    let committed = commit_one_part(
        &fixture,
        286_010,
        b"metadata-without-bytes",
        RetentionClass::Standard,
    )
    .await;

    let restored = restored_from_builder(&fixture.config, &fixture.clock, &fixture.disk, |root| {
        let destination = object_dir(root, &committed.locator_ref);
        fs::create_dir_all(&destination).expect("partial object dir");
        fs::copy(
            object_meta_path(&fixture.storage_root, &committed.locator_ref),
            destination.join("object.json"),
        )
        .expect("copy object metadata");
    });

    let recovery = restored.state.recovery_summary();
    assert_eq!(recovery.recovered_committed_objects, 0);
    assert_eq!(recovery.discarded_incoherent_objects, 1);
    assert!(!object_dir(&restored.storage_root, &committed.locator_ref).exists());

    let fetch = fetch_object(
        &restored.app,
        &committed.locator_ref,
        &committed.fetch_capability,
    )
    .await;
    assert_error(
        fetch,
        StatusCode::NOT_FOUND,
        "REJECT_QATTSVC_LOCATOR_UNKNOWN",
    )
    .await;
}

#[tokio::test]
async fn partial_restore_ciphertext_without_object_json_fails_closed() {
    let fixture = Fixture::new();
    let committed = commit_one_part(
        &fixture,
        286_011,
        b"bytes-without-metadata",
        RetentionClass::Standard,
    )
    .await;

    let restored = restored_from_builder(&fixture.config, &fixture.clock, &fixture.disk, |root| {
        let destination = object_dir(root, &committed.locator_ref);
        fs::create_dir_all(&destination).expect("partial object dir");
        fs::copy(
            object_bytes_path(&fixture.storage_root, &committed.locator_ref),
            destination.join("ciphertext.bin"),
        )
        .expect("copy object ciphertext");
    });

    let recovery = restored.state.recovery_summary();
    assert_eq!(recovery.recovered_committed_objects, 0);
    assert_eq!(recovery.discarded_orphan_object_dirs, 1);
    assert!(!object_dir(&restored.storage_root, &committed.locator_ref).exists());

    let fetch = fetch_object(
        &restored.app,
        &committed.locator_ref,
        &committed.fetch_capability,
    )
    .await;
    assert_error(
        fetch,
        StatusCode::NOT_FOUND,
        "REJECT_QATTSVC_LOCATOR_UNKNOWN",
    )
    .await;
}

#[tokio::test]
async fn partial_restore_orphan_parts_and_missing_parts_fail_closed() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(
        286_020,
        b"journaled-part-missing-after-restore",
        RetentionClass::Standard,
    );
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(upload.status(), StatusCode::OK);

    let orphan_session_id = "orphanPartOnlySessionNA0286";
    let restored = restored_from_builder(&fixture.config, &fixture.clock, &fixture.disk, |root| {
        let destination = session_dir(root, &session_id);
        fs::create_dir_all(&destination).expect("partial session dir");
        fs::copy(
            session_meta_path(&fixture.storage_root, &session_id),
            destination.join("session.json"),
        )
        .expect("copy session metadata");

        let orphan_part = session_part_path(root, orphan_session_id, 0);
        fs::create_dir_all(orphan_part.parent().expect("orphan parent")).unwrap();
        fs::write(orphan_part, b"orphan-staged-ciphertext").unwrap();
    });

    let recovery = restored.state.recovery_summary();
    assert_eq!(recovery.resumable_sessions, 0);
    assert_eq!(recovery.discarded_incoherent_sessions, 1);
    assert_eq!(recovery.discarded_orphan_session_dirs, 1);
    assert!(!session_dir(&restored.storage_root, &session_id).exists());
    assert!(!session_dir(&restored.storage_root, orphan_session_id).exists());

    let status = jsonless_get(
        &restored.app,
        &format!("/v1/attachments/sessions/{session_id}"),
        &[("X-QATT-Resume-Token", &resume_token)],
    )
    .await;
    assert_error(status, StatusCode::CONFLICT, "REJECT_QATTSVC_SESSION_STATE").await;
}

#[tokio::test]
async fn rejected_expired_deleted_and_aborted_state_do_not_resurrect() {
    let fixture = Fixture::new();

    let malformed = fixture
        .raw_json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            "{\"attachment_id\":\"0000\",\"plaintext\":\"NA0286_REJECTED_PLAINTEXT\"".to_owned(),
        )
        .await;
    let malformed_body = assert_error(
        malformed,
        StatusCode::BAD_REQUEST,
        "REJECT_QATTSVC_MALFORMED_JSON",
    )
    .await;
    assert!(!malformed_body
        .to_string()
        .contains("NA0286_REJECTED_PLAINTEXT"));

    let (_expired_session_parts, expired_session_request) =
        one_part_payload(286_030, b"expired-session", RetentionClass::Short);
    let (expired_session_id, expired_resume_token) =
        create_session(&fixture, &expired_session_request).await;

    let expired_object =
        commit_one_part(&fixture, 286_031, b"expired-object", RetentionClass::Short).await;

    fixture.clock.advance(10);
    let expired_session_status = jsonless_get(
        &fixture.app,
        &format!("/v1/attachments/sessions/{expired_session_id}"),
        &[("X-QATT-Resume-Token", &expired_resume_token)],
    )
    .await;
    assert_error(
        expired_session_status,
        StatusCode::GONE,
        "REJECT_QATTSVC_EXPIRED",
    )
    .await;
    let expired_object_fetch = fetch_object(
        &fixture.app,
        &expired_object.locator_ref,
        &expired_object.fetch_capability,
    )
    .await;
    assert_error(
        expired_object_fetch,
        StatusCode::GONE,
        "REJECT_QATTSVC_EXPIRED",
    )
    .await;

    let (_aborted_parts, aborted_request) =
        one_part_payload(286_032, b"aborted-session", RetentionClass::Standard);
    let (aborted_session_id, aborted_resume_token) =
        create_session(&fixture, &aborted_request).await;
    let abort = bytes_request_on(
        &fixture.app,
        Method::DELETE,
        &format!("/v1/attachments/sessions/{aborted_session_id}"),
        &[("X-QATT-Resume-Token", &aborted_resume_token)],
        Vec::new(),
    )
    .await;
    assert_eq!(abort.status(), StatusCode::OK);

    let committed_after_session_removal = commit_one_part(
        &fixture,
        286_033,
        b"committed-session-removes-old-session",
        RetentionClass::Standard,
    )
    .await;

    let restored = full_root_restore(&fixture);

    let restored_expired_session = jsonless_get(
        &restored.app,
        &format!("/v1/attachments/sessions/{expired_session_id}"),
        &[("X-QATT-Resume-Token", &expired_resume_token)],
    )
    .await;
    assert_error(
        restored_expired_session,
        StatusCode::GONE,
        "REJECT_QATTSVC_EXPIRED",
    )
    .await;

    let restored_expired_object = fetch_object(
        &restored.app,
        &expired_object.locator_ref,
        &expired_object.fetch_capability,
    )
    .await;
    assert_error(
        restored_expired_object,
        StatusCode::GONE,
        "REJECT_QATTSVC_EXPIRED",
    )
    .await;

    let restored_aborted = jsonless_get(
        &restored.app,
        &format!("/v1/attachments/sessions/{aborted_session_id}"),
        &[("X-QATT-Resume-Token", &aborted_resume_token)],
    )
    .await;
    assert_error(
        restored_aborted,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_RESUME_TOKEN",
    )
    .await;

    let committed_session_status = jsonless_get(
        &restored.app,
        &format!(
            "/v1/attachments/sessions/{}",
            committed_after_session_removal.session_id
        ),
        &[(
            "X-QATT-Resume-Token",
            &committed_after_session_removal.resume_token,
        )],
    )
    .await;
    assert_error(
        committed_session_status,
        StatusCode::CONFLICT,
        "REJECT_QATTSVC_SESSION_STATE",
    )
    .await;
}

#[tokio::test]
async fn mismatched_descriptor_or_object_metadata_fails_closed() {
    let fixture = Fixture::new();
    let locator_mismatch = commit_one_part(
        &fixture,
        286_040,
        b"locator-mismatch-object",
        RetentionClass::Standard,
    )
    .await;
    let length_mismatch = commit_one_part(
        &fixture,
        286_041,
        b"length-mismatch-object",
        RetentionClass::Standard,
    )
    .await;

    let mut locator_meta: serde_json::Value = serde_json::from_slice(
        &fs::read(object_meta_path(
            &fixture.storage_root,
            &locator_mismatch.locator_ref,
        ))
        .unwrap(),
    )
    .unwrap();
    locator_meta["locator_ref"] = serde_json::Value::String("MismatchedLocatorNA0286".to_owned());
    fs::write(
        object_meta_path(&fixture.storage_root, &locator_mismatch.locator_ref),
        serde_json::to_vec_pretty(&locator_meta).unwrap(),
    )
    .unwrap();

    let mut length_meta: serde_json::Value = serde_json::from_slice(
        &fs::read(object_meta_path(
            &fixture.storage_root,
            &length_mismatch.locator_ref,
        ))
        .unwrap(),
    )
    .unwrap();
    length_meta["ciphertext_len"] = serde_json::Value::from(length_mismatch.body.len() as u64 + 1);
    fs::write(
        object_meta_path(&fixture.storage_root, &length_mismatch.locator_ref),
        serde_json::to_vec_pretty(&length_meta).unwrap(),
    )
    .unwrap();

    let restored = full_root_restore(&fixture);
    let recovery = restored.state.recovery_summary();
    assert_eq!(recovery.recovered_committed_objects, 0);
    assert_eq!(recovery.discarded_incoherent_objects, 2);

    for object in [locator_mismatch, length_mismatch] {
        let fetch =
            fetch_object(&restored.app, &object.locator_ref, &object.fetch_capability).await;
        assert_error(
            fetch,
            StatusCode::NOT_FOUND,
            "REJECT_QATTSVC_LOCATOR_UNKNOWN",
        )
        .await;
    }
}
