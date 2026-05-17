use crab_gateway::config::GatewayConfig;
use crab_gateway::management::{serve as serve_management, ManagementState};
use anyhow::Result;
use async_trait::async_trait;
use crab_cache::{FingerprintConfig, RequestCoalescer, TieredCache, TtlConfig};
use crab_metrics::global_metrics;
use crab_proxy::{GatewayProxy, GatewayState, RuntimeConfig};
use crab_reasoning::ReasoningStore;
use crab_route::AffinityRouter;
use crab_semantic::{EmbedderPool, SemanticGateConfig, SemanticCache, VectorStore};
use pingora_core::server::Server;
use pingora_core::services::background::background_service;
use pingora_proxy::http_proxy_service;
use prometheus::Registry;
use std::sync::{Arc, RwLock};
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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
        let location = panic_info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "unknown".to_string());

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

    std::fs::create_dir_all("./logs").ok();

    let file_appender = tracing_appender::rolling::daily("./logs", "gateway.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(non_blocking);

    let stdout_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stdout);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(json_layer)
        .with(stdout_layer)
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/gateway.toml".to_string());

    let config = GatewayConfig::load(&config_path)?;
    info!(config_path = %config_path, "Configuration loaded");

    if let Err(errors) = config.validate() {
        for err in &errors {
            tracing::error!("Configuration validation error: {}", err);
        }
        anyhow::bail!("Configuration validation failed with {} error(s)", errors.len());
    }
    info!("Configuration validated successfully");

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

    let router = rt.block_on(async { AffinityRouter::new(&backends) })?;

    let l1_pool = rt.block_on(async {
        bb8::Pool::builder()
            .max_size(config.cache.l1_pool_size.unwrap_or(16))
            .build(bb8_redis::RedisConnectionManager::new(
                config.cache.l1_redis_url.clone(),
            )?)
            .await
    })?;

    let ttl_config = Arc::new(RwLock::new(TtlConfig {
        default_ttl_secs: config.cache.default_ttl_secs.unwrap_or(3600),
        model_overrides: config.cache.model_ttl_overrides.clone().unwrap_or_default(),
        consumer_overrides: config.cache.consumer_ttl_overrides.clone().unwrap_or_default(),
    }));

    let l0_config = crab_cache::L0Config {
        max_capacity: config.cache.l0_max_capacity.unwrap_or(10_000),
        ttl_secs: config.cache.l0_ttl_secs.unwrap_or(3600),
    };

    let tiered_cache = Arc::new(
        rt.block_on(async { TieredCache::new(l1_pool, l0_config, ttl_config.clone()).await })?,
    );

    let semantic_cache = if config.semantic.enabled {
        let pool = EmbedderPool::load(
            &config.semantic.model_path,
            &config.semantic.tokenizer_path,
            config.semantic.max_concurrent_embeds,
        )?;

        let store = VectorStore::new(
            &config.semantic.qdrant_url,
            &config.semantic.collection_name,
            config.semantic.vector_size.unwrap_or(384),
        );

        let store = rt.block_on(store)?;

        let cache = SemanticCache::new(
            Arc::new(pool),
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
    let reasoning_store = Arc::new(ReasoningStore::new(
        &reasoning_config.cache_db_path,
        reasoning_config.cache_max_age_secs,
        reasoning_config.cache_max_rows,
    )?);

    let upstream_base_url = config.upstream_base_url().to_string();
    let fallback_model = config.fallback_model().to_string();
    let mgmt_cfg = config.management_config();
    let mgmt_listen = mgmt_cfg.listen_addr.clone();
    let mgmt_admin_key = mgmt_cfg.admin_key.into_inner();

    let bootstrap_api_key = config.api_key.into_inner();

    info!(
        thinking_mode = %reasoning_config.thinking_mode,
        reasoning_effort = %reasoning_config.reasoning_effort,
        display_reasoning = reasoning_config.display_reasoning,
        collapsible_reasoning = reasoning_config.collapsible_reasoning,
        missing_reasoning_strategy = %reasoning_config.missing_reasoning_strategy,
        "Reasoning configuration"
    );

    let trace_logger = if let Some(trace_config) = &config.trace_logging {
        if trace_config.enabled {
            let (logger, _handle) = crab_proxy::TraceLogger::init(crab_proxy::TraceConfig {
                enabled: trace_config.enabled,
                path: trace_config.path.clone(),
                max_lines: trace_config.max_lines,
                max_files: trace_config.max_files,
            });
            info!(
                path = %trace_config.path,
                max_lines = trace_config.max_lines,
                max_files = trace_config.max_files,
                "Trace logging enabled"
            );
            Some(Arc::new(logger))
        } else {
            None
        }
    } else {
        None
    };

    let runtime = RuntimeConfig::new(
        router,
        ttl_config,
        conn_config,
        config.cache.stream_cache_enabled,
        FingerprintConfig {
            version: config.cache.fingerprint_version,
            normalize_content: config.cache.fingerprint_normalize_content,
        },
        upstream_base_url,
        fallback_model,
        bootstrap_api_key.clone(),
    );
    runtime.insert_bootstrap_key(&bootstrap_api_key, "default");

    // Background task: TCP health check for upstream backends
    {
        let runtime = runtime.clone();
        let health_interval = config.upstream.health_check_interval_secs;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("health check runtime");
            rt.block_on(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(health_interval));
                loop {
                    interval.tick().await;
                    let backends: Vec<(String, std::net::SocketAddr)> = {
                        let router = runtime.router.read()
                            .map_err(|e| tracing::error!(error=%e, "Router lock poisoned"))
                            .ok();
                        match router {
                            Some(r) => r.backends().iter().map(|b| (b.name.clone(), b.addr)).collect(),
                            None => continue,
                        }
                    };

                    for (name, addr) in &backends {
                        let start = std::time::Instant::now();
                        let result = tokio::net::TcpStream::connect(addr).await;
                        let elapsed_ms = start.elapsed().as_millis() as u64;

                        let health = match result {
                            Ok(_) => crab_route::BackendHealth {
                                healthy: true,
                                last_check_ms: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64,
                                latency_ms: elapsed_ms,
                            },
                            Err(e) => {
                                tracing::warn!(backend = %name, addr = %addr, error = %e, "Health check failed");
                                crab_route::BackendHealth {
                                    healthy: false,
                                    last_check_ms: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as u64,
                                    latency_ms: 0,
                                }
                            }
                        };

                        if let Ok(mut health_map) = runtime.backend_health.write() {
                            health_map.insert(name.clone(), health);
                        }
                    }
                }
            });
        });
    }

    let mgmt_state = ManagementState {
        runtime: runtime.clone(),
        admin_key: mgmt_admin_key,
    };

    let mgmt_listen_thread = mgmt_listen.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("management runtime");
        rt.block_on(async {
            if let Err(e) = serve_management(&mgmt_listen_thread, mgmt_state).await {
                tracing::error!(error = %e, "Management API server failed");
            }
        });
    });

    let semantic_gate = SemanticGateConfig {
        min_query_chars: config.semantic.min_query_chars,
        max_query_chars: config.semantic.max_query_chars,
        embed_only_on_exact_miss: config.semantic.embed_only_on_exact_miss,
    };

    let state = Arc::new(GatewayState {
        runtime,
        tiered_cache,
        semantic_cache,
        semantic_gate,
        coalescer: {
            let max_inflight = config.upstream.max_coalesce_inflight.unwrap_or(1000);
            let timeout = config.upstream.coalesce_timeout_secs.unwrap_or(60);
            Arc::new(RequestCoalescer::with_config(max_inflight, timeout))
        },
        reasoning_store,
        reasoning_config,
        trace_logger,
        cache_key_namespace: config.cache.cache_key_namespace.clone(),
        pricing: config.cache.pricing.clone().unwrap_or_default(),
    });

    let proxy = GatewayProxy::new(state);
    let mut proxy_service = http_proxy_service(&server.configuration, proxy);
    proxy_service.add_tcp(&config.listen_addr);

    server.add_service(proxy_service);

    info!(
        listen_addr = %config.listen_addr,
        metrics_addr = %config.metrics_addr,
        management_addr = %mgmt_listen,
        "CrabCache gateway starting"
    );

    server.run_forever();
}
