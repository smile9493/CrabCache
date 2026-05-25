use pingora_core::protocols::l4::ext::TcpKeepalive;
use pingora_core::upstreams::peer::{ALPN, PeerOptions};
use std::time::Duration;

use crate::context::ConnectionConfig;

pub fn apply_connection_options(config: &ConnectionConfig, options: &mut PeerOptions) {
    if config.upstream_force_http1 {
        options.set_http_version(1, 1);
        options.alpn = ALPN::H1;
        options.h2_ping_interval = None;
        options.max_h2_streams = 1;
    } else if let Some(ping_secs) = config.h2_ping_interval_secs
        && ping_secs > 0
    {
        options.h2_ping_interval = Some(Duration::from_secs(ping_secs));
    }

    if !config.upstream_tls_curves.is_empty() {
        use std::sync::OnceLock;
        static CACHED_CURVES: OnceLock<&'static str> = OnceLock::new();
        let curves: &'static str = *CACHED_CURVES
            .get_or_init(|| Box::leak(config.upstream_tls_curves.clone().into_boxed_str()));
        options.curves = Some(curves);
    }

    if let (Some(idle), Some(interval), Some(count)) = (
        config.tcp_keepalive_idle_secs,
        config.tcp_keepalive_interval_secs,
        config.tcp_keepalive_count,
    ) {
        options.tcp_keepalive = Some(TcpKeepalive {
            idle: Duration::from_secs(idle),
            interval: Duration::from_secs(interval),
            count,
            user_timeout: Duration::from_secs(0),
        });
    }

    if config.upstream_disable_keepalive {
        options.idle_timeout = Some(Duration::from_secs(0));
    } else if let Some(idle_secs) = config.idle_timeout_secs {
        options.idle_timeout = Some(Duration::from_secs(idle_secs));
    }

    if let Some(secs) = config.upstream_connection_timeout_secs {
        if secs > 0 {
            options.connection_timeout = Some(Duration::from_secs(secs));
        }
    }

    if let Some(secs) = config.upstream_write_timeout_secs {
        if secs > 0 {
            options.write_timeout = Some(Duration::from_secs(secs));
        }
    }

    if let Some(secs) = config.upstream_request_timeout_secs {
        if secs > 0 {
            options.read_timeout = Some(Duration::from_secs(secs));
        }
    }
}
