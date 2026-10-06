use std::path::Path;

use super::{RestExecutionPolicy, RestJsonError, RestJsonHeader, RestJsonRequest, fetch_rest_json};
use crate::{PayloadDocument, PayloadRunOptions, PayloadRunOutcome, run_project_value_payloads};

/// Logical JSON identities and existing mapping context. The synchronous
/// request cannot be interrupted mid-call; its configured timeout still applies.
pub struct RestJsonMappingOptions<'a> {
    pub response_path: &'a Path,
    pub output_path: &'a Path,
    pub runtime_parameters: Option<&'a engine::RuntimeParameters>,
    pub trace_sink: Option<&'a dyn engine::TraceSink>,
    pub debug_hook: Option<&'a dyn engine::DebugHook>,
    pub should_continue: Option<&'a dyn Fn() -> bool>,
}

impl<'a> RestJsonMappingOptions<'a> {
    pub fn new(response_path: &'a Path, output_path: &'a Path) -> Self {
        Self {
            response_path,
            output_path,
            runtime_parameters: None,
            trace_sink: None,
            debug_hook: None,
            should_continue: None,
        }
    }
}

fn continue_run(options: &RestJsonMappingOptions<'_>) -> Result<(), RestJsonError> {
    if options.should_continue.is_some_and(|gate| !gate()) {
        return Err(RestJsonError::Cancelled);
    }
    Ok(())
}

fn json_options(options: &::mapping::FormatOptions, input: bool) -> bool {
    let mut remaining = options.clone();
    remaining.json_document = false;
    remaining.json_schema_unresolved_reference = None;
    remaining.mfd_decimal_input_names.clear();
    if input {
        if remaining
            .external_source
            .as_ref()
            .is_some_and(|source| source.payload() != ::mapping::ExternalPayloadFormat::Json)
        {
            return false;
        }
        remaining.external_source = None;
    }
    remaining == ::mapping::FormatOptions::default()
}

fn has_document_list(scope: &::mapping::Scope) -> bool {
    scope.output_path().is_some() || scope.children.iter().any(has_document_list)
}

fn preflight(
    project: &::mapping::Project,
    options: &RestJsonMappingOptions<'_>,
) -> Result<(), RestJsonError> {
    for path in [options.response_path, options.output_path] {
        crate::payload::validate_logical_path(path, "REST JSON identity")
            .map_err(|_| RestJsonError::RequestPolicy("invalid logical JSON identity"))?;
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            return Err(RestJsonError::RequestPolicy(
                "logical input and output identities must use .json",
            ));
        }
    }
    if !project.extra_sources.is_empty()
        || !project.extra_targets.is_empty()
        || has_document_list(&project.root)
        || !json_options(&project.source_options, true)
        || !json_options(&project.target_options, false)
    {
        return Err(RestJsonError::RequestPolicy(
            "this route requires one strict JSON input and one JSON output",
        ));
    }
    crate::require_valid(project).map_err(RestJsonError::Mapping)
}

/// Preflights one immutable project before any connection, fetches once, and
/// delegates its complete response to the unchanged payload mapping API.
/// Returns no output value on any failure and never writes configured paths.
pub fn run_project_value_rest_json_payloads(
    project: &::mapping::Project,
    project_path: &Path,
    request: &RestJsonRequest<'_>,
    policy: RestExecutionPolicy,
    options: &RestJsonMappingOptions<'_>,
) -> Result<PayloadRunOutcome, RestJsonError> {
    if policy == RestExecutionPolicy::Deny {
        return Err(RestJsonError::Disabled);
    }
    preflight(project, options)?;
    continue_run(options)?;
    let response = fetch_rest_json(request, policy)?;
    continue_run(options)?;
    let primary = PayloadDocument::new(options.response_path, response.bytes())
        .map_err(RestJsonError::Mapping)?;
    let mut payloads = PayloadRunOptions::new(primary).with_output_path(options.output_path);
    if let Some(parameters) = options.runtime_parameters {
        payloads = payloads.with_runtime_parameters(parameters);
    }
    if let Some(trace) = options.trace_sink {
        payloads = payloads.with_trace_sink(trace);
    }
    if let Some(debug) = options.debug_hook {
        payloads = payloads.with_debug_hook(debug);
    }
    let outcome = run_project_value_payloads(project, project_path, &payloads)
        .map_err(RestJsonError::Mapping)?;
    continue_run(options)?;
    if outcome.artifacts.len() != 1 {
        return Err(RestJsonError::RequestPolicy(
            "mapping returned an unexpected artifact count",
        ));
    }
    Ok(outcome)
}

