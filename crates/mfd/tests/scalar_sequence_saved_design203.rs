//! Ignored, bounded public-API qualification for the frozen clean-room recipes.
//! This is native execution and MFD refusal evidence, not native MFD interchange.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Debug;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use engine::{
    EngineError, ExecutionContext, FilterMapBoundary, FilterMapBoundaryKind, FilterMapBudgetKind,
    FilterMapCancellation, FilterMapLimits, FilterMapPhase, TraceEvent, TracePosition, TraceSink,
    TraceValue,
};
use ir::{Instance, ScalarType, SchemaNode, Value, XmlTypeOrigin};
use mapping::{Binding, FunctionId, Project, Scope, ScopeIteration};
use serde::Serialize;
use serde_json::{Value as Json, json};

const RECIPES: &str = include_str!("fixtures/scalar_sequence_saved_design203_recipes.json");
const EXPECTED: &str = include_str!("fixtures/scalar_sequence_saved_design203_expected.json");
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TREE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TREE_FILES: usize = 512;

fn save(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(io::Error::other("203 original exceeds 64 MiB"));
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn save_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    save(path, &bytes)
}

fn error_chain(error: &(dyn Error + 'static)) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = Some(error);
    while let Some(error) = current {
        chain.push(error.to_string());
        current = error.source();
    }
    chain
}

fn scalar_original(value: &Value) -> Json {
    match value {
        Value::Null => json!({"tag":"Null","value":null}),
        Value::JsonNull(value) => json!({"tag":"JsonNull","value":value}),
        Value::XmlNil(value) => json!({"tag":"XmlNil","value":value}),
        Value::Bool(value) => json!({"tag":"Bool","value":value}),
        Value::Int(value) => json!({"tag":"Int","value":value}),
        Value::Float(value) => json!({"tag":"Float","value":value,
            "binary64_bits":format!("{:016x}",value.to_bits())}),
        Value::String(value) => json!({"tag":"String","value":value}),
    }
}

fn instance_original(value: &Instance) -> Json {
    match value {
        Instance::Scalar(value) => json!({"kind":"scalar","value":scalar_original(value)}),
        Instance::Group(fields) => {
            let origin = match fields.xml_type_origin() {
                XmlTypeOrigin::Unknown => json!({"tag":"Unknown","identity":null,"literal":null}),
                XmlTypeOrigin::Absent => json!({"tag":"Absent","identity":null,"literal":null}),
                XmlTypeOrigin::Explicit(identity) => {
                    json!({"tag":"Explicit","identity":identity,"literal":null})
                }
                XmlTypeOrigin::ExplicitPadded {
                    literal,
                    resolved_identity,
                } => json!({"tag":"ExplicitPadded","identity":resolved_identity,"literal":literal}),
            };
            json!({"kind":"group","origin":origin,
                "fields":fields.iter().map(|(name,value)|json!([name,instance_original(value)])).collect::<Vec<_>>()})
        }
        Instance::Repeated(items) => json!({"kind":"repeated",
            "items":items.iter().map(instance_original).collect::<Vec<_>>()}),
        Instance::MappedSequence(items) => json!({"kind":"mapped_sequence",
            "items":items.iter().map(instance_original).collect::<Vec<_>>()}),
        Instance::DocumentSet(items) => json!({"kind":"document_set",
            "items":items.iter().map(|item|json!({"path":item.path(),
                "source_path":item.source_path(),"value":instance_original(item.value())})).collect::<Vec<_>>()}),
    }
}

fn boundary_original(at: &FilterMapBoundary) -> Json {
    json!({"item":at.item,"phase":format!("{:?}",at.phase),
        "capture_index":at.capture_index,"source_position":at.source_position,
        "function":at.function.map(FunctionId::get),"node":at.node,"kind":format!("{:?}",at.kind)})
}

fn value_literal(value: &Value) -> Json {
    match value {
        Value::Null => json!({"Null":null}),
        Value::JsonNull(value) => json!({"JsonNull":value}),
        Value::XmlNil(value) => json!({"XmlNil":value}),
        Value::Bool(value) => json!({"Bool":value}),
        Value::Int(value) => json!({"Int":value}),
        Value::Float(value) => {
            json!({"Float":value,"binary64_bits":format!("{:016x}",value.to_bits())})
        }
        Value::String(value) => json!({"String":value}),
    }
}

fn engine_error_original(error: &EngineError) -> Json {
    match error {
        EngineError::FilterMapRuntime { boundary, source } => json!({"FilterMapRuntime":{
            "boundary":boundary_original(boundary),"source":engine_error_original(source)}}),
        EngineError::MappingException { node, message } => {
            json!({"MappingException":{"node":node,"message":message}})
        }
        EngineError::FilterMapValueType { expected, found } => json!({"FilterMapValueType":{
            "expected":format!("{expected:?}"),"found":value_literal(found)}}),
        EngineError::FilterMapNonFinite { bits } => {
            json!({"FilterMapNonFinite":{"bits":format!("{bits:016x}")}})
        }
        EngineError::FilterMapBudget {
            kind,
            used,
            requested,
            max,
        } => json!({"FilterMapBudget":{
            "kind":format!("{kind:?}"),"used":used.to_string(),"requested":requested.to_string(),"max":max.to_string()}}),
        EngineError::FilterMapCancelled => json!({"FilterMapCancelled":{}}),
        other => {
            json!({"UnexpectedPublicError":{"debug":format!("{other:#?}"),"display":other.to_string()}})
        }
    }
}

fn ty(value: &Json) -> ScalarType {
    match value.as_str().expect("frozen scalar type") {
        "int" | "Int" => ScalarType::Int,
        "bool" | "Bool" => ScalarType::Bool,
        "float" | "Float" => ScalarType::Float,
        "string" | "String" => ScalarType::String,
        other => panic!("unknown frozen scalar type {other}"),
    }
}

fn phase(value: &Json) -> FilterMapPhase {
    match value.as_str().expect("frozen phase") {
        "Source" => FilterMapPhase::Source,
        "Capture" => FilterMapPhase::Capture,
        "Predicate" => FilterMapPhase::Predicate,
        "Mapper" => FilterMapPhase::Mapper,
        other => panic!("unknown frozen phase {other}"),
    }
}

fn kind(value: &Json) -> FilterMapBoundaryKind {
    match value.as_str().expect("frozen boundary kind") {
        "NodeEvaluation" => FilterMapBoundaryKind::NodeEvaluation,
        "CallEntry" => FilterMapBoundaryKind::CallEntry,
        "SourceReservation" => FilterMapBoundaryKind::SourceReservation,
        "StageItem" => FilterMapBoundaryKind::StageItem,
        "Result" => FilterMapBoundaryKind::Result,
        other => panic!("unknown frozen boundary {other}"),
    }
}

fn literal_value(value: &Json) -> Value {
    if let Some(value) = value.get("Int") {
        return Value::Int(value.as_i64().unwrap());
    }
    if let Some(value) = value.get("Bool") {
        return Value::Bool(value.as_bool().unwrap());
    }
    if let Some(value) = value.get("Float") {
        return Value::Float(value.as_f64().unwrap());
    }
    if let Some(value) = value.get("String") {
        return Value::String(value.as_str().unwrap().into());
    }
    assert!(value.get("Null").is_some(), "unknown frozen Value");
    Value::Null
}

fn expected_error(value: &Json) -> EngineError {
    if let Some(value) = value.get("FilterMapRuntime") {
        let at = &value["boundary"];
        return EngineError::FilterMapRuntime {
            boundary: FilterMapBoundary {
                item: at["item"].as_u64().unwrap() as u32,
                phase: phase(&at["phase"]),
                capture_index: at["capture_index"].as_u64().map(|v| v as usize),
                source_position: at["source_position"].as_u64().map(|v| v as usize),
                function: at["function"].as_u64().map(FunctionId::new),
                node: at["node"].as_u64().map(|v| v as u32),
                kind: kind(&at["kind"]),
            },
            source: Box::new(expected_error(&value["source"])),
        };
    }
    if let Some(value) = value.get("MappingException") {
        return EngineError::MappingException {
            node: value["node"].as_u64().unwrap() as u32,
            message: value["message"].as_str().map(str::to_owned),
        };
    }
    if let Some(value) = value.get("FilterMapValueType") {
        return EngineError::FilterMapValueType {
            expected: ty(&value["expected"]),
            found: literal_value(&value["found"]),
        };
    }
    if let Some(value) = value.get("FilterMapBudget") {
        return EngineError::FilterMapBudget {
            kind: match value["kind"].as_str().unwrap() {
                "SourceItems" => FilterMapBudgetKind::SourceItems,
                "Work" => FilterMapBudgetKind::Work,
                other => panic!("unknown frozen budget {other}"),
            },
            used: value["used"].as_u64().unwrap() as u128,
            requested: value["requested"].as_u64().unwrap() as u128,
            max: value["max"].as_u64().unwrap() as u128,
        };
    }
    assert!(
        value.get("FilterMapCancelled").is_some(),
        "unknown frozen engine error"
    );
    EngineError::FilterMapCancelled
}

fn position_original(at: &TracePosition) -> Json {
    json!({"collection":at.collection,"index":at.index,"grouped":at.grouped,
        "join":at.join.map(|v|v.get()),"join_position":at.join_position.map(|(v,n)|(v.get(),n)),
        "document_path":at.document_path})
}

fn preview(value: &TraceValue) -> Json {
    json!({"value_type":value.value_type,"preview":value.preview,"truncated":value.truncated})
}

struct Observations {
    cancel: Option<(FilterMapPhase, FilterMapBoundaryKind)>,
    outputs: BTreeMap<FunctionId, u32>,
    originals: RefCell<Vec<String>>,
    boundaries: RefCell<Vec<FilterMapBoundary>>,
    node_values: RefCell<Vec<Json>>,
    function_values: RefCell<Vec<Json>>,
    stages: RefCell<Vec<Json>>,
}

impl FilterMapCancellation for Observations {
    fn is_cancelled(&self, at: &FilterMapBoundary) -> bool {
        self.originals.borrow_mut().push(format!("CHECKER {at:#?}"));
        self.boundaries.borrow_mut().push(*at);
        if at.kind == FilterMapBoundaryKind::CallEntry
            && matches!(at.function.map(FunctionId::get), Some(7001 | 7002))
        {
            self.stages.borrow_mut().push(json!({"entry":{"phase":format!("{:?}",at.phase),
                "function":at.function.map(FunctionId::get),"source_position":at.source_position}}));
        }
        self.cancel == Some((at.phase, at.kind))
    }
}

impl TraceSink for Observations {
    fn record(&self, event: TraceEvent) {
        self.originals
            .borrow_mut()
            .push(format!("TRACE {event:#?}"));
        match event {
            TraceEvent::NodeValue {
                node,
                positions,
                value,
            } => {
                let mut result = preview(&value);
                result["node"] = json!(node);
                result["positions"] =
                    json!(positions.iter().map(position_original).collect::<Vec<_>>());
                self.node_values.borrow_mut().push(result);
            }
            TraceEvent::FunctionNodeValue {
                function,
                node,
                value,
                ..
            } => {
                let mut result = preview(&value);
                result["function"] = json!(function.get());
                result["node"] = json!(node);
                self.function_values.borrow_mut().push(result.clone());
                if self.outputs.get(&function) == Some(&node) {
                    self.stages
                        .borrow_mut()
                        .push(json!({"output_preview":result}));
                }
            }
            _ => {}
        }
    }
}

fn materialize(case: &Json) -> (Project, String) {
    let mut graph: mapping::Graph = serde_json::from_value(case["parent_graph"].clone()).unwrap();
    let mut root = Scope::default();
    let source = SchemaNode::group("Input", vec![]);
    let site = case["consumer"]["site"].as_str().unwrap();
    let target = if site == "generated_scope" {
        let recipe = &case["consumer"]["scope_recipe"];
        root.children.push(Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Sequence(
                serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
            ),
            filter: recipe["filter"].as_u64().map(|v| v as u32),
            bindings: serde_json::from_value(recipe["bindings"].clone()).unwrap(),
            ..Scope::default()
        });
        let value_type = case["sequence_descriptor"]
            .get("output_type")
            .map_or(ScalarType::Int, ty);
        SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Value", value_type),
                        SchemaNode::scalar("Position", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        )
    } else {
        let node = case["consumer"]["node_id"].as_u64().unwrap() as u32;
        assert!(
            graph
                .nodes
                .insert(
                    node,
                    serde_json::from_value(case["consumer"]["node"].clone()).unwrap()
                )
                .is_none()
        );
        root.bindings.push(Binding {
            target_field: "Result".into(),
            node,
        });
        SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar(
                "Result",
                if site == "exists" {
                    ScalarType::Bool
                } else {
                    ScalarType::Int
                },
            )],
        )
    };
    let mut project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: serde_json::from_value(case["user_functions"].clone()).unwrap(),
        graph,
        root,
    };
    let input = if case["id"] == "filter-map-source-parent-dense-positions" {
        project.source = SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Parents",
                    vec![
                        SchemaNode::scalar("Parent", ScalarType::Int),
                        SchemaNode::group("Driver", vec![]).repeating(),
                    ],
                )
                .repeating(),
            ],
        );
        let rows = project.root.children.remove(0);
        project.root.children.push(Scope {
            target_field: "Parents".into(),
            iteration: ScopeIteration::Source(vec!["Parents".into(), "Driver".into()]),
            children: vec![rows],
            ..Scope::default()
        });
        let ir::SchemaKind::Group { children, .. } = &project.target.kind else {
            panic!("frozen root schema")
        };
        project.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Parents", vec![children[0].clone()]).repeating()],
        );
        "{\"Parents\":[{\"Parent\":1,\"Driver\":[]},{\"Parent\":2,\"Driver\":[]},{\"Parent\":3,\"Driver\":[]},{\"Parent\":4,\"Driver\":[]},{\"Parent\":5,\"Driver\":[]},{\"Parent\":6,\"Driver\":[]},{\"Parent\":7,\"Driver\":[{}]}]}\n".into()
    } else {
        "{}\n".into()
    };
    (project, input)
}

