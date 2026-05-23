mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::{Method, StatusCode};
use qsl_attachments::{
    build_router, qsl_attachments_production_size_class_for_len,
    qsl_attachments_production_size_class_table, sha512_merkle_root, AppState, Config,
    CreateSessionRequest, PartSizeClass, RetentionClass, SizeClassPolicy, TestClock, TestDiskSpace,
    PRODUCTION_SIZE_CLASS_POLICY_V1,
};
use tempfile::TempDir;

use support::{
    assert_error, commit_session, fetch_object, jsonless_get, raw_json_request_on, read_bytes,
    read_json, upload_part, Fixture,
};

const ONE_MIB: usize = 1024 * 1024;
const DEFAULT_MAX: u64 = 101 * 1024 * 1024;
const QSHIELD_DEMO_SMALL_CLASSES: [u64; 12] = [
    256, 512, 768, 1024, 1536, 2048, 3072, 4096, 5120, 6144, 7168, 8192,
];

#[derive(Debug)]
struct Committed {
    session_id: String,
    resume_token: String,
    locator_ref: String,
    fetch_capability: String,
    attachment_id: String,
    body: Vec<u8>,
}

fn production_config(max_class_bytes: u64) -> Config {
    Config {
        max_ciphertext_bytes: DEFAULT_MAX,
        max_open_sessions: 16,
        storage_reserve_bytes: 1024,
        size_class_policy: SizeClassPolicy::production_v1(max_class_bytes).expect("policy"),
        ..Fixture::base_config()
    }
}

fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|index| seed.wrapping_add((index % 251) as u8))
        .collect()
}

fn multipart_payload(
    seed: u64,
    body: &[u8],
    part_size_class: PartSizeClass,
) -> (Vec<Vec<u8>>, CreateSessionRequest) {
    let part_size = part_size_class.bytes() as usize;
    let parts: Vec<Vec<u8>> = body.chunks(part_size).map(|chunk| chunk.to_vec()).collect();
    let request = CreateSessionRequest {
        attachment_id: format!("{seed:064x}"),
        ciphertext_len: body.len() as u64,
        part_size_class,
        part_count: parts.len() as u32,
        integrity_alg: "sha512_merkle_v1".to_owned(),
        integrity_root: sha512_merkle_root(&parts),
        retention_class: RetentionClass::Standard,
    };
    (parts, request)
}

