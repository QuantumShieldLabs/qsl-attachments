mod support;

use axum::http::{Method, StatusCode};
use qsl_attachments::{RetentionClass, SessionStatusResponse};

use support::{
    assert_error, commit_one_part, commit_session, create_session, dir_size, fetch_object,
    object_ciphertext, one_part_payload, read_bytes, read_json, session_part_count, upload_part,
    Fixture,
};

const WRONG_RESUME_TOKEN: &str = "NA0284WRONGRESUMECAPABILITYAAAAAA";
const OTHER_WRONG_RESUME_TOKEN: &str = "NA0284OTHERWRONGRESUMECAPABILITY";
const WRONG_FETCH_CAPABILITY: &str = "NA0284WRONGFETCHCAPABILITYAAAAAAA";
const OTHER_WRONG_FETCH_CAPABILITY: &str = "NA0284OTHERWRONGFETCHCAPABILITYA";

#[tokio::test]
async fn wrong_resume_capability_cannot_mutate_other_session() {
    let fixture = Fixture::new();
    let (target_parts, target_request) =
        one_part_payload(284_001, b"target-ciphertext", RetentionClass::Standard);
    let (other_parts, other_request) =
        one_part_payload(284_002, b"other-ciphertext", RetentionClass::Standard);

    let (target_session, target_resume) = create_session(&fixture, &target_request).await;
    let (other_session, other_resume) = create_session(&fixture, &other_request).await;

    let wrong_upload = upload_part(
        &fixture.app,
        &target_session,
        &other_resume,
        0,
        &target_parts[0],
    )
    .await;
    let body = assert_error(
        wrong_upload,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_RESUME_TOKEN",
    )
    .await;
    assert!(!body.to_string().contains(&other_resume));
    assert_eq!(
        session_part_count(&fixture.storage_root, &target_session),
        0
    );

    let correct_upload = upload_part(
        &fixture.app,
        &target_session,
        &target_resume,
        0,
        &target_parts[0],
    )
    .await;
    assert_eq!(correct_upload.status(), StatusCode::OK);
    assert_eq!(
        session_part_count(&fixture.storage_root, &target_session),
        1
    );

    let wrong_abort = fixture
        .json_request(
            Method::DELETE,
            &format!("/v1/attachments/sessions/{target_session}"),
            &[("X-QATT-Resume-Token", &other_resume)],
            serde_json::Value::Null,
        )
        .await;
    let body = assert_error(
        wrong_abort,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_RESUME_TOKEN",
    )
    .await;
    assert!(!body.to_string().contains(&target_resume));

    let status = fixture
        .json_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{target_session}"),
            &[("X-QATT-Resume-Token", &target_resume)],
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status: SessionStatusResponse = read_json(status).await;
    assert_eq!(status.stored_part_count, 1);

    let target_commit = commit_session(
        &fixture.app,
        &target_session,
        &target_resume,
        &target_request,
    )
    .await;
    assert_eq!(target_commit.status(), StatusCode::OK);

    let other_status = fixture
        .json_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{other_session}"),
            &[("X-QATT-Resume-Token", &other_resume)],
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(other_status.status(), StatusCode::OK);
    assert_eq!(session_part_count(&fixture.storage_root, &other_session), 0);

    let other_upload = upload_part(
        &fixture.app,
        &other_session,
        &other_resume,
        0,
        &other_parts[0],
    )
    .await;
    assert_eq!(other_upload.status(), StatusCode::OK);
}

#[tokio::test]
async fn wrong_fetch_capability_cannot_fetch_other_object() {
    let fixture = Fixture::new();
    let first = commit_one_part(
        &fixture,
        284_010,
        b"first-object-ciphertext",
        RetentionClass::Standard,
    )
    .await;
    let second = commit_one_part(
        &fixture,
        284_011,
        b"second-object-ciphertext",
        RetentionClass::Standard,
    )
    .await;

    let first_before = object_ciphertext(&fixture.storage_root, &first.locator_ref);
    let second_before = object_ciphertext(&fixture.storage_root, &second.locator_ref);

    let wrong_fetch =
        fetch_object(&fixture.app, &first.locator_ref, &second.fetch_capability).await;
    let body = assert_error(
        wrong_fetch,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_FETCH_CAPABILITY",
    )
    .await;
    let error_text = body.to_string();
    assert!(!error_text.contains(&second.fetch_capability));
    assert!(!error_text.contains("first-object-ciphertext"));
    assert!(!error_text.contains("second-object-ciphertext"));
    assert_eq!(
        object_ciphertext(&fixture.storage_root, &first.locator_ref),
        first_before
    );
    assert_eq!(
        object_ciphertext(&fixture.storage_root, &second.locator_ref),
        second_before
    );

    let correct_first =
        fetch_object(&fixture.app, &first.locator_ref, &first.fetch_capability).await;
    assert_eq!(correct_first.status(), StatusCode::OK);
    assert_eq!(read_bytes(correct_first).await, first.body);

    let correct_second =
        fetch_object(&fixture.app, &second.locator_ref, &second.fetch_capability).await;
    assert_eq!(correct_second.status(), StatusCode::OK);
    assert_eq!(read_bytes(correct_second).await, second.body);
}

