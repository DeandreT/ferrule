use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cli::{
    RestExecutionPolicy, RestJsonError, RestJsonHeader, RestJsonMappingOptions, RestJsonRequest,
};

const GRANT: RestExecutionPolicy = RestExecutionPolicy::AllowSingleRequest {
    allow_insecure_http: true,
};

struct Server {
    url: String,
    thread: JoinHandle<Vec<Vec<u8>>>,
}

impl Server {
    fn new(response: Vec<u8>, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "http://{}/data?opaque=query-secret",
            listener.local_addr().unwrap()
        );
        let thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut requests = Vec::new();
            while Instant::now() < deadline {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("local test accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut raw = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    raw.extend_from_slice(&chunk[..read]);
                    if let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if raw.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(raw);
                thread::sleep(delay);
                let _ = stream.write_all(&response);
            }
            requests
        });
        Self { url, thread }
    }

    fn json(body: &[u8]) -> Self {
        Self::new(
            response("200 OK", "application/json", &[], body),
            Duration::ZERO,
        )
    }

    fn finish(self) -> Vec<Vec<u8>> {
        self.thread.join().unwrap()
    }
}

fn response(status: &str, media: &str, extra: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut raw = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )
    .into_bytes();
    for (name, value) in extra {
        raw.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    raw.extend_from_slice(b"\r\n");
    raw.extend_from_slice(body);
    raw
}

fn phase<T>(result: &Result<T, RestJsonError>) -> &'static str {
    match result {
        Ok(_) => "success",
        Err(error) => error.phase(),
    }
}

fn capture<T>(name: &str, result: &Result<T, RestJsonError>, requests: &[Vec<u8>]) {
    // Original server effects and error observation precede semantic assertions.
    // Numeric arrays are formatted directly to avoid materializing a second
    // JSON Value for every byte of the genuine boundary buffers.
    eprintln!("{{\"case\": {name:?}, \"original_requests\": {requests:?}}}");
    eprintln!(
        "{}",
        serde_json::json!({ "case": name, "phase": phase(result),
        "error": result.as_ref().err().map(ToString::to_string),
        "has_source": result.as_ref().err().is_some_and(|error| error.source().is_some()) })
    );
}

fn project() -> mapping::Project {
    serde_json::from_value(serde_json::json!({
        "source": {"name":"Input","kind":{"kind":"group","children":[
            {"name":"Message","kind":{"kind":"scalar","ty":"string"}},
            {"name":"Number","kind":{"kind":"scalar","ty":"int"}},
            {"name":"SignedZero","kind":{"kind":"scalar","ty":"float"}}
        ]}},
        "target": {"name":"Result","kind":{"kind":"group","children":[
            {"name":"Message","kind":{"kind":"scalar","ty":"string"}},
            {"name":"Number","kind":{"kind":"scalar","ty":"int"}},
            {"name":"SignedZero","kind":{"kind":"scalar","ty":"float"}},
            {"name":"Lazy","kind":{"kind":"scalar","ty":"string"}}
        ]}},
        "graph":{"nodes":{
            "0":{"kind":"source_field","path":["Message"]},
            "1":{"kind":"source_field","path":["Number"]},
            "2":{"kind":"source_field","path":["SignedZero"]},
            "3":{"kind":"const","value":false},
            "4":{"kind":"runtime_parameter","name":"context","ty":"string"},
            "5":{"kind":"if","condition":3,"then":4,"else":0}
        }},
        "root":{"bindings":[{"target_field":"Message","node":0},
            {"target_field":"Number","node":1},{"target_field":"SignedZero","node":2},
            {"target_field":"Lazy","node":5}]}
    }))
    .unwrap()
}

