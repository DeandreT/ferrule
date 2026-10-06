//! Explicit, bounded JSON HTTP requests supplied by an execution host.
//! Project metadata never enables this transport or supplies credentials.

use std::error::Error;
use std::fmt;
use std::path::Path;
use std::time::Duration;

mod mapping;
mod validation;

pub use mapping::{
    RestJsonMappingOptions, run_project_rest_json_request_file_payloads,
    run_project_value_rest_json_payloads,
};

pub const MAX_REST_JSON_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_REST_JSON_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_REST_JSON_HEADERS: usize = 64;
pub const MAX_REST_JSON_URL_BYTES: usize = 4096;

/// Each call requires an explicit grant. This is not a remote transaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RestExecutionPolicy {
    #[default]
    Deny,
    AllowSingleRequest {
        allow_insecure_http: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestJsonMethod {
    Get,
    Post,
}

/// Host-owned header values. Deliberately has no Debug or serialization impl.
#[derive(Clone, Copy)]
pub struct RestJsonHeader<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

/// Host-owned transport data, separate from a mapping project. Neither the
/// endpoint nor the body/credentials are exposed through Debug or errors.
pub struct RestJsonRequest<'a> {
    url: &'a str,
    method: RestJsonMethod,
    body: Option<&'a [u8]>,
    headers: &'a [RestJsonHeader<'a>],
    timeout: ::mapping::HttpTimeoutSeconds,
}

impl<'a> RestJsonRequest<'a> {
    pub fn get(url: &'a str) -> Self {
        Self {
            url,
            method: RestJsonMethod::Get,
            body: None,
            headers: &[],
            timeout: ::mapping::HttpTimeoutSeconds::default(),
        }
    }

    pub fn post(url: &'a str, body: &'a [u8]) -> Self {
        Self {
            method: RestJsonMethod::Post,
            body: Some(body),
            ..Self::get(url)
        }
    }

    pub fn with_headers(mut self, headers: &'a [RestJsonHeader<'a>]) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_timeout(mut self, timeout: ::mapping::HttpTimeoutSeconds) -> Self {
        self.timeout = timeout;
        self
    }
}

/// One complete validated response. No response headers, endpoint, or cookies
/// are retained. Reading bytes is an explicit operation by the host.
pub struct RestJsonResponse {
    status: u16,
    body: Vec<u8>,
}

impl fmt::Debug for RestJsonResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RestJsonResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

impl RestJsonResponse {
    pub fn status(&self) -> u16 {
        self.status
    }
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.body
    }
}

/// Transport variants contain only safe categories and counts. Mapping causes
/// retain the existing native errors and may contain application data.
#[derive(Debug)]
pub enum RestJsonError {
    Disabled,
    RequestPolicy(&'static str),
    RequestJson,
    RequestLimit { limit: usize },
    Transport,
    Timeout,
    HttpStatus(u16),
    ResponseMetadata,
    ResponseLimit { limit: usize },
    ResponseUtf8,
    ResponseJson,
    Cancelled,
    Mapping(anyhow::Error),
}

impl RestJsonError {
    pub fn phase(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::RequestPolicy(_) => "request-policy",
            Self::RequestJson => "request-json",
            Self::RequestLimit { .. } => "request-limit",
            Self::Transport => "transport",
            Self::Timeout => "timeout",
            Self::HttpStatus(_) => "http-status",
            Self::ResponseMetadata => "response-metadata",
            Self::ResponseLimit { .. } => "response-limit",
            Self::ResponseUtf8 => "response-utf8",
            Self::ResponseJson => "response-json",
            Self::Cancelled => "cancelled",
            Self::Mapping(_) => "mapping",
        }
    }
}

