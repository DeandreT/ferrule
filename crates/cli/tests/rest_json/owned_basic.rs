use super::*;

fn basic_project(collection: bool) -> mapping::Project {
    let mut project = project();
    project.graph.nodes.insert(
        0,
        mapping::Node::OwnedSourceField(mapping::OwnedSourceField {
            source: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: vec!["Message".into()],
            },
            frame: collection.then(Vec::new),
        }),
    );
    if collection {
        project.source.repeating = true;
        project.target.repeating = true;
        project.root.iteration = mapping::ScopeIteration::OwnedSource {
            source: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: Vec::new(),
            },
            output_path: None,
        };
    }
    project
}

#[test]
fn basic_owned_field_and_collection_map_actual_rest_responses() {
    for collection in [false, true] {
        let (body, expected): (&[u8], _) = if collection {
            (
                br#"[{"Message":"first","Number":9007199254740993,"SignedZero":0.0},{"Message":"second","Number":-2,"SignedZero":1.5}]"#,
                serde_json::json!([
                    {"Message":"first","Number":9007199254740993_i64,"SignedZero":0.0,"Lazy":"first"},
                    {"Message":"second","Number":-2,"SignedZero":1.5,"Lazy":"second"}
                ]),
            )
        } else {
            (
                br#"{"Message":"owned \u65e5\u672c","Number":9007199254740993,"SignedZero":0.0}"#,
                serde_json::json!({"Message":"owned 日本","Number":9007199254740993_i64,"SignedZero":0.0,"Lazy":"owned 日本"}),
            )
        };
        let server = Server::json(body);
        let options =
            RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json"));
        let result = cli::run_project_value_rest_json_payloads(
            &basic_project(collection),
            Path::new("mapping.json"),
            &RestJsonRequest::get(&server.url),
            GRANT,
            &options,
        );
        let requests = server.finish();
        capture("basic-owned-success", &result, &requests);
        if let Ok(outcome) = &result {
            eprintln!("complete Basic REST output: {outcome:?}");
        }
        assert_eq!(requests.len(), 1);
        let outcome = result.unwrap();
        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(outcome.records_written, if collection { 2 } else { 1 });
        let actual: serde_json::Value =
            serde_json::from_slice(&outcome.artifacts[0].bytes).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn basic_owned_rest_response_syntax_errors_are_not_admission_refusals() {
    for collection in [false, true] {
        let server = Server::json(b"not JSON");
        let options =
            RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json"));
        let result = cli::run_project_value_rest_json_payloads(
            &basic_project(collection),
            Path::new("mapping.json"),
            &RestJsonRequest::get(&server.url),
            GRANT,
            &options,
        );
        let requests = server.finish();
        capture("basic-owned-response-syntax", &result, &requests);
        assert_eq!(requests.len(), 1);
        assert_eq!(phase(&result), "response-json");
        assert!(result.unwrap_err().source().is_none());
    }
}

#[test]
fn basic_owned_observer_refusal_precedes_rest_connection() {
    struct Trace(std::cell::Cell<usize>);
    impl engine::TraceSink for Trace {
        fn record(&self, _: engine::TraceEvent) {
            self.0.set(self.0.get() + 1);
        }
    }
    for collection in [false, true] {
        let server = Server::json(b"not JSON");
        let trace = Trace(std::cell::Cell::new(0));
        let mut options =
            RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json"));
        options.trace_sink = Some(&trace);
        let result = cli::run_project_value_rest_json_payloads(
            &basic_project(collection),
            Path::new("mapping.json"),
            &RestJsonRequest::get(&server.url),
            GRANT,
            &options,
        );
        let requests = server.finish();
        capture("basic-owned-observer-preflight", &result, &requests);
        assert!(requests.is_empty());
        assert_eq!(trace.0.get(), 0);
        let RestJsonError::Mapping(error) = result.unwrap_err() else {
            panic!("expected typed observer refusal");
        };
        assert_eq!(
            error.downcast_ref::<engine::EngineError>(),
            Some(&engine::EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: None,
                context: engine::OwnedCapabilityContext::Tracing,
            })
        );
    }
}
