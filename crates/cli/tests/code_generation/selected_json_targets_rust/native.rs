//! Test-only JSON bridge; native selected execution remains the public engine.
include!("capture.rs.txt");
use mapping::Project;
use std::{cell::Cell, sync::Arc};

fn runtime_boundary(at: &engine::FilterMapBoundary) -> FilterMapBoundary {
    FilterMapBoundary {
        item: at.item,
        phase: match at.phase {
            engine::FilterMapPhase::Source => FilterMapPhase::Source,
            engine::FilterMapPhase::Capture => FilterMapPhase::Capture,
            engine::FilterMapPhase::Predicate => FilterMapPhase::Predicate,
            engine::FilterMapPhase::Mapper => FilterMapPhase::Mapper,
        },
        capture_index: at.capture_index,
        source_position: at.source_position,
        function: at.function.map(mapping::FunctionId::get),
        node: at.node,
        kind: match at.kind {
            engine::FilterMapBoundaryKind::SourceReservation => {
                FilterMapBoundaryKind::SourceReservation
            }
            engine::FilterMapBoundaryKind::NodeEvaluation => FilterMapBoundaryKind::NodeEvaluation,
            engine::FilterMapBoundaryKind::CallEntry => FilterMapBoundaryKind::CallEntry,
            engine::FilterMapBoundaryKind::StageItem => FilterMapBoundaryKind::StageItem,
            engine::FilterMapBoundaryKind::Result => FilterMapBoundaryKind::Result,
        },
    }
}
struct NativeChecker<'a>(&'a Checker);
impl engine::FilterMapCancellation for NativeChecker<'_> {
    fn is_cancelled(&self, at: &engine::FilterMapBoundary) -> bool {
        self.0.is_cancelled(&runtime_boundary(at))
    }
}
fn engine_original(error: &engine::EngineError, original: &mut String) {
    writeln!(original, "complete-engine-error={error:#?};display={error}").unwrap();
    match error {
        engine::EngineError::FilterMapRuntime { source, .. } => engine_original(source, original),
        engine::EngineError::FilterMapValueType {
            found: Value::Float(value),
            ..
        } => writeln!(original, "found.binary64={:016x}", value.to_bits()).unwrap(),
        engine::EngineError::FilterMapNonFinite { bits } => {
            writeln!(original, "binary64={bits:016x}").unwrap()
        }
        _ => {}
    }
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        writeln!(original, "source-debug={error:?};source-display={error}").unwrap();
        source = error.source();
    }
}
fn normalize(error: engine::EngineError) -> JsonBoundaryError {
    let runtime = match error {
        engine::EngineError::UnknownTarget { name } => RuntimeError::UnknownTarget { name },
        engine::EngineError::MappingException { node, message } => {
            RuntimeError::MappingException { node, message }
        }
        engine::EngineError::MappingFailure { rule, message } => {
            RuntimeError::MappingFailure { rule, message }
        }
        engine::EngineError::MissingRuntimeParameter { node, name } => {
            RuntimeError::MissingRuntimeParameter { node, name }
        }
        engine::EngineError::MissingDynamicSourceLoader { source_name: name } => {
            RuntimeError::MissingDynamicSourceLoader {
                source: source_name(&name),
            }
        }
        engine::EngineError::DynamicSourceLoad {
            source_name: name,
            path,
            message,
        } => RuntimeError::DynamicSourceLoad {
            source: source_name(&name),
            path,
            message,
        },
        engine::EngineError::FilterMapRuntime { boundary, source } => {
            let cause = normalize(*source);
            let JsonBoundaryError::Execution(source) = cause else {
                return cause;
            };
            RuntimeError::FilterMapRuntime {
                boundary: runtime_boundary(&boundary),
                source: Box::new(source),
            }
        }
        engine::EngineError::FilterMapBudget {
            kind,
            used,
            requested,
            max,
        } => RuntimeError::FilterMapBudget {
            kind: match kind {
                engine::FilterMapBudgetKind::SourceItems => {
                    codegen_runtime::FilterMapBudgetKind::SourceItems
                }
                engine::FilterMapBudgetKind::Work => codegen_runtime::FilterMapBudgetKind::Work,
            },
            used,
            requested,
            max,
        },
        engine::EngineError::FilterMapCancelled => RuntimeError::FilterMapCancelled,
        // Complete unexpected EngineError was already retained before normalization.
        error => {
            return JsonBoundaryError::InvalidOutput {
                message: format!("unhandled native test normalization: {error:#?}"),
            };
        }
    };
    JsonBoundaryError::Execution(runtime)
}
struct NativeLoader<'a> {
    host: &'a Loader,
    project: &'a Project,
    total: Cell<usize>,
    directory: &'a Path,
    prefix: &'a str,
}
impl engine::DynamicSourceLoader for NativeLoader<'_> {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        let document = codegen_runtime::DynamicJsonSourceLoader::load(self.host, source, path)?;
        if document.len() > codegen_runtime::MAX_DYNAMIC_SOURCE_BYTES {
            return Err("document exceeds the 67108864-byte dynamic-source limit".into());
        }
        let total = self
            .total
            .get()
            .checked_add(document.len())
            .ok_or("dynamic-source byte count overflow")?;
        if total > codegen_runtime::MAX_DYNAMIC_SOURCE_TOTAL_BYTES {
            return Err("documents exceed the 268435456-byte combined dynamic-source limit".into());
        }
        self.total.set(total);
        let text = std::str::from_utf8(&document)
            .map_err(|error| format!("document is not UTF-8: {error}"))?;
        let declared = self
            .project
            .extra_sources
            .iter()
            .find(|input| input.name == source && input.dynamic_path.is_some())
            .ok_or("undeclared dynamic source")?;
        let schema = serde_json::to_string(&declared.schema).map_err(|error| error.to_string())?;
        let parsed = codegen_runtime::parse_json(&schema, text);
        let mut original = String::new();
        match &parsed {
            Ok(value) => instance_original(value, "native-loader", &mut original),
            Err(error) => error_original(error, &mut original),
        }
        retain(
            self.directory,
            &format!(
                "{}-NATIVE-LOADER-{:02}-COMPLETE-PARSED.txt",
                self.prefix,
                self.host.originals.borrow().len() - 1
            ),
            original.as_bytes(),
        );
        parsed.map(Arc::new).map_err(|error| error.to_string())
    }
}
type NativeAdmission = (Option<usize>, Instance, Vec<(String, Instance)>);
fn invoke(project: &Project, call: &Call<'_>) -> Actual {
    let controlled = !call.case["filter_map_controls"].is_null();
    let admission = (|| -> Result<NativeAdmission, JsonBoundaryError> {
        // Selection FIRST. The typed extras API is not substituted for JSON admission.
        let selected = match call.case["selection"]["Named"].as_str() {
            None => None,
            Some(name) => Some(
                project
                    .extra_targets
                    .iter()
                    .position(|target| target.name == name)
                    .ok_or_else(|| {
                        JsonBoundaryError::Execution(RuntimeError::UnknownTarget {
                            name: name.into(),
                        })
                    })?,
            ),
        };
        let declared = project
            .extra_sources
            .iter()
            .filter(|input| input.dynamic_path.is_none())
            .collect::<Vec<_>>();
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in call.inputs {
            if !declared.iter().any(|input| input.name == *name) {
                return Err(RuntimeError::UnexpectedNamedSource { name: name.clone() }.into());
            }
            if !seen.insert(name.as_str()) {
                return Err(RuntimeError::DuplicateNamedSource {
                    name: source_name(name),
                }
                .into());
            }
        }
        for input in &declared {
            if !seen.contains(input.name.as_str()) {
                return Err(RuntimeError::MissingNamedSource {
                    name: source_name(&input.name),
                }
                .into());
            }
        }
        let parse = |schema: &ir::SchemaNode, document: &[u8]| {
            let descriptor = serde_json::to_string(schema).unwrap();
            if call.api == "text" {
                codegen_runtime::parse_json(&descriptor, std::str::from_utf8(document).unwrap())
            } else {
                codegen_runtime::parse_json_bytes(&descriptor, document)
            }
        };
        let source = parse(&project.source, call.source)?;
        let inputs = call
            .inputs
            .iter()
            .map(|(name, document)| {
                let schema = &declared
                    .iter()
                    .find(|input| input.name == *name)
                    .unwrap()
                    .schema;
                Ok((name.clone(), parse(schema, document)?))
            })
            .collect::<Result<Vec<_>, JsonBoundaryError>>()?;
        Ok((selected, source, inputs))
    })();
    let (selected, source, inputs) = match admission {
        Ok(admitted) => admitted,
        Err(error) => {
            return Actual {
                outcome: Err(error),
                counters: None,
                inner: false,
                native: None,
                wire: None,
            };
        }
    };
    let mut original = String::new();
    instance_original(&source, "native-primary", &mut original);
    for (name, input) in &inputs {
        instance_original(input, &format!("native-static[{name:?}]"), &mut original);
    }
    retain(
        call.directory,
        &format!("{}-COMPLETE-NATIVE-PARSED-INPUTS.txt", call.prefix),
        original.as_bytes(),
    );
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in call.parameters {
        parameters.insert(name, value.clone()).unwrap();
    }
    let checker = call.checker.map(NativeChecker);
    let loader = call.loader.map(|host| NativeLoader {
        host,
        project,
        total: Cell::new(0),
        directory: call.directory,
        prefix: call.prefix,
    });
    let mut execution =
        engine::ExecutionContext::with_main_mapping_file_path(call.mapping_path, call.main_path)
            .with_parameters(&parameters);
    if let Some(datetime) = call.datetime {
        execution = execution.with_current_datetime(datetime);
    }
    if let Some(checker) = &checker {
        execution = execution.with_filter_map_cancellation(checker);
    }
    if let Some(loader) = &loader {
        execution = execution.with_dynamic_source_loader(loader);
    }
    if controlled {
        execution = execution.with_filter_map_limits(
            engine::FilterMapLimits::new(call.limits.source_items(), call.limits.work()).unwrap(),
        );
    }
    let selection = selected.map_or(engine::TargetSelection::Primary, |index| {
        engine::TargetSelection::Named(&project.extra_targets[index].name)
    });
    let outcome = engine::run_selected_target_with_sources_and_context(
        project, &source, inputs, &execution, selection,
    );
    let mut original = String::new();
    match &outcome {
        Ok(engine::SelectedTargetOutput::Primary(value)) => {
            instance_original(value, "Primary", &mut original)
        }
        Ok(engine::SelectedTargetOutput::Named(output)) => {
            writeln!(original, "selected-name={:?}", output.name).unwrap();
            instance_original(&output.instance, "Named", &mut original);
        }
        Err(error) => engine_original(error, &mut original),
    }
    retain(
        call.directory,
        &format!("{}-COMPLETE-NATIVE-TYPED-OUTCOME.txt", call.prefix),
        original.as_bytes(),
    );
    let outcome = outcome.map_err(normalize).and_then(|output| {
        let (name, schema, value) = match (selected, output) {
            (None, engine::SelectedTargetOutput::Primary(value)) => (None, &project.target, value),
            (Some(index), engine::SelectedTargetOutput::Named(output))
                if output.name == project.extra_targets[index].name =>
            {
                (
                    Some(output.name),
                    &project.extra_targets[index].schema,
                    output.instance,
                )
            }
            _ => return Err(JsonBoundaryError::InvalidOutput {
                message:
                    "generated mapping returned a target that does not match the resolved selection"
                        .into(),
            }),
        };
        let descriptor = serde_json::to_string(schema).unwrap();
        let document = if call.api == "text" {
            codegen_runtime::serialize_json(&descriptor, &value)?.into_bytes()
        } else {
            codegen_runtime::serialize_json_bytes(&descriptor, &value)?
        };
        Ok((
            Output {
                name: name.clone(),
                document,
            },
            (name, value),
        ))
    });
    let (outcome, native) = match outcome {
        Ok((output, typed)) => (Ok(output), Some(typed)),
        Err(error) => (Err(error), None),
    };
    let mut actual = Actual {
        outcome,
        counters: None,
        inner: controlled,
        native,
        wire: None,
    };
    public_original(call, &actual);
    actual.wire = actual
        .outcome
        .as_ref()
        .ok()
        .map(|output| codegen_runtime::parse_json_bytes(call.wire_schema, &output.document));
    actual
}
pub(super) fn run(
    projects: &[Project],
    corpus: &Json,
    directory: &Path,
) -> Vec<(String, String, bool)> {
    let schemas = projects
        .iter()
        .map(|project| {
            (
                serde_json::to_string(&project.target).unwrap(),
                project
                    .extra_targets
                    .iter()
                    .map(|target| {
                        (
                            target.name.clone(),
                            serde_json::to_string(&target.schema).unwrap(),
                        )
                    })
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    cases(corpus, directory, &schemas, false, |call| {
        invoke(&projects[call.profile], call)
    })
}