async fn commit_payload(fixture: &Fixture, seed: u64, body: Vec<u8>) -> Committed {
    let (parts, request) = multipart_payload(seed, &body, PartSizeClass::P1024k);
    let response = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(&request).expect("request json"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let session_body: serde_json::Value = read_json(response).await;
    let session_id = session_body["session_id"].as_str().expect("session id");
    let resume_token = session_body["resume_token"]
        .as_str()
        .expect("resume token")
        .to_owned();
    let expected_class =
        qsl_attachments_production_size_class_for_len(body.len() as u64, DEFAULT_MAX)
            .expect("size class");
    assert_eq!(
        session_body["size_class"]["policy"],
        PRODUCTION_SIZE_CLASS_POLICY_V1
    );
    assert_eq!(session_body["size_class"]["class_bytes"], expected_class);

    for (index, part) in parts.iter().enumerate() {
        let upload = upload_part(&fixture.app, session_id, &resume_token, index as u32, part).await;
        assert_eq!(upload.status(), StatusCode::OK);
    }

    let commit = commit_session(&fixture.app, session_id, &resume_token, &request).await;
    assert_eq!(commit.status(), StatusCode::OK);
    let commit_body: serde_json::Value = read_json(commit).await;
    assert_eq!(
        commit_body["size_class"]["policy"],
        PRODUCTION_SIZE_CLASS_POLICY_V1
    );
    assert_eq!(commit_body["size_class"]["class_bytes"], expected_class);
    Committed {
        session_id: session_id.to_owned(),
        resume_token,
        locator_ref: commit_body["locator_ref"]
            .as_str()
            .expect("locator")
            .to_owned(),
        fetch_capability: commit_body["fetch_capability"]
            .as_str()
            .expect("fetch capability")
            .to_owned(),
        attachment_id: commit_body["attachment_id"]
            .as_str()
            .expect("attachment")
            .to_owned(),
        body,
    }
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

#[test]
fn na0344_size_class_policy_ok() {
    let classes = qsl_attachments_production_size_class_table(DEFAULT_MAX).expect("table");
    assert_eq!(
        &classes[..QSHIELD_DEMO_SMALL_CLASSES.len()],
        QSHIELD_DEMO_SMALL_CLASSES
    );
    assert!(classes.contains(&(16 * 1024)));
    assert!(classes.contains(&(1024 * 1024)));
    assert!(classes.contains(&(2 * 1024 * 1024)));
    assert_eq!(classes.last().copied(), Some(DEFAULT_MAX));
    assert_eq!(
        qsl_attachments_production_size_class_for_len(8192, DEFAULT_MAX).unwrap(),
        8192
    );
    assert_eq!(
        qsl_attachments_production_size_class_for_len(8193, DEFAULT_MAX).unwrap(),
        16 * 1024
    );
    assert_eq!(
        qsl_attachments_production_size_class_for_len(ONE_MIB as u64 + 1, DEFAULT_MAX).unwrap(),
        2 * 1024 * 1024
    );
    assert_eq!(
        qsl_attachments_production_size_class_for_len(DEFAULT_MAX, DEFAULT_MAX).unwrap(),
        DEFAULT_MAX
    );
}

#[tokio::test]
async fn na0344_valid_small_medium_large_object_ok() {
    let fixture = Fixture::with_config(production_config(DEFAULT_MAX));
    for (seed, len) in [
        (34_400_001, 777usize),
        (34_400_002, 65_537usize),
        (34_400_003, ONE_MIB + 1),
    ] {
        let committed = commit_payload(&fixture, seed, payload(len, seed as u8)).await;
        let fetch = fetch_object(
            &fixture.app,
            &committed.locator_ref,
            &committed.fetch_capability,
        )
        .await;
        assert_eq!(fetch.status(), StatusCode::OK);
        assert_eq!(read_bytes(fetch).await, committed.body);
        let object_json: serde_json::Value = serde_json::from_slice(
            &fs::read(object_meta_path(
                &fixture.storage_root,
                &committed.locator_ref,
            ))
            .expect("object json"),
        )
        .expect("object json parse");
        assert_eq!(
            object_json["size_class"]["policy"],
            PRODUCTION_SIZE_CLASS_POLICY_V1
        );
    }
}

#[tokio::test]
async fn na0344_invalid_config_and_oversize_reject_ok() {
    let tempdir = TempDir::new().expect("tempdir");
    let invalid = Config {
        storage_root: tempdir.path().join("data"),
        size_class_policy: SizeClassPolicy::ProductionV1 {
            max_class_bytes: 12_345,
        },
        ..Fixture::base_config()
    };
    let clock = TestClock::new(1_700_000_000);
    let disk = TestDiskSpace::new(u64::MAX / 4);
    let result = AppState::new_with_disk_space(invalid, Arc::new(clock), Arc::new(disk));
    assert!(result.is_err());
    assert_eq!(
        result.err().expect("invalid config").kind(),
        std::io::ErrorKind::InvalidInput
    );

    let fixture = Fixture::with_config(production_config(ONE_MIB as u64));
    let body = payload(ONE_MIB + 1, 7);
    let (_parts, request) = multipart_payload(34_400_010, &body, PartSizeClass::P1024k);
    let response = fixture
        .json_request(
            Method::POST,
            "/v1/attachments/sessions",
            &[],
            serde_json::to_value(&request).expect("request json"),
        )
        .await;
    assert_error(
        response,
        StatusCode::PAYLOAD_TOO_LARGE,
        "REJECT_QATTSVC_QUOTA",
    )
    .await;
    let session_entries = fs::read_dir(fixture.storage_root.join("sessions"))
        .expect("sessions dir")
        .filter_map(Result::ok)
        .count();
    assert_eq!(session_entries, 0);
}

#[tokio::test]
async fn na0344_malformed_descriptor_and_object_reject_ok() {
    let fixture = Fixture::with_config(production_config(DEFAULT_MAX));
    let malformed = raw_json_request_on(
        &fixture.app,
        Method::POST,
        "/v1/attachments/sessions",
        &[],
        "{ not-json".to_owned(),
    )
    .await;
    assert_error(
        malformed,
        StatusCode::BAD_REQUEST,
        "REJECT_QATTSVC_MALFORMED_JSON",
    )
    .await;
    let session_entries = fs::read_dir(fixture.storage_root.join("sessions"))
        .expect("sessions dir")
        .filter_map(Result::ok)
        .count();
    assert_eq!(session_entries, 0);

    let committed = commit_payload(&fixture, 34_400_020, payload(1024, 8)).await;
    let meta_path = object_meta_path(&fixture.storage_root, &committed.locator_ref);
    let mut object_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&meta_path).expect("object json"))
            .expect("object json parse");
    object_json["size_class"]["class_bytes"] = serde_json::json!(1);
    fs::write(
        &meta_path,
        serde_json::to_vec_pretty(&object_json).expect("object json serialize"),
    )
    .expect("write malformed object");

    let (_app, state) = fixture.restart();
    let recovery = state.recovery_summary();
    assert_eq!(recovery.recovered_committed_objects, 0);
    assert_eq!(recovery.discarded_incoherent_objects, 1);
    assert!(!object_dir(&fixture.storage_root, &committed.locator_ref).exists());
}

