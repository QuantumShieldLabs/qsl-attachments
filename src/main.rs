use std::sync::Arc;

use qsl_attachments::{build_router, AppState, Config, SystemClock};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = Config::from_env().expect("valid configuration");
    let policy_surface = config.operator_policy_surface();
    info!(
        bind_addr = %config.bind_addr,
        storage_root = %config.storage_root.display(),
        max_ciphertext_bytes = config.max_ciphertext_bytes,
        max_open_sessions = config.max_open_sessions,
        storage_reserve_bytes = config.storage_reserve_bytes,
        session_ttl_secs = config.session_ttl_secs,
        service_policy_subject = policy_surface.service_policy_subject,
        authorization_model = policy_surface.authorization_model,
        authorization_header = policy_surface.authorization_header,
        quota_scope = policy_surface.quota_scope,
        resume_token_scope = policy_surface.resume_token_scope,
        fetch_capability_scope = policy_surface.fetch_capability_scope,
        resource_ref_model = policy_surface.resource_ref_model,
        principal_model = policy_surface.principal_model,
        transfer_model = policy_surface.transfer_model,
        "qatt startup configuration"
    );
    let bind_addr = config.bind_addr;
    let state = AppState::new(config, Arc::new(SystemClock)).expect("initialize state");
    let recovery = state.recovery_summary();
    info!(
        durability_boundary = "single_node_local_storage_root",
        graceful_restart_scope = "same_root_only",
        committed_object_recovery = "object_json_plus_ciphertext_bin",
        supported_backup_shape = "cold_full_root_plus_matching_config",
        unsupported_cases =
            "abrupt_crash_open_session,hot_live_backup,partial_restore,cross_file_transaction",
        resumable_sessions = recovery.resumable_sessions,
        discarded_incoherent_sessions = recovery.discarded_incoherent_sessions,
        discarded_orphan_session_dirs = recovery.discarded_orphan_session_dirs,
        discarded_orphan_part_files = recovery.discarded_orphan_part_files,
        recovered_committed_objects = recovery.recovered_committed_objects,
        discarded_incoherent_objects = recovery.discarded_incoherent_objects,
        discarded_orphan_object_dirs = recovery.discarded_orphan_object_dirs,
        "qatt startup durability recovery"
    );
    let app = build_router(state);
    let listener = TcpListener::bind(bind_addr).await.expect("bind listener");
    axum::serve(listener, app).await.expect("serve application");
}