#[test]
fn default_denial_and_invalid_requests_have_no_server_effects() {
    for case in [
        "deny",
        "http",
        "json",
        "utf8",
        "header",
        "duplicate",
        "protocol",
        "large-body",
        "large-header",
        "url-userinfo",
    ] {
        let server = Server::json(b"{}");
        let body = vec![b'a'; cli::MAX_REST_JSON_BODY_BYTES + 1];
        let header_value = "x".repeat(cli::MAX_REST_JSON_HEADER_BYTES);
        let mut url = server.url.clone();
        if case == "url-userinfo" {
            url = url.replacen("http://", "http://secret:password@", 1);
        }
        let headers = match case {
            "header" => vec![RestJsonHeader {
                name: "X-Token",
                value: "token\r\nX-Injected: yes",
            }],
            "duplicate" => vec![
                RestJsonHeader {
                    name: "X-Token",
                    value: "a",
                },
                RestJsonHeader {
                    name: "x-token",
                    value: "b",
                },
            ],
            "protocol" => vec![RestJsonHeader {
                name: "Host",
                value: "other-host",
            }],
            "large-header" => vec![RestJsonHeader {
                name: "X-Token",
                value: &header_value,
            }],
            _ => vec![],
        };
        let request = match case {
            "json" => RestJsonRequest::post(&url, b"{bad:1}"),
            "utf8" => RestJsonRequest::post(&url, b"\xff"),
            "large-body" => RestJsonRequest::post(&url, &body),
            _ => RestJsonRequest::get(&url),
        }
        .with_headers(&headers);
        let policy = match case {
            "deny" => RestExecutionPolicy::default(),
            "http" => RestExecutionPolicy::AllowSingleRequest {
                allow_insecure_http: false,
            },
            _ => GRANT,
        };
        let result = cli::fetch_rest_json(&request, policy);
        let requests = server.finish();
        capture(case, &result, &requests);
        assert!(result.is_err());
        assert!(requests.is_empty());
        assert!(result.unwrap_err().source().is_none());
    }
}

#[test]
fn post_preserves_complete_raw_body_and_explicit_credentials_once() {
    let server = Server::json(b" { \"ok\" : true } ");
    let original = b" { \"question\" : \"a & b\", \"n\" : 9007199254740993 } ";
    let headers = [
        RestJsonHeader {
            name: "Authorization",
            value: "Bearer host-secret",
        },
        RestJsonHeader {
            name: "X-Correlation",
            value: "opaque",
        },
    ];
    let result = cli::fetch_rest_json(
        &RestJsonRequest::post(&server.url, original).with_headers(&headers),
        GRANT,
    );
    let requests = server.finish();
    capture("post", &result, &requests);
    if let Ok(response) = &result {
        eprintln!(
            "{{\"returned\": {:?}, \"status\": {}}}",
            response.bytes(),
            response.status()
        );
    }
    assert_eq!(requests.len(), 1);
    let raw = &requests[0];
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(&raw[end..], original);
    let header = String::from_utf8(raw[..end].to_vec())
        .unwrap()
        .to_ascii_lowercase();
    assert!(header.starts_with("post /data?opaque=query-secret http/1.1\r\n"));
    assert!(header.contains("authorization: bearer host-secret\r\n"));
    assert!(header.contains("accept-encoding: identity\r\n"));
    let response = result.unwrap();
    assert_eq!(response.bytes(), b" { \"ok\" : true } ");
    assert!(!format!("{response:?}").contains("ok"));
}

#[test]
fn responses_refuse_status_encoding_media_utf8_json_and_headers_without_retry() {
    let redirect_destination = Server::json(b"{}");
    let mut cases = vec![
        (
            "status",
            response("503 Unavailable", "application/json", &[], b"{}"),
            "http-status",
        ),
        (
            "redirect",
            response(
                "302 Found",
                "application/json",
                &[("Location", &redirect_destination.url)],
                b"{}",
            ),
            "http-status",
        ),
        (
            "gzip",
            response(
                "200 OK",
                "application/json",
                &[("Content-Encoding", "gzip")],
                b"{}",
            ),
            "response-metadata",
        ),
        (
            "media",
            response("200 OK", "text/json", &[], b"{}"),
            "response-metadata",
        ),
        (
            "charset",
            response("200 OK", "application/json; charset=iso-8859-1", &[], b"{}"),
            "response-metadata",
        ),
        (
            "utf8",
            response("200 OK", "application/json", &[], b"\xff"),
            "response-utf8",
        ),
        (
            "json",
            response("200 OK", "application/json", &[], b"{a:1}"),
            "response-json",
        ),
        (
            "empty",
            response("204 No Content", "application/json", &[], b""),
            "response-json",
        ),
        ("disconnect", vec![], "transport"),
    ];
    let large = "x".repeat(cli::MAX_REST_JSON_HEADER_BYTES);
    cases.push((
        "headers",
        response("200 OK", "application/json", &[("X-Large", &large)], b"{}"),
        "response-metadata",
    ));
    for (name, raw, expected) in cases {
        let server = Server::new(raw, Duration::ZERO);
        let request = if name == "disconnect" {
            RestJsonRequest::post(&server.url, b"{\"effect\":true}")
        } else {
            RestJsonRequest::get(&server.url)
        };
        let result = cli::fetch_rest_json(&request, GRANT);
        let requests = server.finish();
        capture(name, &result, &requests);
        assert_eq!(phase(&result), expected);
        assert_eq!(requests.len(), 1);
        let error = result.unwrap_err();
        assert!(error.source().is_none());
        let display = format!("{error:?} {error}");
        assert!(!display.contains("query-secret"));
        assert!(!display.contains("127.0.0.1"));
    }
    assert!(redirect_destination.finish().is_empty());
}

