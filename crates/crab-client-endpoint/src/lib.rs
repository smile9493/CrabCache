//! Client-facing gateway base URL discovery (LAN, FRP, OpenResty, observed headers).
//!
//! Used by the Pingora gateway (runtime + management API) and the Admin dashboard.

mod discover;
mod frp;
mod lan;
mod observed;
mod openresty;

pub use discover::{
    ClientEndpointSnapshot, DiscoveryConfig, PublicUrlSource, discover,
    refresh_public_from_observed,
};
pub use lan::{get_local_ip_addresses, select_primary_private_ip};
pub use observed::url_from_forwarded_headers;
pub use openresty::{DEFAULT_CONF_DIR, detect_gateway_base_url, parse_conf_content};
