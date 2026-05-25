use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub primary_ip: Option<String>,
    pub all_ips: Vec<NetworkInterface>,
    pub gateway_url: String,
    pub gateway_url_lan: Option<String>,
    #[serde(default)]
    pub gateway_url_openresty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub ip: String,
    pub is_primary: bool,
}
