use std::path::Path;

/// Default 1Panel OpenResty site config directory on the host.
pub const DEFAULT_CONF_DIR: &str = "/opt/1panel/www/conf.d";

/// Upstream target that identifies the CrabCache API `server` block.
const DEFAULT_GATEWAY_UPSTREAM: &str = "127.0.0.1:8080";

/// Parse OpenResty/Nginx site configs and return the public client base URL for the gateway
/// (e.g. `https://v4.example.com:18000`) when a `server` block proxies to `gateway_upstream`.
pub fn detect_gateway_base_url(conf_dir: &Path, gateway_upstream: &str) -> Option<String> {
    let entries = std::fs::read_dir(conf_dir).ok()?;
    let mut found = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("conf") {
            continue;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        found.extend(parse_conf_content(&content, gateway_upstream));
    }

    found.sort();
    found.dedup();
    found.into_iter().next()
}

/// Parse a single config file body (unit-testable).
pub fn parse_conf_content(content: &str, gateway_upstream: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for block in split_server_blocks(content) {
        if let Some(url) = parse_server_block(&block, gateway_upstream) {
            urls.push(url);
        }
    }
    urls
}

fn split_server_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;

    for line in content.lines() {
        let trimmed = line.trim();
        if depth == 0 {
            if trimmed.starts_with("server") && trimmed.contains('{') {
                current.clear();
                current.push_str(line);
                current.push('\n');
                depth += brace_delta(line);
            }
            continue;
        }

        current.push_str(line);
        current.push('\n');
        depth += brace_delta(line);
        if depth == 0 && !current.is_empty() {
            blocks.push(current.clone());
            current.clear();
        }
    }

    blocks
}

fn brace_delta(line: &str) -> i32 {
    let open = line.matches('{').count() as i32;
    let close = line.matches('}').count() as i32;
    open - close
}

fn parse_server_block(block: &str, gateway_upstream: &str) -> Option<String> {
    if !block_contains_gateway_upstream(block, gateway_upstream) {
        return None;
    }

    let listen = parse_listen(block)?;
    let server_name = parse_server_name(block)?;
    let scheme = if listen.ssl { "https" } else { "http" };
    let port = listen.port;
    let host = server_name;

    if (scheme == "https" && port == 443) || (scheme == "http" && port == 80) {
        Some(format!("{scheme}://{host}"))
    } else {
        Some(format!("{scheme}://{host}:{port}"))
    }
}

fn block_contains_gateway_upstream(block: &str, gateway_upstream: &str) -> bool {
    let needle = format!("proxy_pass http://{gateway_upstream}");
    let needle_slash = format!("proxy_pass http://{gateway_upstream}/");
    block.contains(&needle) || block.contains(&needle_slash)
}

struct ListenInfo {
    port: u16,
    ssl: bool,
}

fn parse_listen(block: &str) -> Option<ListenInfo> {
    for line in block.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("listen ") {
            continue;
        }
        let rest = trimmed.strip_prefix("listen ")?.trim().trim_end_matches(';');
        let ssl = rest.contains("ssl");
        let port_token = rest
            .split_whitespace()
            .next()
            .unwrap_or("80")
            .trim_matches(|c: char| !c.is_ascii_digit());
        let port: u16 = port_token.parse().unwrap_or(80);
        return Some(ListenInfo { port, ssl });
    }
    None
}

fn parse_server_name(block: &str) -> Option<String> {
    for line in block.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("server_name ") {
            let name = name.trim().trim_end_matches(';').split_whitespace().next()?;
            if name != "_" && name != "localhost" {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
server {
    listen 18000 ssl;
    server_name v4.wumingaicg.website;
    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}
server {
    listen 18010 ssl;
    server_name v4.wumingaicg.website;
    location / {
        proxy_pass http://127.0.0.1:18001;
    }
}
"#;

    #[test]
    fn parses_api_server_not_admin() {
        let urls = parse_conf_content(SAMPLE, DEFAULT_GATEWAY_UPSTREAM);
        assert_eq!(urls, vec!["https://v4.wumingaicg.website:18000".to_string()]);
    }

    #[test]
    fn parses_nested_location_blocks() {
        let conf = r#"
server {
    listen 18000 ssl;
    server_name v4.wumingaicg.website;
    location ^~ /.well-known {
        root /usr/share/nginx/html;
    }
    location ^~ / {
        proxy_pass http://127.0.0.1:8080;
    }
}
"#;
        let urls = parse_conf_content(conf, DEFAULT_GATEWAY_UPSTREAM);
        assert_eq!(urls, vec!["https://v4.wumingaicg.website:18000".to_string()]);
    }

    #[test]
    fn ignores_blocks_without_gateway_upstream() {
        let conf = r#"
server {
    listen 18010 ssl;
    server_name admin.example.com;
    proxy_pass http://127.0.0.1:18001;
}
"#;
        assert!(parse_conf_content(conf, DEFAULT_GATEWAY_UPSTREAM).is_empty());
    }
}