fn independent_input(case: &Json) -> Instance {
    // Exact hand-authored public input shell: seven ordinary rows, not a
    // generated parent or a constant substituted for the Position capture.
    if case["id"] == "filter-map-source-parent-dense-positions" {
        Instance::Group(
            vec![(
                "Parents".into(),
                Instance::Repeated(vec![
                    Instance::Group(
                        vec![
                            ("Parent".into(), Instance::Scalar(Value::Int(1))),
                            ("Driver".into(), Instance::Repeated(vec![])),
                        ]
                        .into(),
                    ),
                    Instance::Group(
                        vec![
                            ("Parent".into(), Instance::Scalar(Value::Int(2))),
                            ("Driver".into(), Instance::Repeated(vec![])),
                        ]
                        .into(),
                    ),
                    Instance::Group(
                        vec![
                            ("Parent".into(), Instance::Scalar(Value::Int(3))),
                            ("Driver".into(), Instance::Repeated(vec![])),
                        ]
                        .into(),
                    ),
                    Instance::Group(
                        vec![
                            ("Parent".into(), Instance::Scalar(Value::Int(4))),
                            ("Driver".into(), Instance::Repeated(vec![])),
                        ]
                        .into(),
                    ),
                    Instance::Group(
                        vec![
                            ("Parent".into(), Instance::Scalar(Value::Int(5))),
                            ("Driver".into(), Instance::Repeated(vec![])),
                        ]
                        .into(),
                    ),
                    Instance::Group(
                        vec![
                            ("Parent".into(), Instance::Scalar(Value::Int(6))),
                            ("Driver".into(), Instance::Repeated(vec![])),
                        ]
                        .into(),
                    ),
                    Instance::Group(
                        vec![
                            ("Parent".into(), Instance::Scalar(Value::Int(7))),
                            (
                                "Driver".into(),
                                Instance::Repeated(vec![Instance::Group(Vec::new().into())]),
                            ),
                        ]
                        .into(),
                    ),
                ]),
            )]
            .into(),
        )
    } else {
        Instance::Group(Vec::new().into())
    }
}