#[tokio::test]
async fn missing_malformed_capabilities_fail_closed_with_reason_code() {
    let fixture = Fixture::new();
    let (parts, request) =
        one_part_payload(284_020, b"malformed-boundary", RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;

    let missing_resume = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[],
            parts[0].clone(),
        )
        .await;
    let missing_resume_body = assert_error(
        missing_resume,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_RESUME_TOKEN",
    )
    .await;
    assert!(!missing_resume_body.to_string().contains(&resume_token));
    assert_eq!(session_part_count(&fixture.storage_root, &session_id), 0);

    let malformed_resume = fixture
        .bytes_request(
            Method::PUT,
            &format!("/v1/attachments/sessions/{session_id}/parts/0"),
            &[("X-QATT-Resume-Token", "not a base64url token")],
            parts[0].clone(),
        )
        .await;
    assert_error(
        malformed_resume,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_RESUME_TOKEN",
    )
    .await;
    assert_eq!(session_part_count(&fixture.storage_root, &session_id), 0);

    let upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(upload.status(), StatusCode::OK);
    let commit = commit_session(&fixture.app, &session_id, &resume_token, &request).await;
    assert_eq!(commit.status(), StatusCode::OK);
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap().to_owned();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap().to_owned();

    let missing_fetch = support::jsonless_get(
        &fixture.app,
        &format!("/v1/attachments/objects/{locator_ref}"),
        &[],
    )
    .await;
    let missing_fetch_body = assert_error(
        missing_fetch,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_FETCH_CAPABILITY",
    )
    .await;
    assert!(!missing_fetch_body.to_string().contains(&fetch_capability));

    let malformed_fetch = support::jsonless_get(
        &fixture.app,
        &format!("/v1/attachments/objects/{locator_ref}"),
        &[("X-QATT-Fetch-Capability", "not a base64url token")],
    )
    .await;
    assert_error(
        malformed_fetch,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_FETCH_CAPABILITY",
    )
    .await;

    let correct_fetch = fetch_object(&fixture.app, &locator_ref, &fetch_capability).await;
    assert_eq!(correct_fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(correct_fetch).await, b"malformed-boundary");
}

