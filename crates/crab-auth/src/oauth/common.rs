use base64::Engine;
use rand::Rng;
use sha2::Digest;
use std::future::Future;
use std::pin::Pin;
use tokio::time::{Duration, sleep};

/// Result from the OAuth callback
pub struct CallbackResult {
    pub code: String,
    pub state: String,
}

/// Generate PKCE code verifier (96 random bytes -> 128-char base64url string)
pub fn generate_pkce_verifier() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..96).map(|_| rng.r#gen()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

/// Generate PKCE challenge from verifier (SHA256 -> base64url)
pub fn generate_pkce_challenge(verifier: &str) -> String {
    let hash = sha2::Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
}

/// Generate a random state string (32 hex chars)
pub fn generate_random_state() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.r#gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Generate a random nonce string (32 hex chars)
pub fn generate_random_nonce() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.r#gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Start a local HTTP server to receive the OAuth callback.
///
/// Listens on `127.0.0.1:{port}` for a single request at `callback_path`.
/// Extracts `code` and `state` query parameters and sends them via a oneshot channel.
/// If `success_path` is provided and the request matches it, serves an HTML success page.
pub async fn start_callback_server(
    port: u16,
    callback_path: &str,
    success_path: Option<&str>,
) -> Result<
    (
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Receiver<CallbackResult>,
    ),
    std::io::Error,
> {
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let callback_path = callback_path.to_string();
    let success_path = success_path.map(|s| s.to_string());
    let (tx, rx) = tokio::sync::oneshot::channel::<CallbackResult>();

    let handle = tokio::spawn(async move {
        // Accept a single connection
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };

        // Read the HTTP request
        let mut buf = vec![0u8; 4096];
        let n = match tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
            Ok(n) => n,
            Err(_) => return,
        };
        let request = String::from_utf8_lossy(&buf[..n]);

        // Parse the request line (first line)
        let first_line = request.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() < 2 {
            return;
        }
        let path_with_query = parts[1];

        // Split path and query
        let (path, query_str) = match path_with_query.find('?') {
            Some(i) => (&path_with_query[..i], &path_with_query[i + 1..]),
            None => (path_with_query, ""),
        };

        // Check if this is the success path (just serve HTML)
        if let Some(ref sp) = success_path
            && path == sp.as_str()
        {
            let body = "<html><body><h1>Authorization Successful</h1><p>You may close this window.</p></body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut stream).await;
            return;
        }

        // If not the callback path, return 404
        if path != callback_path {
            let response =
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut stream).await;
            return;
        }

        // Parse query parameters for code and state
        let mut code = None;
        let mut state = None;
        for param in query_str.split('&') {
            if let Some((key, value)) = param.split_once('=') {
                match key {
                    "code" => code = Some(decode_query_value(value)),
                    "state" => state = Some(decode_query_value(value)),
                    _ => {}
                }
            }
        }

        // Send success response to browser
        let body = "<html><body><h1>Authorization Successful</h1><p>You may close this window and return to the terminal.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
        let _ = tokio::io::AsyncWriteExt::shutdown(&mut stream).await;

        // Send the result through the channel
        if let (Some(code), Some(state)) = (code, state) {
            let _ = tx.send(CallbackResult { code, state });
        }
    });

    Ok((handle, rx))
}

/// Decode a percent-encoded query value
fn decode_query_value(value: &str) -> String {
    url::form_urlencoded::parse(value.as_bytes())
        .map(|(_k, v)| v.into_owned())
        .next()
        .unwrap_or_else(|| value.to_string())
}

/// Print the URL for the user to open in their browser
pub fn open_browser(url: &str) {
    println!("Open this URL in your browser:\n{url}");
}

/// Retry an async operation with linear backoff (1s, 2s, ... between attempts).
///
/// Non-retryable errors return immediately. `max_retries` is the total number of attempts.
pub async fn retry_with_backoff<T, E>(
    max_retries: usize,
    is_retryable: impl Fn(&E) -> bool,
    mut op: impl FnMut() -> Pin<Box<dyn Future<Output = Result<T, E>> + Send>>,
) -> Result<T, E> {
    if max_retries == 0 {
        return op().await;
    }

    let mut last_err = None;
    for attempt in 0..max_retries {
        if attempt > 0 {
            sleep(Duration::from_secs(attempt as u64)).await;
        }
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if !is_retryable(&err) {
                    return Err(err);
                }
                last_err = Some(err);
            }
        }
    }

    Err(last_err.expect("max_retries > 0 guarantees at least one attempt"))
}
