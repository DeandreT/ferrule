//! Static HTTP(S) XML acquisition and sanitized request diagnostics.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::FormatOptions;

use crate::ExecutionPolicy;

const DEFAULT_HTTP_TIMEOUT_SECONDS: u64 = 30;
const MAX_HTTP_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_HTTP_RESPONSE_HEADER_BYTES: usize = 64 * 1024;
const MAX_HTTP_REDIRECTS: u32 = 5;

pub(super) fn http_url(path: &Path) -> Option<&str> {
    let value = path.to_str()?;
    let (scheme, _) = value.split_once("://")?;
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")).then_some(value)
}

pub(super) fn read_http_xml(
    url: &str,
    schema: &SchemaNode,
    options: &FormatOptions,
    execution_policy: &ExecutionPolicy,
) -> anyhow::Result<Instance> {
    let uri = url
        .parse::<ureq::http::Uri>()
        .map_err(|_| anyhow::anyhow!("invalid HTTP source URL `{}`", sanitize_url(url)))?;
    let scheme = uri.scheme_str().unwrap_or_default();
    let is_http = scheme.eq_ignore_ascii_case("http");
    let is_https = scheme.eq_ignore_ascii_case("https");
    if (!is_http && !is_https) || uri.authority().is_none() {
        bail!("invalid HTTP source URL `{}`", sanitize_url(url));
    }
    if uri
        .authority()
        .is_some_and(|authority| authority.as_str().contains('@'))
    {
        bail!(
            "HTTP source URL `{}` must not contain credentials",
            sanitize_uri(&uri)
        );
    }

    let timeout_seconds = options
        .http_get
        .as_ref()
        .map(|http| u64::from(http.timeout_seconds().get()))
        .unwrap_or(DEFAULT_HTTP_TIMEOUT_SECONDS);
    let display_url = sanitize_uri(&uri);
    // Restricted runs follow no redirect: its target origin is unchecked.
    let max_redirects = if execution_policy.is_restricted() {
        0
    } else {
        MAX_HTTP_REDIRECTS
    };
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_seconds)))
        .max_redirects(max_redirects)
        .max_redirects_will_error(true)
        .max_response_header_size(MAX_HTTP_RESPONSE_HEADER_BYTES)
        .https_only(is_https)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(url)
        .header("User-Agent", concat!("ferrule/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|error| http_request_error(error, &display_url, timeout_seconds))?;
    if response.status().is_redirection() {
        execution_policy.authorize_capability(engine::RestrictedCapability::HttpRedirect)?;
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_HTTP_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|error| http_body_error(error, &display_url, timeout_seconds))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        anyhow::anyhow!("HTTP GET {display_url} returned a response that is not UTF-8")
    })?;
    let parsed = match &options.wsdl {
        Some(wsdl) => format_xml::from_wsdl_message_str(&text, schema, wsdl.operation()),
        None => format_xml::from_str(&text, schema),
    };
    parsed.with_context(|| format!("parsing XML response from HTTP GET {display_url}"))
}

fn http_request_error(error: ureq::Error, url: &str, timeout_seconds: u64) -> anyhow::Error {
    match error {
        ureq::Error::StatusCode(status) => {
            anyhow::anyhow!("HTTP GET {url} returned status {status}")
        }
        ureq::Error::Timeout(_) => {
            anyhow::anyhow!("HTTP GET {url} timed out after {timeout_seconds} seconds")
        }
        ureq::Error::TooManyRedirects => {
            anyhow::anyhow!("HTTP GET {url} exceeded {MAX_HTTP_REDIRECTS} redirects")
        }
        ureq::Error::RequireHttpsOnly(_) => {
            anyhow::anyhow!("HTTP GET {url} refused an insecure redirect")
        }
        ureq::Error::LargeResponseHeader(_, _) => anyhow::anyhow!(
            "HTTP GET {url} response headers exceeded {} KiB",
            MAX_HTTP_RESPONSE_HEADER_BYTES / 1024
        ),
        other => anyhow::anyhow!("HTTP GET {url} failed: {other}"),
    }
}

fn http_body_error(error: ureq::Error, url: &str, timeout_seconds: u64) -> anyhow::Error {
    match error {
        ureq::Error::BodyExceedsLimit(_) => anyhow::anyhow!(
            "HTTP GET {url} response exceeded {} MiB",
            MAX_HTTP_RESPONSE_BYTES / (1024 * 1024)
        ),
        other => http_request_error(other, url, timeout_seconds),
    }
}

pub(super) fn sanitize_url(url: &str) -> String {
    let without_query = url.split_once('?').map_or(url, |(prefix, _)| prefix);
    let without_fragment = without_query
        .split_once('#')
        .map_or(without_query, |(prefix, _)| prefix);
    let Some((scheme, remainder)) = without_fragment.split_once("://") else {
        return without_fragment.to_string();
    };
    let authority_end = remainder.find('/').unwrap_or(remainder.len());
    let (authority, path) = remainder.split_at(authority_end);
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{authority}{path}")
}

fn sanitize_uri(uri: &ureq::http::Uri) -> String {
    match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => {
            let authority = authority.as_str();
            let authority = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            format!("{scheme}://{authority}{}", uri.path())
        }
        _ => sanitize_url(&uri.to_string()),
    }
}