#[tokio::test]
async fn deleted_or_aborted_resource_capability_behavior_is_deterministic() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(284_030, b"abort-me", RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(upload.status(), StatusCode::OK);

    let abort = fixture
        .json_request(
            Method::DELETE,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(abort.status(), StatusCode::OK);
    assert_eq!(session_part_count(&fixture.storage_root, &session_id), 0);

    for response in [
        fixture
            .json_request(
                Method::DELETE,
                &format!("/v1/attachments/sessions/{session_id}"),
                &[("X-QATT-Resume-Token", &resume_token)],
                serde_json::Value::Null,
            )
            .await,
        upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await,
    ] {
        assert_error(
            response,
            StatusCode::FORBIDDEN,
            "REJECT_QATTSVC_RESUME_TOKEN",
        )
        .await;
    }

    let object = commit_one_part(
        &fixture,
        284_031,
        b"expires-after-fetch",
        RetentionClass::Short,
    )
    .await;
    let before_expiry =
        fetch_object(&fixture.app, &object.locator_ref, &object.fetch_capability).await;
    assert_eq!(before_expiry.status(), StatusCode::OK);
    assert_eq!(read_bytes(before_expiry).await, object.body);

    fixture
        .clock
        .advance(fixture.config.short_retention_ttl_secs + 1);
    for _ in 0..2 {
        let expired =
            fetch_object(&fixture.app, &object.locator_ref, &object.fetch_capability).await;
        assert_error(expired, StatusCode::GONE, "REJECT_QATTSVC_EXPIRED").await;
    }
}

#[tokio::test]
async fn duplicate_capability_use_matches_documented_semantics() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(284_040, b"duplicate-scope", RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;

    let first_upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(first_upload.status(), StatusCode::OK);
    let duplicate_upload =
        upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(duplicate_upload.status(), StatusCode::OK);
    assert_eq!(session_part_count(&fixture.storage_root, &session_id), 1);

    let status = fixture
        .json_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(status.status(), StatusCode::OK);

    let commit = commit_session(&fixture.app, &session_id, &resume_token, &request).await;
    assert_eq!(commit.status(), StatusCode::OK);
    let commit_body: serde_json::Value = read_json(commit).await;
    let locator_ref = commit_body["locator_ref"].as_str().unwrap().to_owned();
    let fetch_capability = commit_body["fetch_capability"].as_str().unwrap().to_owned();

    let duplicate_commit = commit_session(&fixture.app, &session_id, &resume_token, &request).await;
    assert_error(
        duplicate_commit,
        StatusCode::CONFLICT,
        "REJECT_QATTSVC_SESSION_STATE",
    )
    .await;

    for _ in 0..2 {
        let fetch = fetch_object(&fixture.app, &locator_ref, &fetch_capability).await;
        assert_eq!(fetch.status(), StatusCode::OK);
        assert_eq!(read_bytes(fetch).await, b"duplicate-scope");
    }
}

#[tokio::test]
async fn bounded_capability_abuse_has_no_panic_and_no_unbounded_growth() {
    let fixture = Fixture::new();
    let (parts, request) = one_part_payload(284_050, b"abuse-bounded", RetentionClass::Standard);
    let (session_id, resume_token) = create_session(&fixture, &request).await;
    let size_before_resume_abuse = dir_size(&fixture.storage_root);

    for attempt in 1..=6 {
        let response = fixture
            .json_request(
                Method::GET,
                &format!("/v1/attachments/sessions/{session_id}"),
                &[("X-QATT-Resume-Token", WRONG_RESUME_TOKEN)],
                serde_json::Value::Null,
            )
            .await;
        let expected = if attempt <= fixture.config.invalid_secret_attempt_limit {
            (StatusCode::FORBIDDEN, "REJECT_QATTSVC_RESUME_TOKEN")
        } else {
            (StatusCode::TOO_MANY_REQUESTS, "REJECT_QATTSVC_ABUSE")
        };
        assert_error(response, expected.0, expected.1).await;
    }

    assert_eq!(dir_size(&fixture.storage_root), size_before_resume_abuse);
    let status = fixture
        .json_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", &resume_token)],
            serde_json::Value::Null,
        )
        .await;
    assert_eq!(status.status(), StatusCode::OK);
    let upload = upload_part(&fixture.app, &session_id, &resume_token, 0, &parts[0]).await;
    assert_eq!(upload.status(), StatusCode::OK);

    let committed = commit_one_part(
        &fixture,
        284_051,
        b"fetch-abuse-bounded",
        RetentionClass::Standard,
    )
    .await;
    let size_before_fetch_abuse = dir_size(&fixture.storage_root);
    for attempt in 1..=6 {
        let response = fetch_object(
            &fixture.app,
            &committed.locator_ref,
            if attempt % 2 == 0 {
                WRONG_FETCH_CAPABILITY
            } else {
                OTHER_WRONG_FETCH_CAPABILITY
            },
        )
        .await;
        let expected = if attempt <= fixture.config.invalid_secret_attempt_limit {
            (StatusCode::FORBIDDEN, "REJECT_QATTSVC_FETCH_CAPABILITY")
        } else {
            (StatusCode::TOO_MANY_REQUESTS, "REJECT_QATTSVC_ABUSE")
        };
        assert_error(response, expected.0, expected.1).await;
    }

    assert_eq!(dir_size(&fixture.storage_root), size_before_fetch_abuse);
    let fetch = fetch_object(
        &fixture.app,
        &committed.locator_ref,
        &committed.fetch_capability,
    )
    .await;
    assert_eq!(fetch.status(), StatusCode::OK);
    assert_eq!(read_bytes(fetch).await, committed.body);

    let wrong_other_resume = fixture
        .json_request(
            Method::GET,
            &format!("/v1/attachments/sessions/{session_id}"),
            &[("X-QATT-Resume-Token", OTHER_WRONG_RESUME_TOKEN)],
            serde_json::Value::Null,
        )
        .await;
    assert_error(
        wrong_other_resume,
        StatusCode::FORBIDDEN,
        "REJECT_QATTSVC_RESUME_TOKEN",
    )
    .await;
}
