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
    info!(
        bind_addr = %config.bind_addr,
        storage_root = %config.storage_root.display(),
        max_ciphertext_bytes = config.max_ciphertext_bytes,
        max_open_sessions = config.max_open_sessions,
        storage_reserve_bytes = config.storage_reserve_bytes,
        session_ttl_secs = config.session_ttl_secs,
        "qatt startup configuration"
    );
    let bind_addr = config.bind_addr;
    let state = AppState::new(config, Arc::new(SystemClock)).expect("initialize state");
    let app = build_router(state);
    let listener = TcpListener::bind(bind_addr).await.expect("bind listener");
    axum::serve(listener, app).await.expect("serve application");
}
