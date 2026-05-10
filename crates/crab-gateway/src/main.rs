mod config;

use crate::config::GatewayConfig;
use anyhow::Result;
use async_trait::async_trait;
use crab_cache::{RequestCoalescer, TieredCache, TtlConfig};
use crab_metrics::global_metrics;
use crab_proxy::{GatewayProxy, GatewayState};
use crab_reasoning::ReasoningStore;
use crab_route::AffinityRouter;
use crab_semantic::{Embedder, SemanticCache, VectorStore};
use pingora_core::server::Server;
use pingora_core::services::background::background_service;
use pingora_proxy::http_proxy_service;
use prometheus::Registry;
use std::sync::Arc;
use tracing::info;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

struct MetricsServer {
    addr: String,
    registry: Registry,
}

#[async_trait]
impl pingora_core::services::background::BackgroundService for MetricsServer {
    async fn start(&self, _shutdown: pingora_core::server::ShutdownWatch) {
        let listener = match std::net::TcpListener::bind(&self.addr) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(addr = %self.addr, error = %e, "Failed to bind metrics addr");
                return;
            }
        };

        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };

            let encoder = prometheus::TextEncoder::new();
            let metric_families = self.registry.gather();
            let output = encoder.encode_to_string(&metric_families).unwrap_or_default();

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
                output.len(),
                output
            );

            use std::io::Write;
            let mut stream = stream;
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    }
}

fn main() -> Result<()> {
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info.location().map(|l| l.to_string()).unwrap_or_else(|| "unknown".to_string());
        
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        tracing::error!(
            location = %location,
            message = %message,
            "Panic occurred"
        );
    }));

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/gateway.toml".to_string());

    let config = GatewayConfig::load(&config_path)?;
    info!(config_path = %config_path, "Configuration loaded");

    let backends = config.parse_endpoints();
    info!(backend_count = backends.len(), "Backends parsed");

    let mut server = Server::new(None)?;
    server.bootstrap();

    let registry = Registry::new();
    global_metrics().register(&registry)?;

    let metrics_service = MetricsServer {
        addr: config.metrics_addr.clone(),
        registry,
    };
    server.add_service(background_service("metrics", metrics_service));

    let rt = tokio::runtime::Runtime::new()?;

    let router = Arc::new(rt.block_on(async { AffinityRouter::new(&backends) })?);

    let l1_pool = rt.block_on(async {
        bb8::Pool::builder()
            .max_size(config.cache.l1_pool_size.unwrap_or(16))
            .build(bb8_redis::RedisConnectionManager::new(
                config.cache.l1_redis_url.clone(),
            )?)
            .await
    })?;

    let ttl_config = TtlConfig {
        default_ttl_secs: config.cache.default_ttl_secs.unwrap_or(3600),
        model_overrides: config.cache.model_ttl_overrides.clone().unwrap_or_default(),
        consumer_overrides: config.cache.consumer_ttl_overrides.clone().unwrap_or_default(),
    };

    let l0_config = crab_cache::L0Config {
        max_capacity: config.cache.l0_max_capacity.unwrap_or(10_000),
        ttl_secs: config.cache.l0_ttl_secs.unwrap_or(3600),
    };

    let tiered_cache = Arc::new(
        rt.block_on(async { TieredCache::new(l1_pool, l0_config, ttl_config).await })?,
    );

    let semantic_cache = if config.semantic.enabled {
        let embedder = Arc::new(Embedder::load(
            &config.semantic.model_path,
            &config.semantic.tokenizer_path,
        )?);

        let store = VectorStore::new(
            &config.semantic.qdrant_url,
            &config.semantic.collection_name,
            config.semantic.vector_size.unwrap_or(384),
        );

        let store = rt.block_on(store)?;

        let cache = SemanticCache::new(
            embedder,
            store,
            config.semantic.similarity_threshold.unwrap_or(0.95),
            config.semantic.ttl_secs.unwrap_or(7200),
        );

        Some(Arc::new(rt.block_on(cache)?))
    } else {
        None
    };

    let conn_config = config.connection.clone().unwrap_or_default();

    let reasoning_config = config.reasoning.clone().unwrap_or_default();
    let reasoning_store = Arc::new(
        ReasoningStore::new(
            &reasoning_config.cache_db_path,
            reasoning_config.cache_max_age_secs,
            reasoning_config.cache_max_rows,
        )?
    );

    let upstream_base_url = config.upstream_base_url().to_string();
    let fallback_model = config.fallback_model().to_string();

    info!(
        thinking_mode = %reasoning_config.thinking_mode,
        reasoning_effort = %reasoning_config.reasoning_effort,
        display_reasoning = reasoning_config.display_reasoning,
        collapsible_reasoning = reasoning_config.collapsible_reasoning,
        missing_reasoning_strategy = %reasoning_config.missing_reasoning_strategy,
        "Reasoning configuration"
    );

    let keys = dashmap::DashMap::new();

    let state = Arc::new(GatewayState {
        router,
        tiered_cache,
        semantic_cache,
        coalescer: Arc::new(RequestCoalescer::new()),
        reasoning_store,
        api_key: config.api_key.clone(),
        conn_config,
        reasoning_config,
        upstream_base_url,
        fallback_model,
        keys,
    });

    let proxy = GatewayProxy::new(state);
    let mut proxy_service = http_proxy_service(&server.configuration, proxy);
    proxy_service.add_tcp(&config.listen_addr);

    server.add_service(proxy_service);

    info!(
        listen_addr = %config.listen_addr,
        metrics_addr = %config.metrics_addr,
        "CrabCache gateway starting"
    );

    server.run_forever();
}