#[tokio::test]
async fn na0344_retention_purge_and_backup_boundary_ok() {
    let fixture = Fixture::with_config(production_config(DEFAULT_MAX));
    let committed = commit_payload(&fixture, 34_400_030, payload(4097, 9)).await;

    let backup_tempdir = TempDir::new().expect("backup tempdir");
    let backup_root = backup_tempdir.path().join("data");
    copy_dir_all(&fixture.storage_root, &backup_root);
    let mut restored_config = fixture.config.clone();
    restored_config.storage_root = backup_root.clone();
    let restored_state = AppState::new_with_disk_space(
        restored_config,
        Arc::new(fixture.clock.clone()),
        Arc::new(fixture.disk.clone()),
    )
    .expect("restore state");
    let restored_app = build_router(restored_state.clone());
    assert_eq!(
        restored_state
            .recovery_summary()
            .recovered_committed_objects,
        1
    );
    let restored_fetch = fetch_object(
        &restored_app,
        &committed.locator_ref,
        &committed.fetch_capability,
    )
    .await;
    assert_eq!(restored_fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(restored_fetch).await, committed.body);

    fixture
        .clock
        .advance(fixture.config.standard_retention_ttl_secs + 1);
    let expired = fetch_object(
        &fixture.app,
        &committed.locator_ref,
        &committed.fetch_capability,
    )
    .await;
    assert_error(expired, StatusCode::GONE, "REJECT_QATTSVC_EXPIRED").await;
    assert!(!object_bytes_path(&fixture.storage_root, &committed.locator_ref).exists());
}

#[tokio::test]
async fn na0344_no_secret_artifact_qsl_server_boundary_and_qshield_demo_compatibility_ok() {
    let fixture = Fixture::with_config(production_config(DEFAULT_MAX));
    let committed = commit_payload(&fixture, 34_400_040, payload(1537, 10)).await;
    let object_json = fs::read_to_string(object_meta_path(
        &fixture.storage_root,
        &committed.locator_ref,
    ))
    .expect("object json");
    assert!(!object_json.contains(&committed.fetch_capability));
    assert!(!object_json.contains(&committed.resume_token));
    for event in fixture.state.audit_snapshot() {
        assert_ne!(
            event.session_handle.as_deref(),
            Some(committed.session_id.as_str())
        );
        assert_ne!(
            event.locator_handle.as_deref(),
            Some(committed.locator_ref.as_str())
        );
        assert_ne!(
            event.attachment_handle.as_deref(),
            Some(committed.attachment_id.as_str())
        );
        if let Some(handle) = event
            .locator_handle
            .or(event.attachment_handle)
            .or(event.session_handle)
        {
            assert_eq!(handle.len(), 12);
            assert!(handle.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }

    let qsl_server_path = jsonless_get(&fixture.app, "/v1/qsl-server/routes", &[]).await;
    assert_eq!(qsl_server_path.status(), StatusCode::NOT_FOUND);

    let classes = qsl_attachments_production_size_class_table(DEFAULT_MAX).expect("table");
    assert_eq!(
        &classes[..QSHIELD_DEMO_SMALL_CLASSES.len()],
        QSHIELD_DEMO_SMALL_CLASSES
    );
}
