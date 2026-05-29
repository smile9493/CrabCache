use axum::{
    body::Body,
    http::{Request, Response, StatusCode, header},
    middleware::Next,
    response::IntoResponse,
};

fn is_built_asset_path(path: &str) -> bool {
    path.starts_with("/crab-dashboard-")
        || path.starts_with("/output-")
        || path.ends_with(".js")
        || path.ends_with(".wasm")
        || path.ends_with(".css")
        || path.ends_with(".svg")
        || path.ends_with(".ico")
        || path.ends_with(".woff2")
}

/// `index.html` must not be cached so WASM/JS hash updates take effect immediately.
pub async fn static_cache_headers(req: Request<Body>, next: Next) -> Response<Body> {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;

    // During active dashboard iteration, prefer freshness over long-lived immutable
    // caching to prevent clients (especially mobile/Firefox) from pinning stale bundles.
    let cache_value = if path == "/"
        || path == "/index.html"
        || path.ends_with(".html")
        || path.ends_with(".js")
        || path.ends_with(".wasm")
        || path.ends_with(".css")
    {
        Some("no-store, no-cache, must-revalidate, max-age=0")
    } else {
        None
    };

    if let Some(value) = cache_value
        && let Ok(header_value) = value.parse()
    {
        res.headers_mut()
            .insert(header::CACHE_CONTROL, header_value);
        res.headers_mut()
            .insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
        res.headers_mut()
            .insert(header::EXPIRES, header::HeaderValue::from_static("0"));
    }

    // ServeDir SPA fallback returns index.html for missing hashed assets; treat as 404.
    if is_built_asset_path(&path)
        && res.status().is_success()
        && res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("text/html"))
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    res
}
