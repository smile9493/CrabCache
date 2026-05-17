mod network;
mod routes;
mod state;
mod types;

use state::AppState;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct ServerConfig {
    listen_addr: String,
    cert_path: Option<PathBuf>,
    key_path: Option<PathBuf>,
}

impl ServerConfig {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        
        let mut listen_addr = "0.0.0.0:3000".to_string();
        let mut cert_path = None;
        let mut key_path = None;
        
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--listen" | "-l" => {
                    if i + 1 < args.len() {
                        listen_addr = args[i + 1].clone();
                        i += 1;
                    }
                }
                "--cert" | "-c" => {
                    if i + 1 < args.len() {
                        cert_path = Some(PathBuf::from(&args[i + 1]));
                        i += 1;
                    }
                }
                "--key" | "-k" => {
                    if i + 1 < args.len() {
                        key_path = Some(PathBuf::from(&args[i + 1]));
                        i += 1;
                    }
                }
                "--https" => {
                    cert_path = Some(PathBuf::from("certs/cert.pem"));
                    key_path = Some(PathBuf::from("certs/key.pem"));
                }
                _ => {}
            }
            i += 1;
        }
        
        ServerConfig {
            listen_addr,
            cert_path,
            key_path,
        }
    }
    
    fn is_https(&self) -> bool {
        self.cert_path.is_some() && self.key_path.is_some()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let _ = rustls::crypto::ring::default_provider().install_default();

    let admin_key = std::env::var("CRABCACHE_ADMIN_KEY").unwrap_or_else(|_| "admin".to_string());
    if admin_key == "admin" {
        tracing::warn!(
            "CRABCACHE_ADMIN_KEY is unset or uses the default 'admin'; set a strong key for production. \
             Dashboard must use the same value in the Admin API Key sign-in screen."
        );
    }

    let config = ServerConfig::from_args();
    let state = Arc::new(AppState::new());

    match state.gateway.list_keys().await {
        Ok(specs) => {
            for spec in specs {
                state.keys_meta.insert(
                    spec.id.clone(),
                    crate::state::KeyMetadata {
                        id: spec.id,
                        token: spec.key_full.unwrap_or_default(),
                        rpm_limit: 0,
                        monthly_token_limit: 0,
                        current_rpm: 0,
                        tokens_this_month: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        expired_at: None,
                        model_limits: Vec::new(),
                        remain_quota: -1,
                        unlimited_quota: true,
                    },
                );
            }
            info!(count = state.keys_meta.len(), "Synced API keys from gateway");
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Gateway management API unreachable; key operations may fail until gateway is up"
            );
        }
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = routes::router(state)
        .layer(cors)
        .fallback_service(ServeDir::new("crates/crab-dashboard/dist"));

    let protocol = if config.is_https() { "https" } else { "http" };
    
    info!(
        addr = config.listen_addr,
        protocol = protocol,
        "CrabCache Admin Dashboard starting"
    );

    if config.is_https() {
        let cert_path = config.cert_path.unwrap();
        let key_path = config.key_path.unwrap();
        
        info!(
            cert = %cert_path.display(),
            key = %key_path.display(),
            "Using HTTPS with self-signed certificate"
        );
        
        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            cert_path,
            key_path,
        )
        .await?;
        
        let addr: std::net::SocketAddr = config.listen_addr.parse()?;
        
        axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
        axum::serve(listener, app).await?;
    }

    Ok(())
}