fn retain_tree(root: &Path, evidence: &Path) -> io::Result<Vec<Json>> {
    fn visit(
        root: &Path,
        path: &Path,
        evidence: &Path,
        entries: &mut Vec<Json>,
        total: &mut u64,
    ) -> io::Result<()> {
        let mut children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(io::Error::other("unexpected export symlink"));
            }
            if meta.is_dir() {
                visit(root, &path, evidence, entries, total)?;
                continue;
            }
            if !meta.is_file()
                || entries.len() >= MAX_TREE_FILES
                || meta.len() > MAX_FILE_BYTES as u64
            {
                return Err(io::Error::other("export original tree bound/type refusal"));
            }
            *total += meta.len();
            if *total > MAX_TREE_BYTES {
                return Err(io::Error::other("export original tree exceeds 128 MiB"));
            }
            let bytes = fs::read(&path)?;
            let copy = evidence.join(format!("artifact-{:03}.original", entries.len()));
            save(&copy, &bytes)?;
            entries.push(json!({"relative":path.strip_prefix(root).unwrap().to_string_lossy(),"bytes":bytes.len(),"copy":copy.file_name().unwrap().to_string_lossy()}));
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, evidence, &mut entries, &mut 0)?;
    Ok(entries)
}

fn mfd_control<T: Debug + Serialize>(
    actual: &Result<T, mfd::MfdError>,
    dir: &Path,
    artifacts: &Path,
) -> io::Result<bool> {
    save(
        &dir.join("result.debug.txt"),
        format!("{actual:#?}\n").as_bytes(),
    )?;
    let diagnostic = match actual {
        Ok(value) => json!({"Ok":value}),
        Err(error) => {
            json!({"Err":{"debug":format!("{error:#?}"),"display":error.to_string(),"causes":error_chain(error),
            "unsupported_item":if let mfd::MfdError::UnsupportedSequenceComposition {item}=error {Some(*item)}else {None}}})
        }
    };
    save_json(&dir.join("result.complete.json"), &diagnostic)?;
    let tree = retain_tree(artifacts, dir);
    save(
        &dir.join("artifact-retention.result.debug.txt"),
        format!("{tree:#?}\n").as_bytes(),
    )?;
    let tree = tree?;
    save_json(&dir.join("artifacts.complete.json"), &tree)?;
    Ok(matches!(
        actual,
        Err(mfd::MfdError::UnsupportedSequenceComposition { item: 111 })
    ) && tree.is_empty()
        && actual.as_ref().err().is_some_and(|error| {
            error.to_string()
                == "cannot export filter/map sequence item 111: capability is unsupported"
        }))
}

