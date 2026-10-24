use std::sync::Arc;

use axum::Router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::local::LocalSessionManager,
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowHeaders, AllowMethods, CorsLayer};

use crate::config::AppConfig;
use crate::server::{QuickDeskMcpServer, SharedState};
use crate::ws_client::WsClient;

pub async fn start_http(
    config: &AppConfig,
    ws: WsClient,
    shared_state: Arc<SharedState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ct = CancellationToken::new();
    let allowed_devices = config.allowed_devices.clone().unwrap_or_default();
    let session_manager = Arc::new(LocalSessionManager::default());
    let mut http_config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(!config.stateless)
        .with_cancellation_token(ct.child_token());
    if let Some(hosts) = &config.allowed_hosts {
        http_config.allowed_hosts.extend(hosts.iter().cloned());
    }

    let mcp_service: StreamableHttpService<QuickDeskMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                Ok(QuickDeskMcpServer::new(
                    ws.clone(),
                    allowed_devices.clone(),
                    shared_state.clone(),
                ))
            },
            session_manager,
            http_config,
        );

    let app = Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { "ok" }),
        )
        .nest_service("/mcp", mcp_service);

    // Apply CORS layer when cors_origin is configured
    let app = if let Some(origins) = &config.cors_origin {
        let allowed_origins: Vec<axum::http::HeaderValue> = origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();

        let cors = CorsLayer::new()
            .allow_origin(allowed_origins)
            .allow_methods(AllowMethods::any())
            .allow_headers(AllowHeaders::any());

        tracing::info!("CORS enabled for origins: {:?}", origins);
        app.layer(cors)
    } else {
        app.layer(CorsLayer::permissive())
    };

    let bind_addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    let local_addr = listener.local_addr()?;

    tracing::info!("MCP HTTP server listening on http://{}", local_addr);
    tracing::info!(
        "  MCP endpoint: http://{}/mcp",
        local_addr
    );
    tracing::info!(
        "  Health check: http://{}/health",
        local_addr
    );
    tracing::info!(
        "  Legacy session mode: {} (MCP 2026-07-28 is always stateless)",
        if config.stateless { "stateless" } else { "stateful" }
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let ctrl_c = tokio::signal::ctrl_c();
            let stdin_eof = async {
                // Detect parent process closing our stdin pipe.
                // On Windows, QProcess::terminate() sends WM_CLOSE which
                // console apps can't receive. Instead, the parent closes
                // our stdin before calling terminate, and we detect the EOF.
                let mut stdin = tokio::io::stdin();
                let mut buf = [0u8; 1];
                loop {
                    match stdin.read(&mut buf).await {
                        Ok(0) | Err(_) => break, // EOF or error
                        Ok(_) => continue,        // ignore stray bytes
                    }
                }
            };
            tokio::select! {
                _ = ctrl_c => {},
                _ = stdin_eof => {},
            }
            tracing::info!("Shutting down HTTP server...");
            ct.cancel();
        })
        .await?;

    Ok(())
}
