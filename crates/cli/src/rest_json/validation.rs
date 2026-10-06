use std::collections::BTreeSet;

use super::{
    MAX_REST_JSON_BODY_BYTES, MAX_REST_JSON_HEADER_BYTES, MAX_REST_JSON_HEADERS,
    MAX_REST_JSON_URL_BYTES, RestExecutionPolicy, RestJsonError, RestJsonMethod, RestJsonRequest,
};

pub(super) fn request(
    request: &RestJsonRequest<'_>,
    policy: RestExecutionPolicy,
) -> Result<bool, RestJsonError> {
    let allow_http = match policy {
        RestExecutionPolicy::Deny => return Err(RestJsonError::Disabled),
        RestExecutionPolicy::AllowSingleRequest {
            allow_insecure_http,
        } => allow_insecure_http,
    };
    if request.url.len() > MAX_REST_JSON_URL_BYTES {
        return Err(RestJsonError::RequestLimit {
            limit: MAX_REST_JSON_URL_BYTES,
        });
    }
    if request.url.contains('#')
        || request
            .url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(RestJsonError::RequestPolicy("invalid endpoint"));
    }
    let uri = request
        .url
        .parse::<ureq::http::Uri>()
        .map_err(|_| RestJsonError::RequestPolicy("invalid endpoint"))?;
    match uri.scheme_str() {
        Some("https") => {}
        Some("http") if allow_http => {}
        _ => {
            return Err(RestJsonError::RequestPolicy(
                "HTTPS required unless HTTP is explicitly allowed",
            ));
        }
    }
    if uri.host().is_none_or(str::is_empty)
        || uri
            .authority()
            .is_none_or(|authority| authority.as_str().contains('@'))
    {
        return Err(RestJsonError::RequestPolicy(
            "endpoint requires a host and forbids user information",
        ));
    }
    if request.headers.len() > MAX_REST_JSON_HEADERS {
        return Err(RestJsonError::RequestPolicy("too many host headers"));
    }
    let mut names = BTreeSet::new();
    // Include the fixed protocol headers and a bounded request line in the ledger.
    let mut bytes = request.url.len() * 2 + 512;
    for header in request.headers {
        bytes = bytes
            .checked_add(header.name.len())
            .and_then(|n| n.checked_add(header.value.len()))
            .and_then(|n| n.checked_add(4))
            .ok_or(RestJsonError::RequestLimit {
                limit: MAX_REST_JSON_HEADER_BYTES,
            })?;
        if bytes > MAX_REST_JSON_HEADER_BYTES {
            return Err(RestJsonError::RequestLimit {
                limit: MAX_REST_JSON_HEADER_BYTES,
            });
        }
        if header.name.is_empty()
            || !header.name.bytes().all(token)
            || header.value.bytes().any(|b| !(0x20..=0x7e).contains(&b))
        {
            return Err(RestJsonError::RequestPolicy("invalid host header"));
        }
        let name = header.name.to_ascii_lowercase();
        if !names.insert(name.clone()) {
            return Err(RestJsonError::RequestPolicy("duplicate host header"));
        }
        if matches!(
            name.as_str(),
            "host"
                | "content-length"
                | "transfer-encoding"
                | "connection"
                | "accept"
                | "accept-encoding"
                | "content-type"
                | "proxy-authorization"
                | "cookie"
                | "expect"
                | "te"
                | "trailer"
                | "upgrade"
                | "user-agent"
        ) {
            return Err(RestJsonError::RequestPolicy(
                "host header conflicts with transport policy",
            ));
        }
    }
    match (request.method, request.body) {
        (RestJsonMethod::Get, None) => {}
        (RestJsonMethod::Post, Some(body)) => {
            if body.len() > MAX_REST_JSON_BODY_BYTES {
                return Err(RestJsonError::RequestLimit {
                    limit: MAX_REST_JSON_BODY_BYTES,
                });
            }
            let text = std::str::from_utf8(body).map_err(|_| RestJsonError::RequestJson)?;
            serde_json::from_str::<serde_json::Value>(text)
                .map_err(|_| RestJsonError::RequestJson)?;
        }
        _ => {
            return Err(RestJsonError::RequestPolicy(
                "GET forbids a body; POST requires one",
            ));
        }
    }
    Ok(allow_http)
}

fn token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

pub(super) fn response_metadata(headers: &ureq::http::HeaderMap) -> Result<(), RestJsonError> {
    let types = headers.get_all("content-type").iter().collect::<Vec<_>>();
    if types.len() != 1 {
        return Err(RestJsonError::ResponseMetadata);
    }
    let content_type = types[0]
        .to_str()
        .map_err(|_| RestJsonError::ResponseMetadata)?;
    let mut parts = content_type.split(';');
    let media = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let json_media = media == "application/json"
        || (media.starts_with("application/")
            && media.ends_with("+json")
            && media.len() > "application/+json".len()
            && media["application/".len()..].bytes().all(token));
    if !json_media {
        return Err(RestJsonError::ResponseMetadata);
    }
    let mut charset_seen = false;
    for part in parts {
        let (name, value) = part
            .trim()
            .split_once('=')
            .ok_or(RestJsonError::ResponseMetadata)?;
        if !name.trim().eq_ignore_ascii_case("charset")
            || charset_seen
            || !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "utf-8" | "\"utf-8\""
            )
        {
            return Err(RestJsonError::ResponseMetadata);
        }
        charset_seen = true;
    }
    for encoding in headers.get_all("content-encoding").iter() {
        if !encoding
            .to_str()
            .map_err(|_| RestJsonError::ResponseMetadata)?
            .trim()
            .eq_ignore_ascii_case("identity")
        {
            return Err(RestJsonError::ResponseMetadata);
        }
    }
    for length in headers.get_all("content-length").iter() {
        let length = length
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(RestJsonError::ResponseMetadata)?;
        if length > MAX_REST_JSON_BODY_BYTES as u64 {
            return Err(RestJsonError::ResponseLimit {
                limit: MAX_REST_JSON_BODY_BYTES,
            });
        }
    }
    Ok(())
}
