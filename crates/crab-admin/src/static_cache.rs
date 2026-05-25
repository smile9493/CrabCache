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

    let cache_value = if path == "/" || path == "/index.html" {
        Some("no-cache, no-store, must-revalidate")
    } else if path.starts_with("/crab-dashboard-")
        && (path.ends_with(".js") || path.ends_with(".wasm"))
    {
        Some("public, max-age=31536000, immutable")
    } else if path.starts_with("/output-") && path.ends_with(".css") {
        Some("public, max-age=86400")
    } else {
        None
    };

    if let Some(value) = cache_value {
        if let Ok(header_value) = value.parse() {
            res.headers_mut()
                .insert(header::CACHE_CONTROL, header_value);
        }
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