#[test]
fn exact_body_limit_and_one_byte_overflow_work_with_and_without_length() {
    let exact = format!("\"{}\"", "a".repeat(cli::MAX_REST_JSON_BODY_BYTES - 2)).into_bytes();
    let server = Server::json(&exact);
    let result = cli::fetch_rest_json(&RestJsonRequest::get(&server.url), GRANT);
    let requests = server.finish();
    capture("exact-limit", &result, &requests);
    if let Ok(response) = &result {
        eprintln!("{{\"returned\": {:?}}}", response.bytes());
    }
    assert_eq!(result.unwrap().bytes(), exact);
    assert_eq!(requests.len(), 1);
    let over = vec![b' '; cli::MAX_REST_JSON_BODY_BYTES + 1];
    let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", over.len()).into_bytes();
    raw.extend_from_slice(&over);
    raw.extend_from_slice(b"\r\n0\r\n\r\n");
    for (name, raw) in [
        (
            "declared-over",
            response("200 OK", "application/json", &[], &over),
        ),
        ("chunked-over", raw),
    ] {
        let server = Server::new(raw, Duration::ZERO);
        let result = cli::fetch_rest_json(&RestJsonRequest::get(&server.url), GRANT);
        let requests = server.finish();
        capture(name, &result, &requests);
        assert_eq!(phase(&result), "response-limit");
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn exact_request_body_and_host_header_count_are_enforced_by_server_effects() {
    let original = format!("\"{}\"", "a".repeat(cli::MAX_REST_JSON_BODY_BYTES - 2)).into_bytes();
    let server = Server::json(b"{}");
    let result = cli::fetch_rest_json(&RestJsonRequest::post(&server.url, &original), GRANT);
    let requests = server.finish();
    capture("exact-request-body", &result, &requests);
    assert!(result.is_ok());
    assert_eq!(requests.len(), 1);
    let raw = &requests[0];
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(&raw[end..], original);
    for count in [64, 65] {
        let names = (0..count)
            .map(|index| format!("X-Host-{index}"))
            .collect::<Vec<_>>();
        let headers = names
            .iter()
            .map(|name| RestJsonHeader {
                name,
                value: "opaque",
            })
            .collect::<Vec<_>>();
        let server = Server::json(b"{}");
        let result = cli::fetch_rest_json(
            &RestJsonRequest::get(&server.url).with_headers(&headers),
            GRANT,
        );
        let requests = server.finish();
        capture("header-count", &result, &requests);
        assert_eq!(result.is_ok(), count == 64);
        assert_eq!(requests.len(), usize::from(count == 64));
    }
}

#[test]
fn json_suffix_media_and_explicit_host_parameters_keep_native_context_behavior() {
    let body = br#"{"Message":"ordinary","Number":1,"SignedZero":-0.0}"#;
    let server = Server::new(
        response(
            "200 OK",
            "application/problem+json; charset=\"UTF-8\"",
            &[],
            body,
        ),
        Duration::ZERO,
    );
    let mut project = project();
    project.graph.nodes.insert(
        3,
        mapping::Node::Const {
            value: ir::Value::Bool(true),
        },
    );
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert(
            "context",
            ir::Value::String("explicit host value".to_string()),
        )
        .unwrap();
    let mut options =
        RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json"));
    options.runtime_parameters = Some(&parameters);
    let result = cli::run_project_value_rest_json_payloads(
        &project,
        Path::new("mapping.json"),
        &RestJsonRequest::get(&server.url),
        GRANT,
        &options,
    );
    let requests = server.finish();
    capture("host-context", &result, &requests);
    if let Ok(outcome) = &result {
        eprintln!(
            "{}",
            serde_json::json!({"artifacts":outcome.artifacts.iter().map(|a| &a.bytes).collect::<Vec<_>>()})
        );
    }
    let outcome = result.unwrap();
    assert_eq!(requests.len(), 1);
    let value: serde_json::Value = serde_json::from_slice(&outcome.artifacts[0].bytes).unwrap();
    assert_eq!(value["Lazy"], "explicit host value");
}

#[test]
fn request_timeout_is_typed_and_does_not_retry_after_post() {
    let server = Server::new(
        response("200 OK", "application/json", &[], b"{}"),
        Duration::from_millis(1400),
    );
    let request = RestJsonRequest::post(&server.url, b"{\"effect\":true}")
        .with_timeout(mapping::HttpTimeoutSeconds::new(1).unwrap());
    let result = cli::fetch_rest_json(&request, GRANT);
    let requests = server.finish();
    capture("timeout", &result, &requests);
    assert_eq!(phase(&result), "timeout");
    assert_eq!(requests.len(), 1);
}

#[test]
fn response_mapping_equals_offline_payloads_and_keeps_lazy_context_and_numeric_bits() {
    let original = br#"{"Message":"A <&> \u00e9","Number":9007199254740993,"SignedZero":-0.0}"#;
    let server = Server::json(original);
    let project = project();
    let source = Path::new("response.json");
    let target = Path::new("result.json");
    let options = RestJsonMappingOptions::new(source, target);
    let result = cli::run_project_value_rest_json_payloads(
        &project,
        Path::new("mapping.json"),
        &RestJsonRequest::get(&server.url),
        GRANT,
        &options,
    );
    let requests = server.finish();
    capture("mapped", &result, &requests);
    if let Ok(outcome) = &result {
        eprintln!(
            "{}",
            serde_json::json!({"artifacts":outcome.artifacts.iter().map(|a| &a.bytes).collect::<Vec<_>>()})
        );
    }
    let outcome = result.unwrap();
    let offline = cli::run_project_value_payloads(
        &project,
        Path::new("mapping.json"),
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(source, original).unwrap())
            .with_output_path(target),
    )
    .unwrap();
    assert_eq!(outcome, offline);
    assert_eq!(requests.len(), 1);
    let value: serde_json::Value = serde_json::from_slice(&outcome.artifacts[0].bytes).unwrap();
    assert_eq!(value["Number"].as_i64(), Some(9007199254740993));
    assert_eq!(
        value["SignedZero"].as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(value["Lazy"], value["Message"]);
}

#[test]
fn preflight_and_cancellation_refuse_before_effects_and_native_mapping_errors_keep_causes() {
    for case in [
        "json5",
        "json-lines",
        "xml-input",
        "xml-output",
        "named",
        "named-input",
        "dynamic",
        "logical",
        "cancel",
        "mapping",
    ] {
        let server = Server::json(br#"{"Message":"ordinary","Number":1,"SignedZero":0.0}"#);
        let mut project = project();
        if case == "json5" {
            project.source_options.json5 = true;
        }
        if case == "json-lines" {
            project.source_options.json_lines = true;
        }
        if case == "xml-input" {
            project.source_options.xml_document = true;
        }
        if case == "xml-output" {
            project.target_options.xml_document = true;
        }
        if case == "named" {
            project.extra_targets.push(mapping::NamedTarget {
                name: "Other".to_string(),
                schema: project.target.clone(),
                root: project.root.clone(),
                path: None,
                options: mapping::FormatOptions::default(),
            });
        }
        if case == "named-input" {
            project.extra_sources.push(mapping::NamedSource {
                name: "Catalog".to_string(),
                path: "never-read.json".to_string(),
                schema: project.source.clone(),
                options: mapping::FormatOptions::default(),
                dynamic_path: None,
            });
        }
        if case == "dynamic" {
            project.source.repeating = true;
            project.root.iteration = mapping::ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path: 0,
            };
        }
        if case == "mapping" {
            project.graph.nodes.insert(
                3,
                mapping::Node::Const {
                    value: ir::Value::Bool(true),
                },
            );
        }
        let cancel = || false;
        let mut options = RestJsonMappingOptions::new(
            Path::new("response.json"),
            if case == "logical" {
                Path::new("result.xml")
            } else {
                Path::new("result.json")
            },
        );
        if case == "cancel" {
            options.should_continue = Some(&cancel);
        }
        let result = cli::run_project_value_rest_json_payloads(
            &project,
            Path::new("mapping.json"),
            &RestJsonRequest::get(&server.url),
            GRANT,
            &options,
        );
        let requests = server.finish();
        capture(case, &result, &requests);
        if case == "mapping" {
            assert_eq!(phase(&result), "mapping");
            assert_eq!(requests.len(), 1);
            let error = result.unwrap_err();
            assert!(error.source().is_some());
            if let RestJsonError::Mapping(error) = error {
                assert!(error.chain().any(|cause| cause.is::<engine::EngineError>()));
            } else {
                panic!("missing native mapping cause");
            }
        } else {
            assert_eq!(
                phase(&result),
                if case == "cancel" {
                    "cancelled"
                } else {
                    "request-policy"
                }
            );
            assert!(requests.is_empty());
        }
    }
}

#[test]
fn cancellation_after_response_returns_no_artifacts_and_does_not_repeat_remote_effect() {
    let server = Server::json(br#"{"Message":"ordinary","Number":1,"SignedZero":0.0}"#);
    let calls = std::cell::Cell::new(0);
    let gate = || {
        let current = calls.get();
        calls.set(current + 1);
        current == 0
    };
    let mut options =
        RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json"));
    options.should_continue = Some(&gate);
    let result = cli::run_project_value_rest_json_payloads(
        &project(),
        Path::new("mapping.json"),
        &RestJsonRequest::get(&server.url),
        GRANT,
        &options,
    );
    let requests = server.finish();
    capture("post-response-cancel", &result, &requests);
    assert_eq!(phase(&result), "cancelled");
    assert_eq!(requests.len(), 1);
}

#[test]
fn native_writer_failure_keeps_its_cause_and_returns_no_result() {
    let server = Server::json(br#"{"Message":"ordinary","Number":1,"SignedZero":0.0}"#);
    let mut project = project();
    project.graph.nodes.insert(
        2,
        mapping::Node::Const {
            value: ir::Value::Float(f64::NAN),
        },
    );
    let result = cli::run_project_value_rest_json_payloads(
        &project,
        Path::new("mapping.json"),
        &RestJsonRequest::get(&server.url),
        GRANT,
        &RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json")),
    );
    let requests = server.finish();
    capture("native-writer", &result, &requests);
    assert_eq!(phase(&result), "mapping");
    assert_eq!(requests.len(), 1);
    if let RestJsonError::Mapping(error) = result.unwrap_err() {
        assert!(
            error
                .chain()
                .any(|cause| cause.is::<format_json::JsonFormatError>())
        );
    } else {
        panic!("missing native writer cause");
    }
}

#[test]
fn captured_response_metadata_never_selects_the_url_or_method() {
    let body = br#"{"Message":"ordinary","Number":1,"SignedZero":0.0}"#;
    let selected = Server::json(body);
    let stored = Server::json(b"{}");
    let mut project = project();
    project.source_path = Some(stored.url.clone());
    project.source_options.external_source = Some(
        mapping::ExternalSourceOptions::http_post(
            mapping::ExternalHttpMode::Manual,
            mapping::HttpTimeoutSeconds::default(),
            None,
            None,
            mapping::ExternalPayloadFormat::Json,
            Vec::new(),
        )
        .unwrap(),
    );
    let options = RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json"));
    let result = cli::run_project_value_rest_json_payloads(
        &project,
        Path::new("mapping.json"),
        &RestJsonRequest::get(&selected.url),
        GRANT,
        &options,
    );
    let requests = selected.finish();
    let stored_requests = stored.finish();
    capture("captured-metadata", &result, &requests);
    eprintln!(
        "{}",
        serde_json::json!({"stored_origin_requests":stored_requests})
    );
    if let Ok(outcome) = &result {
        eprintln!(
            "{}",
            serde_json::json!({"artifacts":outcome.artifacts.iter().map(|a| &a.bytes).collect::<Vec<_>>()})
        );
    }
    let outcome = result.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(stored_requests.is_empty());
    assert!(requests[0].starts_with(b"GET "));
    let offline = cli::run_project_value_payloads(
        &project,
        Path::new("mapping.json"),
        &cli::PayloadRunOptions::new(
            cli::PayloadDocument::new(Path::new("response.json"), body).unwrap(),
        )
        .with_output_path(Path::new("result.json")),
    )
    .unwrap();
    assert_eq!(outcome, offline);
}

#[test]
fn exact_url_byte_boundary_keeps_the_whole_request_target_and_overflow_has_no_effect() {
    const LIMIT: usize = 4096;
    let body = b"{\"boundary\":\"url\"}";
    for (name, bytes) in [("url-4096", LIMIT), ("url-4097", LIMIT + 1)] {
        let server = Server::json(body);
        let prefix = format!("{}&padding=", server.url);
        let padding = "u".repeat(bytes - prefix.len());
        let url = format!("{prefix}{padding}");
        let result = cli::fetch_rest_json(&RestJsonRequest::get(&url), GRANT);
        let requests = server.finish();
        capture(name, &result, &requests);
        eprintln!(
            "{}",
            serde_json::json!({
                "case": name, "original_url": url, "original_url_utf8_bytes": url.len(),
                "returned_body": result.as_ref().ok().map(|response| response.bytes()),
                "returned_status": result.as_ref().ok().map(|response| response.status()),
                "returned_error_debug": result.as_ref().err().map(|error| format!("{error:?}")),
            })
        );

        assert_eq!(url.len(), bytes);
        if bytes == LIMIT {
            let response = result.unwrap();
            assert_eq!(response.bytes(), body);
            assert_eq!(response.status(), 200);
            assert_eq!(requests.len(), 1);
            let raw = &requests[0];
            let end = raw
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap()
                + 4;
            assert_eq!(
                end,
                raw.len(),
                "GET retains a complete header block without a body"
            );
            let request = std::str::from_utf8(raw).unwrap();
            assert_eq!(
                request.split("\r\n").next().unwrap(),
                format!("GET /data?opaque=query-secret&padding={padding} HTTP/1.1")
            );
        } else {
            assert!(matches!(result, Err(RestJsonError::RequestLimit { limit }) if limit == LIMIT));
            assert!(
                requests.is_empty(),
                "the oversized URL must be refused before any server effect"
            );
        }
    }
}

#[test]
fn exact_conservative_header_ledger_keeps_the_whole_value_and_overflow_has_no_effect() {
    const LIMIT: usize = 64 * 1024;
    const NAME: &str = "X-Boundary-Witness";
    let body = b"{\"boundary\":\"header\"}";
    for extra in [0, 1] {
        let name = if extra == 0 {
            "header-ledger-65536"
        } else {
            "header-ledger-65537"
        };
        let server = Server::json(body);
        let url = server.url.clone();
        // This is the published conservative admission ledger, including its
        // fixed request/protocol allowance. It is not the serialized HTTP size.
        let reserved = 2 * url.len() + 512 + NAME.len() + 4;
        let value = "h".repeat(LIMIT - reserved + extra);
        let headers = [RestJsonHeader {
            name: NAME,
            value: &value,
        }];
        let result =
            cli::fetch_rest_json(&RestJsonRequest::get(&url).with_headers(&headers), GRANT);
        let requests = server.finish();
        capture(name, &result, &requests);
        eprintln!(
            "{}",
            serde_json::json!({
                "case": name, "original_url": url, "original_header_name": NAME,
                "original_header_value": value, "original_header_value_utf8_bytes": value.len(),
                "conservative_admission_ledger_bytes": reserved + value.len(),
                "returned_body": result.as_ref().ok().map(|response| response.bytes()),
                "returned_status": result.as_ref().ok().map(|response| response.status()),
                "returned_error_debug": result.as_ref().err().map(|error| format!("{error:?}")),
            })
        );

        assert_eq!(reserved + value.len(), LIMIT + extra);
        if extra == 0 {
            let response = result.unwrap();
            assert_eq!(response.bytes(), body);
            assert_eq!(response.status(), 200);
            assert_eq!(requests.len(), 1);
            let raw = &requests[0];
            let end = raw
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap()
                + 4;
            assert_eq!(
                end,
                raw.len(),
                "GET retains a complete header block without a body"
            );
            let request = std::str::from_utf8(raw).unwrap();
            assert_eq!(
                request.split("\r\n").next().unwrap(),
                "GET /data?opaque=query-secret HTTP/1.1"
            );
            let fields = request
                .split("\r\n")
                .filter_map(|line| {
                    let (name, content) = line.split_once(':')?;
                    name.eq_ignore_ascii_case(NAME).then_some(content)
                })
                .collect::<Vec<_>>();
            assert_eq!(fields.len(), 1);
            // Ignore only protocol OWS; all original bytes remain in capture.
            assert_eq!(fields[0].trim_matches([' ', '\t']), value);
        } else {
            assert!(matches!(result, Err(RestJsonError::RequestLimit { limit }) if limit == LIMIT));
            assert!(
                requests.is_empty(),
                "the oversized ledger must be refused before any server effect"
            );
        }
    }
}
