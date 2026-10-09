//! Literal native selection witnesses; generated outcomes supply no oracle.
use codegen_runtime::{
    FilterMapBoundary, FilterMapBoundaryKind, FilterMapBudgetKind, FilterMapPhase, RuntimeError,
    XmlTypeOrigin,
};
use ir::{Instance, Value};
use mapping::{FunctionId, Project};
use serde_json::Value as Json;
use std::{cell::RefCell, fmt::Write as _, path::Path, sync::Arc};

#[derive(Debug, PartialEq)]
enum Output {
    Primary(Instance),
    Named {
        name: String,
        instance: Instance,
    },
    All {
        primary: Instance,
        extras: Vec<(String, Instance)>,
    },
}
pub(super) fn typed_instance(value: &Json) -> Instance {
    match value["kind"].as_str().unwrap() {
        "Scalar" => Instance::Scalar(match value["value"]["tag"].as_str().unwrap() {
            "String" => Value::String(value["value"]["value"].as_str().unwrap().into()),
            "Int" => Value::Int(value["value"]["value"].as_i64().unwrap()),
            "Bool" => Value::Bool(value["value"]["value"].as_bool().unwrap()),
            "Float" => Value::Float(f64::from_bits(
                u64::from_str_radix(value["value"]["ieee754_binary64_hex"].as_str().unwrap(), 16)
                    .unwrap(),
            )),
            "Null" => Value::Null,
            "JsonNull" => serde_json::from_str(r#"{"$json_null":true}"#).unwrap(),
            "XmlNil" => serde_json::from_str(r#"{"$xml_nil":true}"#).unwrap(),
            _ => panic!("unknown independently authored scalar tag"),
        }),
        "Group" => {
            let fields = value["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|field| {
                    (
                        field["name"].as_str().unwrap().into(),
                        typed_instance(&field["instance"]),
                    )
                })
                .collect::<Vec<_>>();
            let origin = &value["origin"];
            let origin = match origin["tag"].as_str().unwrap() {
                "Unknown" => XmlTypeOrigin::Unknown,
                "Absent" => XmlTypeOrigin::Absent,
                "Explicit" => XmlTypeOrigin::Explicit(origin["identity"].as_str().unwrap()),
                "ExplicitPadded" => XmlTypeOrigin::ExplicitPadded {
                    literal: origin["literal"].as_str().unwrap(),
                    resolved_identity: origin["resolved_identity"].as_str().unwrap(),
                },
                _ => panic!("unknown independent origin tag"),
            };
            Instance::Group(
                codegen_runtime::InstanceGroup::from(fields)
                    .with_xml_type_origin(origin)
                    .unwrap(),
            )
        }
        "Repeated" => Instance::Repeated(
            value["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(typed_instance)
                .collect(),
        ),
        "MappedSequence" => Instance::MappedSequence(
            value["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(typed_instance)
                .collect(),
        ),
        "DocumentSet" => Instance::DocumentSet(
            value["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| {
                    if let Some(resolved) = d["resolved_source_path"].as_str() {
                        codegen_runtime::DocumentMember::new_source(
                            d["path"].as_str().unwrap(),
                            resolved,
                            typed_instance(&d["instance"]),
                        )
                        .unwrap()
                    } else {
                        codegen_runtime::DocumentMember::new(
                            d["path"].as_str().unwrap(),
                            typed_instance(&d["instance"]),
                        )
                        .unwrap()
                    }
                })
                .collect(),
        ),
        _ => panic!("unknown independent instance kind"),
    }
}
fn instance_original(value: &Instance, path: &str, original: &mut String) {
    writeln!(original, "path={path:?};complete-instance={value:#?}").unwrap();
    match value {
        Instance::Scalar(Value::Float(value)) => writeln!(
            original,
            "path={path:?};ieee754_binary64_hex={:016x}",
            value.to_bits()
        )
        .unwrap(),
        Instance::Scalar(_) => {}
        Instance::Group(fields) => {
            writeln!(
                original,
                "path={path:?};xml-type-origin={:?}",
                fields.xml_type_origin()
            )
            .unwrap();
            for (index, (name, child)) in fields.iter().enumerate() {
                instance_original(child, &format!("{path}/field[{index}]={name:?}"), original);
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            let kind = if matches!(value, Instance::Repeated(_)) {
                "Repeated"
            } else {
                "MappedSequence"
            };
            for (index, child) in items.iter().enumerate() {
                instance_original(child, &format!("{path}/{kind}[{index}]"), original);
            }
        }
        Instance::DocumentSet(documents) => {
            for (index, document) in documents.iter().enumerate() {
                writeln!(
                    original,
                    "document[{index}].path={:?};effective-source-path={:?}",
                    document.path(),
                    document.source_path()
                )
                .unwrap();
                instance_original(
                    document.value(),
                    &format!("{path}/document[{index}]"),
                    original,
                );
            }
        }
    }
}
fn output_original(value: &Output, original: &mut String) {
    match value {
        Output::Primary(instance) => instance_original(instance, "Primary", original),
        Output::Named { name, instance } => {
            instance_original(instance, &format!("Named({name:?})"), original)
        }
        Output::All { primary, extras } => {
            instance_original(primary, "All/Primary", original);
            for (index, (name, instance)) in extras.iter().enumerate() {
                instance_original(instance, &format!("All/extra[{index}]={name:?}"), original);
            }
        }
    }
}
fn boundary(value: &Json) -> FilterMapBoundary {
    FilterMapBoundary {
        item: value["item"].as_u64().unwrap() as u32,
        phase: match value["phase"].as_str().unwrap() {
            "Source" => FilterMapPhase::Source,
            "Capture" => FilterMapPhase::Capture,
            "Predicate" => FilterMapPhase::Predicate,
            "Mapper" => FilterMapPhase::Mapper,
            _ => panic!("phase"),
        },
        capture_index: value["capture_index"].as_u64().map(|v| v as usize),
        source_position: value["source_position"].as_u64().map(|v| v as usize),
        function: value["function"].as_u64(),
        node: value["node"].as_u64().map(|v| v as u32),
        kind: match value["kind"].as_str().unwrap() {
            "SourceReservation" => FilterMapBoundaryKind::SourceReservation,
            "NodeEvaluation" => FilterMapBoundaryKind::NodeEvaluation,
            "CallEntry" => FilterMapBoundaryKind::CallEntry,
            "StageItem" => FilterMapBoundaryKind::StageItem,
            "Result" => FilterMapBoundaryKind::Result,
            _ => panic!("boundary"),
        },
    }
}
fn expected_error(value: &Json) -> RuntimeError {
    match value["kind"].as_str().unwrap() {
        "UnknownTarget" => RuntimeError::UnknownTarget {
            name: value["name"].as_str().unwrap().into(),
        },
        "MappingException" => RuntimeError::MappingException {
            node: value["node"].as_u64().unwrap() as u32,
            message: value["message"].as_str().map(str::to_owned),
        },
        "MappingFailure" => RuntimeError::MappingFailure {
            rule: value["rule"].as_u64().unwrap() as usize,
            message: value["message"].as_str().map(str::to_owned),
        },
        "DynamicSourceLoad" => RuntimeError::DynamicSourceLoad {
            source: match value["source"].as_str().unwrap() {
                "poison" => "poison",
                "catalog" => "catalog",
                _ => panic!("source"),
            },
            path: value["path"].as_str().unwrap().into(),
            message: value["message"].as_str().unwrap().into(),
        },
        "MissingDynamicSourceLoader" => RuntimeError::MissingDynamicSourceLoader {
            source: match value["source"].as_str().unwrap() {
                "catalog" => "catalog",
                "poison" => "poison",
                _ => panic!("source"),
            },
        },
        "FilterMapRuntime" => RuntimeError::FilterMapRuntime {
            boundary: boundary(&value["boundary"]),
            source: Box::new(expected_error(&value["cause"])),
        },
        "FilterMapBudget" => RuntimeError::FilterMapBudget {
            kind: match value["budget_kind"].as_str().unwrap() {
                "SourceItems" => FilterMapBudgetKind::SourceItems,
                "Work" => FilterMapBudgetKind::Work,
                _ => panic!("budget"),
            },
            used: value["used"].as_u64().unwrap() as u128,
            requested: value["requested"].as_u64().unwrap() as u128,
            max: value["max"].as_u64().unwrap() as u128,
        },
        "FilterMapCancelled" => RuntimeError::FilterMapCancelled,
        _ => panic!("unknown independently authored error"),
    }
}
fn expected_output(value: &Json) -> Output {
    match value["variant"].as_str().unwrap() {
        "Primary" => Output::Primary(typed_instance(&value["instance"])),
        "Named" => Output::Named {
            name: value["output"]["name"].as_str().unwrap().into(),
            instance: typed_instance(&value["output"]["instance"]),
        },
        _ => panic!("unknown independently authored output variant"),
    }
}

fn native_boundary(at: FilterMapBoundary) -> engine::FilterMapBoundary {
    engine::FilterMapBoundary {
        item: at.item,
        phase: match at.phase {
            FilterMapPhase::Source => engine::FilterMapPhase::Source,
            FilterMapPhase::Capture => engine::FilterMapPhase::Capture,
            FilterMapPhase::Predicate => engine::FilterMapPhase::Predicate,
            FilterMapPhase::Mapper => engine::FilterMapPhase::Mapper,
        },
        capture_index: at.capture_index,
        source_position: at.source_position,
        function: at.function.map(FunctionId::new),
        node: at.node,
        kind: match at.kind {
            FilterMapBoundaryKind::SourceReservation => {
                engine::FilterMapBoundaryKind::SourceReservation
            }
            FilterMapBoundaryKind::NodeEvaluation => engine::FilterMapBoundaryKind::NodeEvaluation,
            FilterMapBoundaryKind::CallEntry => engine::FilterMapBoundaryKind::CallEntry,
            FilterMapBoundaryKind::StageItem => engine::FilterMapBoundaryKind::StageItem,
            FilterMapBoundaryKind::Result => engine::FilterMapBoundaryKind::Result,
        },
    }
}
fn native_error(literal: RuntimeError) -> engine::EngineError {
    match literal {
        RuntimeError::UnknownTarget { name } => engine::EngineError::UnknownTarget { name },
        RuntimeError::MappingException { node, message } => {
            engine::EngineError::MappingException { node, message }
        }
        RuntimeError::MappingFailure { rule, message } => {
            engine::EngineError::MappingFailure { rule, message }
        }
        RuntimeError::MissingDynamicSourceLoader { source } => {
            engine::EngineError::MissingDynamicSourceLoader {
                source_name: source.into(),
            }
        }
        RuntimeError::DynamicSourceLoad {
            source,
            path,
            message,
        } => engine::EngineError::DynamicSourceLoad {
            source_name: source.into(),
            path,
            message,
        },
        RuntimeError::FilterMapRuntime { boundary, source } => {
            engine::EngineError::FilterMapRuntime {
                boundary: native_boundary(boundary),
                source: Box::new(native_error(*source)),
            }
        }
        RuntimeError::FilterMapBudget {
            kind,
            used,
            requested,
            max,
        } => engine::EngineError::FilterMapBudget {
            kind: match kind {
                FilterMapBudgetKind::SourceItems => engine::FilterMapBudgetKind::SourceItems,
                FilterMapBudgetKind::Work => engine::FilterMapBudgetKind::Work,
            },
            used,
            requested,
            max,
        },
        RuntimeError::FilterMapCancelled => engine::EngineError::FilterMapCancelled,
        _ => panic!("unhandled independent native error recipe"),
    }
}
fn native_error_original(error: &engine::EngineError, original: &mut String) {
    writeln!(original, "complete-engine-error={error:#?};display={error}").unwrap();
    match error {
        engine::EngineError::FilterMapRuntime { source, .. } => {
            native_error_original(source, original)
        }
        engine::EngineError::FilterMapValueType {
            found: Value::Float(value),
            ..
        } => writeln!(
            original,
            "found.ieee754_binary64_hex={:016x}",
            value.to_bits()
        )
        .unwrap(),
        engine::EngineError::FilterMapNonFinite { bits } => {
            writeln!(original, "ieee754_binary64_hex={bits:016x}").unwrap()
        }
        _ => {}
    }
}
struct Checker {
    specification: Json,
    originals: RefCell<Vec<engine::FilterMapBoundary>>,
}
impl engine::FilterMapCancellation for Checker {
    fn is_cancelled(&self, at: &engine::FilterMapBoundary) -> bool {
        self.originals.borrow_mut().push(*at);
        self.specification == "always_cancel"
            || self
                .specification
                .get("cancel_exactly")
                .is_some_and(|exact| *at == native_boundary(boundary(exact)))
    }
}
#[derive(Debug)]
struct LoaderEvent {
    source: String,
    path: String,
    outcome: Result<Arc<Instance>, String>,
}
struct Loader {
    specification: Json,
    originals: RefCell<Vec<LoaderEvent>>,
}
impl engine::DynamicSourceLoader for Loader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        let reply = self.specification["replies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["source"] == source && row["logical_path"] == path);
        let outcome = match reply {
            Some(row) if row.get("Ok").is_some() => Ok(Arc::new(typed_instance(&row["Ok"]))),
            Some(row) => Err(row["Err"]["message"].as_str().unwrap().into()),
            None => Err(self.specification["any_other_request"]["Err"]["message"]
                .as_str()
                .unwrap()
                .into()),
        };
        self.originals.borrow_mut().push(LoaderEvent {
            source: source.into(),
            path: path.into(),
            outcome: outcome.clone(),
        });
        outcome
    }
}

pub(super) fn run(
    projects: &[Project],
    corpus: &Json,
    evidence: &super::Evidence,
) -> Result<Vec<(String, bool)>, Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut ordinal = 0;
    for (profile_index, profile) in corpus["profiles"].as_array().unwrap().iter().enumerate() {
        let project = &projects[profile_index];
        let source = typed_instance(&profile["complete_source"]);
        let controls = profile["calls"].as_array().unwrap();
        let checkers = controls
            .iter()
            .map(|c| Checker {
                specification: c["cancellation"].clone(),
                originals: RefCell::new(Vec::new()),
            })
            .collect::<Vec<_>>();
        let loaders = controls
            .iter()
            .map(|_| Loader {
                specification: profile["complete_loader"].clone(),
                originals: RefCell::new(Vec::new()),
            })
            .collect::<Vec<_>>();
        let contexts = controls
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut context =
                    engine::ExecutionContext::new(Path::new("selection-controls.mapping"));
                if let Some(limits) = c.get("limits") {
                    context = context.with_filter_map_limits(
                        engine::FilterMapLimits::new(
                            limits["source_items"].as_u64().unwrap() as u128,
                            limits["work"].as_u64().unwrap() as u128,
                        )
                        .unwrap(),
                    );
                }
                if !c["cancellation"].is_null() {
                    context = context.with_filter_map_cancellation(&checkers[i]);
                }
                if c["loader"] == "present" {
                    context = context.with_dynamic_source_loader(&loaders[i]);
                }
                context
            })
            .collect::<Vec<_>>();
        for (i, control) in controls.iter().enumerate() {
            let prefix = format!("native-{ordinal:02}");
            ordinal += 1;
            evidence.write(
                &format!("{prefix}-CONTROL-ORIGINAL.json"),
                &serde_json::to_vec_pretty(control)?,
            )?;
            let context_index = control["reuse_immutable_host_context_from"]
                .as_str()
                .map_or(i, |id| {
                    controls.iter().position(|row| row["id"] == id).unwrap()
                });
            let context = &contexts[context_index];
            let checker = &checkers[context_index];
            let checker_start = checker.originals.borrow().len();
            let loader = &loaders[context_index];
            let extras = control
                .get("named_inputs_override")
                .unwrap_or(&profile["complete_named_inputs"])
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    (
                        row["name"].as_str().unwrap().into(),
                        typed_instance(&row["instance"]),
                    )
                })
                .collect::<Vec<_>>();
            let mut original_input = format!(
                "complete-control={control:#?};profile-host-context-literal={:#?};context-index={context_index};checker-start={checker_start}\ncomplete-actual-native-context-construction-inputs: mapping_file_path=selection-controls.mapping; main_mapping_file_path=selection-controls.mapping; current_datetime=None; parameters=None; purpose={:?}; trace=None; debug=None; dynamic_loader_present={}; filter_map_limits_literal={:#?}; cancellation_literal={:#?}\n",
                profile["context"],
                context.purpose(),
                controls[context_index]["loader"] == "present",
                controls[context_index]["limits"],
                controls[context_index]["cancellation"]
            );
            instance_original(&source, "source", &mut original_input);
            for (name, instance) in &extras {
                instance_original(instance, &format!("extra[{name:?}]"), &mut original_input);
            }
            evidence.write(
                &format!("{prefix}-COMPLETE-INPUT-ORIGINAL.txt"),
                original_input.as_bytes(),
            )?;
            let api = control["api"].as_str().unwrap();
            let actual = if api.starts_with("selected") {
                let selection = control["selection"]
                    .get("Named")
                    .and_then(Json::as_str)
                    .map_or(
                        engine::TargetSelection::Primary,
                        engine::TargetSelection::Named,
                    );
                engine::run_selected_target_with_sources_and_context(
                    project, &source, extras, context, selection,
                )
                .map(|output| match output {
                    engine::SelectedTargetOutput::Primary(instance) => Output::Primary(instance),
                    engine::SelectedTargetOutput::Named(output) => Output::Named {
                        name: output.name,
                        instance: output.instance,
                    },
                })
            } else if api.starts_with("legacy_primary") {
                engine::run_with_sources_and_context(project, &source, extras, context)
                    .map(Output::Primary)
            } else {
                engine::run_outputs_with_sources_and_context(project, &source, extras, context).map(
                    |output| Output::All {
                        primary: output.primary,
                        extras: output
                            .extras
                            .into_iter()
                            .map(|extra| (extra.name, extra.instance))
                            .collect(),
                    },
                )
            };
            let mut original = format!(
                "complete-actual={actual:#?};complete-checker-originals={:#?};complete-loader-originals={:#?}\n",
                checker.originals.borrow(),
                loader.originals.borrow()
            );
            match &actual {
                Ok(output) => output_original(output, &mut original),
                Err(error) => native_error_original(error, &mut original),
            }
            for (index, event) in loader.originals.borrow().iter().enumerate() {
                if let Ok(instance) = &event.outcome {
                    instance_original(
                        instance,
                        &format!("loader-event[{index}]/Ok"),
                        &mut original,
                    );
                }
            }
            evidence.write(
                &format!("{prefix}-COMPLETE-ACTUAL-ORIGINAL.txt"),
                original.as_bytes(),
            )?;
            let expected = if let Some(ok) = control["expected"].get("Ok") {
                Ok(expected_output(ok))
            } else {
                Err(native_error(expected_error(&control["expected"]["Err"])))
            };
            let mut expected_original = format!("complete-expected={expected:#?}\n");
            match &expected {
                Ok(output) => output_original(output, &mut expected_original),
                Err(error) => native_error_original(error, &mut expected_original),
            }
            evidence.write(
                &format!("{prefix}-COMPLETE-EXPECTED-ORIGINAL.txt"),
                expected_original.as_bytes(),
            )?;
            let mut matched = actual == expected;
            if let Some(display) = control["expected"]["Err"]["display"].as_str() {
                matched &= matches!(&actual,Err(error)if error.to_string()==display);
            }
            if let Some(wanted) = control.get("expected_loader_calls") {
                let actual = loader
                    .originals
                    .borrow()
                    .iter()
                    .map(|event| serde_json::json!([event.source, event.path]))
                    .collect::<Vec<_>>();
                matched &= Json::Array(actual) == *wanted;
            }
            if let Some(wanted) = control.get("expected_reservation_output_owners") {
                let actual = checker
                    .originals
                    .borrow()
                    .iter()
                    .skip(checker_start)
                    .filter(|at| at.kind == engine::FilterMapBoundaryKind::SourceReservation)
                    .map(|at| serde_json::json!(at.item))
                    .collect::<Vec<_>>();
                matched &= Json::Array(actual) == *wanted;
            }
            if let Some(count) = control["expected_cancellation_calls"].as_u64() {
                matched &= checker.originals.borrow().len() - checker_start == count as usize;
            }
            checks.push((control["id"].as_str().unwrap().to_owned(), matched));
        }
    }
    evidence.write(
        "ALL30-NATIVE-COMPLETE-COMPARISONS.json",
        &serde_json::to_vec_pretty(&checks)?,
    )?;
    Ok(checks)
}
