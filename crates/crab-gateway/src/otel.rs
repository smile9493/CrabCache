//! OpenTelemetry tracing integration (optional, feature-gated).
//!
//! Enabled at compile time via `--features otel` and at runtime via:
//!   CRABCACHE_OTEL_ENABLED=true
//!   CRABCACHE_OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
//!
//! When disabled (default), this module is a no-op.

use tracing::Subscriber;
use tracing_subscriber::Layer;

/// Configuration parsed from environment variables.
pub struct OtelConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub service_name: String,
    pub sample_ratio: f64,
}

impl OtelConfig {
    /// Parse OTel configuration from environment variables.
    /// Returns a config regardless of whether OTel is enabled.
    pub fn from_env() -> Self {
        let enabled = std::env::var("CRABCACHE_OTEL_ENABLED")
            .ok()
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let endpoint = std::env::var("CRABCACHE_OTEL_EXPORTER_OTLP_ENDPOINT").ok();

        let service_name = std::env::var("CRABCACHE_OTEL_SERVICE_NAME")
            .unwrap_or_else(|_| "crabcache-gateway".to_string());

        let sample_ratio = std::env::var("CRABCACHE_OTEL_SAMPLE_RATIO")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.05)
            .clamp(0.0, 1.0);

        Self {
            enabled,
            endpoint,
            service_name,
            sample_ratio,
        }
    }

    /// Whether OTel tracing should be activated (enabled + endpoint present).
    pub fn should_activate(&self) -> bool {
        self.enabled && self.endpoint.is_some()
    }
}

/// Build an optional OTel tracing layer.
///
/// Returns `None` if OTel is disabled or misconfigured.
/// When active, installs an OTLP gRPC exporter and a parent-based sampler.
pub fn build_otel_layer<S>(
    config: &OtelConfig,
) -> Option<Box<dyn Layer<S> + Send + Sync>>
where
    S: Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span> + Send + Sync,
{
    if !config.should_activate() {
        if config.enabled && config.endpoint.is_none() {
            tracing::warn!(
                "OTel enabled but CRABCACHE_OTEL_EXPORTER_OTLP_ENDPOINT not set; OTel tracing disabled"
            );
        }
        return None;
    }

    let endpoint = config.endpoint.as_ref().unwrap();

    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::trace::SdkTracerProvider;

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
    {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "Failed to build OTLP span exporter");
            return None;
        }
    };

    let sampler = opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(config.sample_ratio);

    let provider = SdkTracerProvider::builder()
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attribute(opentelemetry::KeyValue::new(
                    opentelemetry_semantic_conventions::resource::SERVICE_NAME,
                    config.service_name.clone(),
                ))
                .build(),
        )
        .with_batch_exporter(exporter)
        .with_sampler(sampler)
        .build();

    let tracer = provider.tracer("crabcache-gateway");
    opentelemetry::global::set_tracer_provider(provider);

    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_tracked_inactivity(true);

    Some(Box::new(otel_layer))
}

/// Parse a W3C `traceparent` header value.
///
/// Format: `{version:2}-{trace_id:32}-{span_id:16}-{flags:2}`
/// Returns `(trace_id_hex, span_id_hex, flags)` or `None` if invalid.
pub fn parse_traceparent(value: &str) -> Option<(&str, &str, &str)> {
    let value = value.trim();
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 4 {
        return None;
    }
    if parts[0] != "00" {
        return None; // only version 00 supported
    }
    if parts[1].len() != 32 || parts[2].len() != 16 || parts[3].len() != 2 {
        return None;
    }
    // Validate hex
    if !parts[1].chars().all(|c| c.is_ascii_hexdigit())
        || !parts[2].chars().all(|c| c.is_ascii_hexdigit())
        || !parts[3].chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    Some((parts[1], parts[2], parts[3]))
}

/// Generate a new traceparent header value with cryptographically random IDs.
pub fn generate_traceparent() -> String {
    let mut buf = [0u8; 24]; // 16 bytes trace_id + 8 bytes span_id
    getrandom::fill(&mut buf).expect("getrandom failed");
    let trace_id = hex::encode(&buf[..16]);
    let span_id = hex::encode(&buf[16..]);
    format!("00-{}-{}-01", trace_id, span_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_traceparent_valid() {
        let tp = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let (trace_id, span_id, flags) = parse_traceparent(tp).unwrap();
        assert_eq!(trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(span_id, "b7ad6b7169203331");
        assert_eq!(flags, "01");
    }

    #[test]
    fn test_parse_traceparent_invalid_version() {
        assert!(parse_traceparent("01-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01").is_none());
    }

    #[test]
    fn test_parse_traceparent_wrong_parts() {
        assert!(parse_traceparent("00-abc-def").is_none());
    }

    #[test]
    fn test_parse_traceparent_non_hex() {
        assert!(parse_traceparent("00-ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ-b7ad6b7169203331-01").is_none());
    }

    #[test]
    fn test_generate_traceparent_format() {
        let tp = generate_traceparent();
        assert!(parse_traceparent(&tp).is_some());
    }

    #[test]
    fn test_otel_config_from_env_defaults() {
        // Clear env vars for clean test (unsafe in Rust 2024 edition)
        unsafe {
            std::env::remove_var("CRABCACHE_OTEL_ENABLED");
            std::env::remove_var("CRABCACHE_OTEL_EXPORTER_OTLP_ENDPOINT");
            std::env::remove_var("CRABCACHE_OTEL_SERVICE_NAME");
            std::env::remove_var("CRABCACHE_OTEL_SAMPLE_RATIO");
        }

        let config = OtelConfig::from_env();
        assert!(!config.enabled);
        assert!(config.endpoint.is_none());
        assert_eq!(config.service_name, "crabcache-gateway");
        assert!((config.sample_ratio - 0.05).abs() < f64::EPSILON);
        assert!(!config.should_activate());
    }
}