/// CLI companion. The request description and header-values file are host
/// inputs, never project metadata. It loads the project once before transport.
/// Description keys: url, method (GET/POST), optional body_file and
/// timeout_seconds. Header file: [{"name":"Authorization","value":"..."}].
pub fn run_project_rest_json_request_file_payloads(
    project_path: &Path,
    request_path: &Path,
    header_values_path: Option<&Path>,
    policy: RestExecutionPolicy,
    options: &RestJsonMappingOptions<'_>,
) -> Result<PayloadRunOutcome, RestJsonError> {
    if policy == RestExecutionPolicy::Deny {
        return Err(RestJsonError::Disabled);
    }
    let project = crate::load_project(project_path).map_err(RestJsonError::Mapping)?;
    run_project_value_rest_json_request_file_payloads(
        &project,
        project_path,
        request_path,
        header_values_path,
        policy,
        options,
    )
}

/// Host-owned request files applied to one already-loaded mapping snapshot.
/// The files use the same bounded parser as the saved-project companion.
/// Neither request/header values nor remote response data are saved in the project.
pub fn run_project_value_rest_json_request_file_payloads(
    project: &::mapping::Project,
    project_path: &Path,
    request_path: &Path,
    header_values_path: Option<&Path>,
    policy: RestExecutionPolicy,
    options: &RestJsonMappingOptions<'_>,
) -> Result<PayloadRunOutcome, RestJsonError> {
    if policy == RestExecutionPolicy::Deny {
        return Err(RestJsonError::Disabled);
    }
    preflight(project, options)?;
    let bytes = super::read_host_file(request_path, super::MAX_REST_JSON_HEADER_BYTES)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| RestJsonError::RequestPolicy("invalid request description"))?;
    let object = value
        .as_object()
        .ok_or(RestJsonError::RequestPolicy("invalid request description"))?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "url" | "method" | "body_file" | "timeout_seconds"
        )
    }) {
        return Err(RestJsonError::RequestPolicy(
            "unknown request description field",
        ));
    }
    let url =
        object
            .get("url")
            .and_then(|value| value.as_str())
            .ok_or(RestJsonError::RequestPolicy(
                "request description requires an endpoint",
            ))?;
    let method = object
        .get("method")
        .and_then(|value| value.as_str())
        .ok_or(RestJsonError::RequestPolicy(
            "request description requires GET or POST",
        ))?;
    let timeout = match object.get("timeout_seconds") {
        None => ::mapping::HttpTimeoutSeconds::default(),
        Some(value) => value
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .and_then(::mapping::HttpTimeoutSeconds::new)
            .ok_or(RestJsonError::RequestPolicy(
                "timeout must be 1..300 seconds",
            ))?,
    };
    let body = match object.get("body_file") {
        None => None,
        Some(value) => {
            let path = value.as_str().filter(|path| !path.is_empty()).ok_or(
                RestJsonError::RequestPolicy("body_file must name a host file"),
            )?;
            let path = request_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(path);
            Some(super::read_host_file(
                &path,
                super::MAX_REST_JSON_BODY_BYTES,
            )?)
        }
    };
    let header_bytes = header_values_path
        .map(|path| super::read_host_file(path, super::MAX_REST_JSON_HEADER_BYTES))
        .transpose()?;
    let header_value = header_bytes
        .as_ref()
        .map(|bytes| {
            serde_json::from_slice::<serde_json::Value>(bytes)
                .map_err(|_| RestJsonError::RequestPolicy("invalid header-values file"))
        })
        .transpose()?;
    let mut headers = Vec::new();
    if let Some(value) = &header_value {
        let array = value.as_array().ok_or(RestJsonError::RequestPolicy(
            "header-values file requires an array",
        ))?;
        if array.len() > super::MAX_REST_JSON_HEADERS {
            return Err(RestJsonError::RequestPolicy("too many host headers"));
        }
        for value in array {
            let object = value
                .as_object()
                .filter(|object| object.len() == 2)
                .ok_or(RestJsonError::RequestPolicy("invalid header-values entry"))?;
            let name = object
                .get("name")
                .and_then(|value| value.as_str())
                .ok_or(RestJsonError::RequestPolicy("invalid header-values entry"))?;
            let value = object
                .get("value")
                .and_then(|value| value.as_str())
                .ok_or(RestJsonError::RequestPolicy("invalid header-values entry"))?;
            headers.push(RestJsonHeader { name, value });
        }
    }
    let request = match (method, body.as_deref()) {
        ("GET", None) => RestJsonRequest::get(url),
        ("POST", Some(body)) => RestJsonRequest::post(url, body),
        _ => {
            return Err(RestJsonError::RequestPolicy(
                "GET forbids a body; POST requires one",
            ));
        }
    }
    .with_timeout(timeout)
    .with_headers(&headers);
    run_project_value_rest_json_payloads(project, project_path, &request, policy, options)
}
