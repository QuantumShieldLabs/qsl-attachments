use std::sync::Arc;

use qsl_attachments::{build_router, AppState, Config, SystemClock};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = Config::from_env().expect("valid configuration");
    let bind_addr = config.bind_addr;
    let state = AppState::new(config, Arc::new(SystemClock)).expect("initialize state");
    let app = build_router(state);
    let listener = TcpListener::bind(bind_addr).await.expect("bind listener");
    axum::serve(listener, app).await.expect("serve application");
}