fn exposed_checks(observed: &Observations, expected: &Json, case: &Json) -> Vec<String> {
    let mut failures = Vec::new();
    if *observed.stages.borrow() != *expected["stage_public_observation"].as_array().unwrap() {
        failures.push("stage entry/output preview order".into());
    }
    let additional = &expected["additional_public_observations"];
    let boundaries = observed.boundaries.borrow();
    let values = observed.node_values.borrow();
    for (field, phase_value) in [
        ("source_node_attempts", FilterMapPhase::Source),
        ("capture_node_attempts", FilterMapPhase::Capture),
    ] {
        if let Some(nodes) = additional[field].as_array() {
            let actual = boundaries
                .iter()
                .filter(|at| {
                    at.phase == phase_value
                        && at.kind == FilterMapBoundaryKind::NodeEvaluation
                        && at.function.is_none()
                })
                .filter_map(|at| at.node)
                .map(|node| json!(node))
                .collect::<Vec<_>>();
            if actual != *nodes {
                failures.push(field.into());
            }
        }
    }
    let expected_captures = expected["capture_success_previews"].as_array().unwrap();
    let actual_captures = values
        .iter()
        .filter(|at| matches!(at["node"].as_u64(), Some(120 | 122)))
        .map(|at| {
            let mut value = at.clone();
            value.as_object_mut().unwrap().remove("positions");
            value
        })
        .collect::<Vec<_>>();
    if actual_captures != *expected_captures {
        failures.push("capture success order/previews".into());
    }
    if let Some(node) = additional["suppressed_parent_node"].as_u64()
        && values.iter().any(|at| at["node"] == node)
    {
        failures.push("downstream consumer suppression".into());
    }
    if let Some(function) = additional["suppressed_function"].as_u64()
        && boundaries.iter().any(|at| {
            at.kind == FilterMapBoundaryKind::CallEntry
                && at.function.map(FunctionId::get) == Some(function)
        })
    {
        failures.push("nested callee suppression".into());
    }
    if let Some(expected_value) = additional.get("nested_false_argument")
        && !observed.function_values.borrow().contains(expected_value)
    {
        failures.push("nested raw false argument preview".into());
    }
    if let Some(count) = additional["reservation_attempts"].as_u64()
        && boundaries
            .iter()
            .filter(|at| at.kind == FilterMapBoundaryKind::SourceReservation)
            .count()
            != count as usize
    {
        failures.push("reservation checker count".into());
    }
    if let Some(position) = additional.get("capture_parent_position")
        && !values.iter().any(|at| {
            at["node"] == 120
                && at["positions"]
                    .as_array()
                    .is_some_and(|positions| positions.contains(position))
        })
    {
        failures.push("real parent position7".into());
    }
    if let Some(expected_positions) = case.get("expected_downstream_predicate_source_positions") {
        let predicate = case["consumer"]["node"]["predicate"].as_u64().unwrap();
        let actual_positions = values
            .iter()
            .filter(|at| at["node"] == predicate)
            .map(|at| {
                at["positions"]
                    .as_array()
                    .and_then(|positions| {
                        positions
                            .iter()
                            .rev()
                            .find(|position| position["collection"] == json!([]))
                    })
                    .map_or(Json::Null, |position| position["index"].clone())
            })
            .collect::<Vec<_>>();
        if json!(actual_positions) != *expected_positions {
            failures.push("legacy Exists predicate positions and lazy stop".into());
        }
    }
    failures
}