impl fmt::Display for RestJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => f.write_str("live REST execution requires an explicit host grant"),
            Self::RequestPolicy(reason) => write!(f, "REST request policy: {reason}"),
            Self::RequestJson => f.write_str("REST request body must be strict UTF-8 JSON"),
            Self::RequestLimit { limit } => {
                write!(f, "REST request exceeds the {limit}-byte limit")
            }
            Self::Transport => {
                f.write_str("REST transport failed; remote effects may have occurred")
            }
            Self::Timeout => {
                f.write_str("REST request timed out; remote effects may have occurred")
            }
            Self::HttpStatus(status) => {
                write!(f, "REST response status {status} is not successful")
            }
            Self::ResponseMetadata => {
                f.write_str("REST response requires uncompressed UTF-8 JSON metadata")
            }
            Self::ResponseLimit { limit } => {
                write!(f, "REST response exceeds the {limit}-byte limit")
            }
            Self::ResponseUtf8 => f.write_str("REST response is not UTF-8"),
            Self::ResponseJson => f.write_str("REST response is not one strict JSON document"),
            Self::Cancelled => f.write_str("REST execution cancelled before mapping publication"),
            Self::Mapping(error) => write!(f, "REST response mapping failed: {error}"),
        }
    }
}

impl Error for RestJsonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Mapping(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

fn transport_error(error: ureq::Error) -> RestJsonError {
    match error {
        ureq::Error::Timeout(_) => RestJsonError::Timeout,
        ureq::Error::LargeResponseHeader(_, _) => RestJsonError::ResponseMetadata,
        ureq::Error::BodyExceedsLimit(_) => RestJsonError::ResponseLimit {
            limit: MAX_REST_JSON_BODY_BYTES,
        },
        _ => RestJsonError::Transport,
    }
}

/// Performs one request, with no redirects, retries, or environment proxy.
/// The repository's resolved ureq build must exclude gzip, brotli, charset,
/// and cookies; these features change response handling before this API sees
/// headers. A host-installed dependency Trace logger can expose URL queries.
pub fn fetch_rest_json(
    request: &RestJsonRequest<'_>,
    policy: RestExecutionPolicy,
) -> Result<RestJsonResponse, RestJsonError> {
    let allow_http = validation::request(request, policy)?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(u64::from(request.timeout.get()))))
        .https_only(!allow_http)
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .max_response_header_size(MAX_REST_JSON_HEADER_BYTES)
        .max_idle_connections(0)
        .max_idle_connections_per_host(0)
        .build();
    let agent: ureq::Agent = config.into();
    let response = match request.method {
        RestJsonMethod::Get => {
            let mut call = agent
                .get(request.url)
                .header("Accept", "application/json")
                .header("Accept-Encoding", "identity");
            for header in request.headers {
                call = call.header(header.name, header.value);
            }
            call.call()
        }
        RestJsonMethod::Post => {
            let mut call = agent
                .post(request.url)
                .header("Accept", "application/json")
                .header("Accept-Encoding", "identity")
                .header("Content-Type", "application/json");
            for header in request.headers {
                call = call.header(header.name, header.value);
            }
            call.send(
                request
                    .body
                    .ok_or(RestJsonError::RequestPolicy("POST requires a body"))?,
            )
        }
    };
    let mut response = response.map_err(transport_error)?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(RestJsonError::HttpStatus(status));
    }
    validation::response_metadata(response.headers())?;
    // One extra raw byte distinguishes an exact-limit body from overflow.
    // The library limit also bounds a body without Content-Length.
    let body = response
        .body_mut()
        .with_config()
        .limit((MAX_REST_JSON_BODY_BYTES + 1) as u64)
        .read_to_vec()
        .map_err(transport_error)?;
    if body.len() > MAX_REST_JSON_BODY_BYTES {
        return Err(RestJsonError::ResponseLimit {
            limit: MAX_REST_JSON_BODY_BYTES,
        });
    }
    let text = std::str::from_utf8(&body).map_err(|_| RestJsonError::ResponseUtf8)?;
    serde_json::from_str::<serde_json::Value>(text).map_err(|_| RestJsonError::ResponseJson)?;
    Ok(RestJsonResponse { status, body })
}

fn read_host_file(path: &Path, limit: usize) -> Result<Vec<u8>, RestJsonError> {
    use std::io::Read;
    let file = std::fs::File::open(path)
        .map_err(|_| RestJsonError::RequestPolicy("cannot read host request file"))?;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| RestJsonError::RequestPolicy("cannot read host request file"))?;
    if bytes.len() > limit {
        return Err(RestJsonError::RequestLimit { limit });
    }
    Ok(bytes)
}
