use pingora_http::ResponseHeader;
use pingora_proxy::Session;

pub(crate) async fn send_json_error_with_retry_after(
    session: &mut Session,
    status: http::StatusCode,
    body: &[u8],
    retry_after_secs: u64,
) -> bool {
    send_json_error_inner(session, status, body, Some(retry_after_secs)).await
}

pub(crate) async fn send_json_error(
    session: &mut Session,
    status: http::StatusCode,
    body: &[u8],
) -> bool {
    send_json_error_inner(session, status, body, None).await
}

pub(crate) async fn send_json_ok(session: &mut Session, body: &[u8]) -> bool {
    send_json_error_inner(session, http::StatusCode::OK, body, None).await
}

async fn send_json_error_inner(
    session: &mut Session,
    status: http::StatusCode,
    body: &[u8],
    retry_after_secs: Option<u64>,
) -> bool {
    let mut header = match ResponseHeader::build(status, Some(8)) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let _ = header.insert_header("content-type", "application/json");
    let _ = header.insert_header("content-length", body.len().to_string());
    let _ = header.insert_header("connection", "close");
    if let Some(secs) = retry_after_secs {
        let _ = header.insert_header("retry-after", secs.to_string());
    }
    if session
        .downstream_session
        .write_response_header(Box::new(header))
        .await
        .is_err()
    {
        return false;
    }
    session
        .downstream_session
        .write_response_body(bytes::Bytes::copy_from_slice(body), true)
        .await
        .is_ok()
}

pub(crate) async fn send_cors_preflight(session: &mut Session) -> bool {
    let mut header = match ResponseHeader::build(http::StatusCode::NO_CONTENT, Some(8)) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let _ = header.insert_header("access-control-allow-origin", "*");
    let _ = header.insert_header("access-control-allow-methods", "GET, POST, OPTIONS");
    let _ = header.insert_header(
        "access-control-allow-headers",
        "Authorization, Content-Type, X-Request-Id, X-Conversation-Id, X-Consumer, X-Project-Id",
    );
    let _ = header.insert_header("access-control-max-age", "86400");
    let _ = header.insert_header("content-length", "0");
    session
        .downstream_session
        .write_response_header(Box::new(header))
        .await
        .is_ok()
}