fn produce(execute_public_controls: bool) -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_SAVED_DESIGN203_EVIDENCE_DIR")
            .ok_or("explicit evidence directory required")?,
    );
    let spelling = root.components().collect::<PathBuf>();
    if !root.is_absolute()
        || spelling.as_os_str() != root.as_os_str()
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        || root.exists()
    {
        return Err("evidence root must be absent normalized absolute owned path".into());
    }
    fs::create_dir(&root)?;
    save(
        &root.join("frozen-recipes.original.json"),
        RECIPES.as_bytes(),
    )?;
    save(
        &root.join("frozen-independent-public-expected.original.json"),
        EXPECTED.as_bytes(),
    )?;
    let corpus: Json = serde_json::from_str(RECIPES)?;
    let frozen: Json = serde_json::from_str(EXPECTED)?;
    let cases = corpus["cases"].as_array().ok_or("recipes cases")?;
    if cases.len() != 24 || frozen["cases"].as_array().map(Vec::len) != Some(24) {
        return Err("frozen count refusal".into());
    }
    let mut failures = Vec::new();
    let mut native_calls = 0;
    let mut mfd_calls = 0;
    let mut summaries = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let expected = &frozen["cases"][index];
        if case["id"] != expected["id"] {
            return Err("frozen order refusal".into());
        }
        let dir = root.join(format!("{index:02}-{}", case["id"].as_str().unwrap()));
        fs::create_dir(&dir)?;
        let (project, input_text) = materialize(case);
        let expected_input = independent_input(case);
        save(
            &dir.join("project.before.debug.txt"),
            format!("{project:#?}\n").as_bytes(),
        )?;
        save_json(&dir.join("independent-expected.original.json"), expected)?;
        save_json(
            &dir.join("invocation-control.original.json"),
            &json!({"parent":case["invocation_parent"],"host_controls":case.get("host_controls"),"observation_limits":frozen["observation_limits"]}),
        )?;
        save(&dir.join("input.json"), input_text.as_bytes())?;
        save_json(
            &dir.join("independent-input.complete-typed.expected.json"),
            &instance_original(&expected_input),
        )?;
        let encoded = mapping::project_file::encode_pretty(&project);
        save(
            &dir.join("project-encode.result.debug.txt"),
            format!("{encoded:#?}\n").as_bytes(),
        )?;
        let encoded = match encoded {
            Ok(value) => value,
            Err(_) => {
                failures.push(format!("{index}: project encode"));
                continue;
            }
        };
        save(&dir.join("project.json"), encoded.as_bytes())?;
        let decoded = mapping::project_file::decode_bytes(encoded.as_bytes());
        save(
            &dir.join("project-decode.result.debug.txt"),
            format!("{decoded:#?}\n").as_bytes(),
        )?;
        let project = match decoded {
            Ok(value) => value,
            Err(_) => {
                failures.push(format!("{index}: project decode"));
                continue;
            }
        };
        let reencoded = mapping::project_file::encode_pretty(&project);
        save(
            &dir.join("project-reencode.result.debug.txt"),
            format!("{reencoded:#?}\n").as_bytes(),
        )?;
        if reencoded.as_ref().ok() != Some(&encoded) {
            failures.push(format!("{index}: complete codec roundtrip"));
        }
        let input = format_json::from_str(&input_text, &project.source);
        save(
            &dir.join("input-read.result.debug.txt"),
            format!("{input:#?}\n").as_bytes(),
        )?;
        let input = match input {
            Ok(value) => value,
            Err(error) => {
                save_json(
                    &dir.join("input-read.error-causes.json"),
                    &error_chain(&error),
                )?;
                failures.push(format!("{index}: input read"));
                continue;
            }
        };
        save_json(
            &dir.join("input.complete-typed.original.json"),
            &instance_original(&input),
        )?;
        if instance_original(&input) != instance_original(&expected_input) {
            failures.push(format!("{index}: complete independent typed input shell"));
        }
        if !execute_public_controls {
            save_json(
                &dir.join("comparison.complete.json"),
                &json!({"mode":"materialize-only","native_calls":0,"mfd_control_calls":0,"internal_observations_qualified":false}),
            )?;
            summaries.push(json!({"index":index,"id":case["id"],"mode":"materialize-only","native_calls":0,"mfd_control_calls":0}));
            continue;
        }
        let observations = Observations {
            cancel: expected["cancellation"].as_object().map(|_| {
                (
                    phase(&expected["cancellation"]["phase"]),
                    kind(&expected["cancellation"]["kind"]),
                )
            }),
            outputs: project
                .user_functions
                .iter()
                .map(|(id, function)| (*id, function.output))
                .collect(),
            originals: RefCell::new(Vec::new()),
            boundaries: RefCell::new(Vec::new()),
            node_values: RefCell::new(Vec::new()),
            function_values: RefCell::new(Vec::new()),
            stages: RefCell::new(Vec::new()),
        };
        let limits = case
            .get("host_controls")
            .and_then(|v| v.get("limits"))
            .map(|v| {
                FilterMapLimits::new(
                    v["source_items"].as_u64().unwrap() as u128,
                    v["work"].as_u64().unwrap() as u128,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let mapping_path = dir.join("project.json");
        let context = ExecutionContext::new(&mapping_path)
            .with_filter_map_limits(limits)
            .with_filter_map_cancellation(&observations)
            .with_trace_sink(&observations);
        native_calls += 1;
        let actual = engine::run_with_context(&project, &input, &context);
        save(
            &dir.join("native-result.debug.txt"),
            format!("{actual:#?}\n").as_bytes(),
        )?;
        let actual_complete = match &actual {
            Ok(value) => json!({"Ok":instance_original(value)}),
            Err(error) => json!({"Err":engine_error_original(error)}),
        };
        save_json(
            &dir.join("native-result.complete-typed.original.json"),
            &actual_complete,
        )?;
        if let Err(error) = &actual {
            save_json(
                &dir.join("native-error.complete-causes.original.json"),
                &error_chain(error),
            )?;
        }
        save_json(
            &dir.join("public-observations.complete-original.json"),
            &json!({"boundaries":observations.boundaries.borrow().iter().map(boundary_original).collect::<Vec<_>>(),"node_values":*observations.node_values.borrow(),"function_values":*observations.function_values.borrow(),"stage_public_observation":*observations.stages.borrow()}),
        )?;
        save(
            &dir.join("public-trace-and-checker.full-debug.original.txt"),
            format!("{}\n", observations.originals.borrow().join("\n")).as_bytes(),
        )?;
        save_json(&dir.join("project.after.json"), &project)?;
        save_json(
            &dir.join("input.after.complete-typed.original.json"),
            &instance_original(&input),
        )?;
        let project_after = mapping::project_file::encode_pretty(&project);
        save(
            &dir.join("project-after-encode.result.debug.txt"),
            format!("{project_after:#?}\n").as_bytes(),
        )?;
        // Retain independent typed error/display/cause expectations before comparison.
        let expected_error = expected["expected_root_outcome"]
            .get("Err")
            .map(expected_error);
        if let Some(error) = &expected_error {
            save(
                &dir.join("expected-native-error.debug.txt"),
                format!("{error:#?}\n").as_bytes(),
            )?;
            save_json(
                &dir.join("expected-native-error.causes.json"),
                &error_chain(error),
            )?;
        }
        // All complete public/native originals above precede every outcome comparison.
        let mut row = exposed_checks(&observations, expected, case);
        if project_after.as_ref().ok() != Some(&encoded) {
            row.push("complete project codec bytes after native run".into());
        }
        if instance_original(&input) != instance_original(&expected_input) {
            row.push("complete input shell after native run".into());
        }
        match (&actual, &expected_error) {
            (Err(actual), Some(expected)) => {
                if actual != expected || error_chain(actual) != error_chain(expected) {
                    row.push("full typed native error and causes".into());
                }
            }
            (Ok(_), None) => {
                if actual_complete != expected["expected_root_outcome"] {
                    row.push("complete root/value/origin/binary64 result".into());
                }
            }
            _ => row.push("native outcome variant".into()),
        }
        if index >= 7 {
            for action in [
                "preflight",
                "ferrule_extensions_export",
                "native_mfd_export",
            ] {
                let control = dir.join(action);
                fs::create_dir(&control)?;
                let artifacts = control.join("publication");
                fs::create_dir(&artifacts)?;
                let path = artifacts.join("unpublished.mfd");
                mfd_calls += 1;
                let matched = match action {
                    "preflight" => mfd_control(
                        &mfd::preflight_export(&project, &path),
                        &control,
                        &artifacts,
                    )?,
                    "ferrule_extensions_export" => {
                        mfd_control(&mfd::export(&project, &path), &control, &artifacts)?
                    }
                    "native_mfd_export" => mfd_control(
                        &mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd),
                        &control,
                        &artifacts,
                    )?,
                    _ => unreachable!(),
                };
                if !matched {
                    row.push(format!("MFD {action} global refusal/artifact absence"));
                }
            }
        }
        save_json(
            &dir.join("comparison.complete.json"),
            &json!({"failures":row,"internal_fields_unqualified":expected["unqualified_internal_fields"]}),
        )?;
        failures.extend(row.iter().map(|failure| format!("{index}: {failure}")));
        summaries.push(json!({"index":index,"id":case["id"],"native_outcome_matched":row.is_empty(),"internal_observations_qualified":false}));
    }
    let summary = json!({"mode":if execute_public_controls {"qualify-public"}else {"materialize-only"},"native_calls":native_calls,"mfd_control_calls":mfd_calls,"cases":summaries,"failures":failures,
        "external_mfd_save_reopen_calls":0,"internal_counter_or_raw_stage_qualification":false});
    save_json(&root.join("COMPLETE-PUBLIC-QUALIFICATION.json"), &summary)?;
    eprintln!(
        "203 complete originals={} native_calls={native_calls} separate_mfd_control_calls={mfd_calls}",
        root.display()
    );
    assert_eq!(
        summaries.len(),
        24,
        "complete public materialization cohort"
    );
    assert_eq!(
        native_calls,
        if execute_public_controls { 24 } else { 0 },
        "separate native cohort"
    );
    assert_eq!(
        mfd_calls,
        if execute_public_controls { 51 } else { 0 },
        "separate MFD refusal cohort"
    );
    assert!(
        failures.is_empty(),
        "203 public qualification failures: {failures:?}"
    );
    Ok(())
}

#[test]
#[ignore = "root-owned 24 public codec/input materializations; explicit absent evidence directory required"]
fn materialize_frozen_scalar_sequence_saved_design203_public_projects() -> Result<(), Box<dyn Error>>
{
    produce(false)
}

#[test]
#[ignore = "root-owned finite 24-native/51-MFD qualification; explicit absent evidence directory required"]
fn qualify_frozen_scalar_sequence_saved_design203_public_recipes() -> Result<(), Box<dyn Error>> {
    produce(true)
}
