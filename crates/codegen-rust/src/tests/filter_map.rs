//! Approved literals adapted to ordinary compiled public entry points.
use super::*;
use mapping::{
    Binding as MappingBinding, Graph, Node, Project, Scope, ScopeIteration, SequenceExpr,
};
use serde_json::{Value as Json, json};

const CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/scalar-filter-map-v1.json"
));
const EXPECTED: &str = include_str!("filter_map/public_controls.json");
const HOST: &str = include_str!("filter_map/host.rs.txt");

fn ty(case: &Json) -> ScalarType {
    match case["sequence_descriptor"]["output_type"]
        .as_str()
        .unwrap_or("int")
    {
        "float" => ScalarType::Float,
        "bool" => ScalarType::Bool,
        "string" => ScalarType::String,
        _ => ScalarType::Int,
    }
}
fn graph(case: &Json) -> Graph {
    serde_json::from_value(case["parent_graph"].clone()).unwrap()
}

fn project(case: &Json, descriptor: SequenceExpr) -> Project {
    let legacy = case.get("descriptor").is_some();
    let output_item = if legacy { 10 } else { 11 };
    let mut graph = graph(case);
    graph.nodes.insert(
        90,
        Node::Position {
            collection: Vec::new(),
        },
    );
    let mut rows = Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Sequence(descriptor.clone()),
        bindings: vec![
            MappingBinding {
                target_field: "Value".into(),
                node: output_item,
            },
            MappingBinding {
                target_field: "Position".into(),
                node: 90,
            },
        ],
        ..Scope::default()
    };
    let mut child_schema = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Value", ty(case)),
            SchemaNode::scalar("Position", ScalarType::Int),
        ],
    )
    .repeating();
    let mut parent = Scope {
        target_field: "Parent".into(),
        iteration: ScopeIteration::Source(vec!["Parents".into()]),
        ..Scope::default()
    };
    match case["evaluation"]["kind"].as_str() {
        Some("outer_if") => {
            graph.nodes.insert(
                96,
                Node::Const {
                    value: Value::Bool(false),
                },
            );
            graph.nodes.insert(
                97,
                Node::Const {
                    value: Value::Int(7),
                },
            );
            graph.nodes.insert(
                98,
                Node::SequenceItemAt {
                    sequence: descriptor,
                    index: 97,
                },
            );
            graph.nodes.insert(
                99,
                Node::If {
                    condition: 96,
                    then: 98,
                    else_: 97,
                },
            );
            parent.bindings.push(MappingBinding {
                target_field: "Value".into(),
                node: 99,
            });
            child_schema = SchemaNode::scalar("Value", ScalarType::Int);
        }
        Some("item_at" | "aggregate" | "exists_positive") => {
            let kind = case["evaluation"]["kind"].as_str().unwrap();
            graph.nodes.insert(
                31,
                Node::Const {
                    value: Value::Int(if kind == "aggregate" { 1 } else { 2 }),
                },
            );
            if kind != "item_at" {
                graph.nodes.insert(
                    32,
                    Node::Call {
                        function: if kind == "aggregate" {
                            "greater_than"
                        } else {
                            "equal"
                        }
                        .into(),
                        args: vec![11, 31],
                    },
                );
            }
            graph.nodes.insert(
                99,
                match kind {
                    "item_at" => Node::SequenceItemAt {
                        sequence: descriptor,
                        index: 31,
                    },
                    "aggregate" => Node::SequenceAggregate {
                        function: mapping::AggregateOp::Sum,
                        sequence: descriptor,
                        predicate: Some(32),
                        expression: Some(11),
                        arg: None,
                    },
                    _ => Node::SequenceExists {
                        sequence: descriptor,
                        predicate: 32,
                    },
                },
            );
            parent.bindings.push(MappingBinding {
                target_field: "Value".into(),
                node: 99,
            });
            child_schema = SchemaNode::scalar(
                "Value",
                if kind == "exists_positive" {
                    ScalarType::Bool
                } else {
                    ScalarType::Int
                },
            );
        }
        Some("exists_after_complete_sequence") => {
            let consumers: Graph =
                serde_json::from_value(json!({"nodes":case["evaluation"]["consumer_nodes"]}))
                    .unwrap();
            graph.nodes.extend(consumers.nodes);
            graph.nodes.insert(
                99,
                Node::SequenceExists {
                    sequence: descriptor,
                    predicate: case["evaluation"]["consumer_output"].as_u64().unwrap() as u32,
                },
            );
            parent.bindings.push(MappingBinding {
                target_field: "Value".into(),
                node: 99,
            });
            child_schema = SchemaNode::scalar("Value", ScalarType::Bool);
        }
        _ => {
            parent.children.push(std::mem::take(&mut rows));
        }
    }
    if case["id"] == "positions-parent-source-dense" {
        graph.nodes.insert(
            20,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
        graph.nodes.insert(
            94,
            Node::Position {
                collection: Vec::new(),
            },
        );
        graph.nodes.insert(
            91,
            Node::Const {
                value: Value::Int(7),
            },
        );
        graph.nodes.insert(
            92,
            Node::Call {
                function: "equal".into(),
                args: vec![94, 91],
            },
        );
        parent.filter = Some(92);
    }
    Project {
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("Parents", ScalarType::Int).repeating()],
        ),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Parent", vec![child_schema]).repeating()],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: if legacy {
            Default::default()
        } else {
            serde_json::from_value(case["user_functions"].clone()).unwrap()
        },
        graph,
        root: Scope {
            children: vec![parent],
            ..Scope::default()
        },
    }
}

fn retain(directory: &Path, name: &str, bytes: &[u8]) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(name))
        .unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
}

fn static_expected(case: &Json, project: &Project) -> Option<mapping::FilterMapAdmissionError> {
    use mapping::{FilterMapAdmissionError, FilterMapAdmissionKind as K, FilterMapStage};
    let kind = match case["id"].as_str().unwrap() {
        "unsupported-tokenizer-source" => K::UnsupportedSource { found: "tokenize" },
        "duplicate-private-owner" => K::DuplicatePrivateOwner { item: 11, count: 2 },
        "predicate-signature-static-before-unselected-runtime" => K::StageSignature {
            stage: FilterMapStage::Predicate,
            function: FunctionId::new(100),
            expected_parameters: vec![ScalarType::Int, ScalarType::Int],
            found_parameters: vec![ScalarType::Int, ScalarType::Int],
            expected_output: ScalarType::Bool,
            found_output: ScalarType::Int,
        },
        "stage-context-node-static-refusal" => K::UnsupportedStageNode {
            function: FunctionId::new(101),
            node: 1,
            kind: "position",
        },
        "stage-cycle-static-refusal" => K::FunctionCycle {
            path: vec![FunctionId::new(101), FunctionId::new(101)],
        },
        "stage-depth65-static-before-any-artifact" => K::FunctionDepth {
            path: (101..=165).map(FunctionId::new).collect(),
            limit: 64,
        },
        _ => return None,
    };
    let location = if case["evaluation"]["kind"] == "outer_if" {
        "graph node 98"
    } else {
        "primary target/child 0/child 0"
    };
    let _ = project;
    Some(FilterMapAdmissionError {
        item: 11,
        location: location.into(),
        kind,
    })
}

fn supplementary(corpus: &Json, index: usize) -> Json {
    let mut case = corpus["cases"][1].clone();
    case["id"] = match index {
        39 => json!("named-default-complete-output-set"),
        40 => json!("named-cumulative-source-budget"),
        41 => json!("named-cumulative-work-reservation"),
        42 => json!("compiled-stage-depth64-complete-result"),
        43 => json!("stage-depth65-static-before-any-artifact"),
        44 => json!("compiled-item-at-output-owner-index"),
        45 => json!("compiled-aggregate-output-owner-predicate"),
        46 => json!("compiled-exists-output-owner-predicate"),
        _ => unreachable!(),
    };
    if (42..=43).contains(&index) {
        case["parent_graph"]["nodes"]["2"]["value"] = json!(1);
        let last = if index == 42 { 164 } else { 165 };
        case["user_functions"]
            .as_object_mut()
            .unwrap()
            .retain(|id, _| id == "100");
        for id in 101..=last {
            case["user_functions"][id.to_string()] = json!({
                "library":"sequence-design", "name":format!("depth-{id}"),
                "parameters":[{"id":1,"name":"item","ty":"int"},
                    {"id":2,"name":"source_position","ty":"int"}],
                "output_name":"value", "output_type":"int",
                "body":{"nodes":if id == last { json!({"1":{"kind":"function_parameter","parameter":1}}) }
                    else { json!({"1":{"kind":"function_parameter","parameter":1},
                        "2":{"kind":"function_parameter","parameter":2},
                        "3":{"kind":"user_function_call","function":id+1,"args":[1,2]}}) }},
                "output":if id == last { 1 } else { 3 }
            });
        }
    }
    if index >= 44 {
        case["evaluation"]["kind"] = match index {
            44 => json!("item_at"),
            45 => json!("aggregate"),
            46 => json!("exists_positive"),
            _ => unreachable!(),
        };
    }
    case
}

fn add_named(project: &mut Project) {
    let mut root = project.root.clone();
    let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(descriptor)) =
        &mut root.children[0].children[0].iteration
    else {
        unreachable!()
    };
    let SequenceExpr::Generate { item, .. } = descriptor.source.as_mut() else {
        unreachable!()
    };
    *item = 12;
    descriptor.item = 13;
    root.children[0].children[0].bindings[0].node = 13;
    project.graph.nodes.insert(
        12,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.graph.nodes.insert(
        13,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.extra_targets.push(mapping::NamedTarget {
        name: "Second".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root,
    });
}

// This suffix calls the public primitive with the unchanged emitted callbacks.
// It is a separate test delegate, never ordinary mapping output telemetry.
fn delegate(case: &Json, index: usize) -> String {
    if index >= 37 || case["evaluation"]["kind"] == "outer_if" {
        return "\npub fn direct_filter_map192<'a>(_: &'a Instance, _: FilterMapLimits, _: Option<&'a dyn FilterMapCancellation>) -> Option<crate::Direct192> { None }\n".into();
    }
    let d = &case["sequence_descriptor"];
    let source = &d["source"];
    let from = source["from"].as_u64();
    let to = source["to"].as_u64().unwrap();
    let predicate = d["predicate"].as_u64().unwrap();
    let mapper = d["mapper"].as_u64().unwrap();
    let item = d["item"].as_u64().unwrap();
    let mut out = String::from(
        "\npub fn direct_filter_map192<'a>(source: &'a Instance, limits: FilterMapLimits, cancellation: Option<&'a dyn FilterMapCancellation>) -> Option<crate::Direct192> {\n    use std::cell::RefCell;\n    let run = FilterMapRun::new(limits, cancellation);\n    let context = ScopeContext::new(source).with_filter_map_run(&run);\n    let parents = context.walk_source(&[\"Parents\"]);\n    let captures = RefCell::new(Vec::new());\n    let kept = RefCell::new(Vec::new());\n    let prefix = RefCell::new(Vec::new());\n    let events = RefCell::new(Vec::new());\n    let mut completed = Vec::new();\n    let mut outcome = Ok(Vec::new());\n",
    );
    if let Some(from) = from {
        out.push_str(&format!("    let from = |context: &ScopeContext<'_>| {{ let result = expression_{from}(context); events.borrow_mut().push(crate::callback_result_original192(\"source-from\", &result)); result }};\n"));
    }
    out.push_str(&format!("    let to = |context: &ScopeContext<'_>| {{ let result = expression_{to}(context); events.borrow_mut().push(crate::callback_result_original192(\"source-to\", &result)); result }};\n"));
    let mut capture_inputs = Vec::new();
    for (i, capture) in d["captures"].as_array().unwrap().iter().enumerate() {
        let node = capture["node"].as_u64().unwrap();
        out.push_str(&format!("    let capture_{i} = |context: &ScopeContext<'_>| {{ let result = expression_{node}(context); events.borrow_mut().push(crate::callback_result_original192(\"capture[{i}]\", &result)); if let Ok(value) = &result {{ captures.borrow_mut().push(value.clone()); }} result }};\n"));
        capture_inputs.push(format!("FilterMapCapture {{ input: FilterMapInput {{ node: {node}, evaluate: &capture_{i} }}, ty: ScalarType::{} }}", scalar_name(capture["ty"].as_str().unwrap())));
    }
    out.push_str(&format!("    let predicate = |context: &ScopeContext<'_>, args: &[Value]| {{ events.borrow_mut().push(crate::callback_arguments_original192(\"predicate\", args)); let result = user_function_{predicate}(context, args); events.borrow_mut().push(crate::callback_result_original192(\"predicate-result\", &result)); result }};\n"));
    out.push_str(&format!("    let mapper = |context: &ScopeContext<'_>, args: &[Value]| {{ events.borrow_mut().push(crate::callback_arguments_original192(\"mapper\", args)); let result = user_function_{mapper}(context, args); events.borrow_mut().push(crate::callback_result_original192(\"mapper-result\", &result)); if let Ok(value) = &result {{ prefix.borrow_mut().push(value.clone()); if let Value::Int(position) = &args[1] {{ kept.borrow_mut().push(*position as usize); }} }} result }};\n"));
    let selection = if case["id"] == "positions-parent-source-dense" {
        "parents.iter().skip(6).take(1)"
    } else {
        "parents.iter().skip(0).take(parents.len())"
    };
    out.push_str(&format!("    for parent in {selection} {{\n        outcome = filter_map_sequence(parent, FilterMapDescriptor {{ item: {item}, from: {}, to: FilterMapInput {{ node: {to}, evaluate: &to }}, captures: &[{}], predicate: FilterMapFunction {{ id: {predicate}, output: USER_FUNCTION_{predicate}_OUTPUT, evaluate: &predicate }}, mapper: FilterMapFunction {{ id: {mapper}, output: USER_FUNCTION_{mapper}_OUTPUT, evaluate: &mapper }}, output_type: ScalarType::{} }});\n        match &outcome {{ Ok(values) => completed.push(values.clone()), Err(_) => break }}\n    }}\n    Some(crate::Direct192 {{ outcome, captures: captures.into_inner(), kept_source_positions: kept.into_inner(), completed_sequences: completed, completed_prefix: prefix.into_inner(), events: events.into_inner(), counters: run.counters() }})\n}}\n", from.map_or("None".into(), |node| format!("Some(FilterMapInput {{ node: {node}, evaluate: &from }})")), capture_inputs.join(", "), scalar_name(d["output_type"].as_str().unwrap())));
    out
}
fn scalar_name(value: &str) -> &'static str {
    match value {
        "int" => "Int",
        "float" => "Float",
        "bool" => "Bool",
        "string" => "String",
        _ => unreachable!(),
    }
}

#[test]
fn compiled_filter_map_public_controls_retain_all_originals_before_comparisons() {
    let output = TempDir::new("rust_filter_map192");
    let directory = output.path().to_path_buf();
    // Preserve every source/process/runtime original on any negative gate.
    std::mem::forget(output);
    fs::create_dir(directory.join("src")).unwrap();
    let evidence = directory.join("originals");
    fs::create_dir(&evidence).unwrap();
    retain(
        &directory,
        "APPROVED39_CORPUS.ORIGINAL.json",
        CORPUS.as_bytes(),
    );
    retain(
        &directory,
        "FROZEN_PUBLIC_EXPECTED.ORIGINAL.json",
        EXPECTED.as_bytes(),
    );
    let corpus: Json = serde_json::from_str(CORPUS).unwrap();
    let cases: Vec<Json> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(corpus["legacy_controls"].as_array().unwrap())
        .cloned()
        .chain((39..47).map(|index| supplementary(&corpus, index)))
        .collect();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let mut controls = Vec::new();
    let mut original_checks = Vec::new();
    let mut modules = String::new();
    let mut calls = String::new();
    for (index, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().unwrap();
        let raw = case
            .get("sequence_descriptor")
            .unwrap_or(&case["descriptor"]);
        let decoded: Result<SequenceExpr, _> = serde_json::from_value(raw.clone());
        retain(
            &directory,
            &format!("{index:02}-{id}-DECODE.ORIGINAL.txt"),
            format!("raw={raw:#?}\nactual={decoded:#?}\n").as_bytes(),
        );
        let descriptor = match decoded {
            Ok(descriptor) => descriptor,
            Err(error) => {
                original_checks.push((
                    id.to_owned(),
                    id == "unknown-new-field-no-silent-fallback"
                        && error.to_string().contains("unknown field `stream`"),
                ));
                continue;
            }
        };
        if let Some(expected_roundtrip) = case.get("expected_roundtrip_descriptor") {
            let roundtrip = serde_json::to_value(&descriptor).unwrap();
            retain(
                &directory,
                &format!("{index:02}-{id}-ROUNDTRIP.ORIGINAL.txt"),
                format!("actual={roundtrip:#?}\nexpected={expected_roundtrip:#?}\n").as_bytes(),
            );
            original_checks.push((
                format!("{id}-legacy-roundtrip"),
                roundtrip == *expected_roundtrip,
            ));
        }
        let mut project = project(case, descriptor);
        if (39..=41).contains(&index) {
            add_named(&mut project);
        }
        let admission = project.validate_filter_map_v1();
        let lowered = codegen::lower(&project);
        retain(&directory, &format!("{index:02}-{id}-PROJECT_AND_PROGRAM.ORIGINAL.txt"),
            format!("project-json={}\nproject={project:#?}\nadmission={admission:#?}\nlowered={lowered:#?}\n",
                serde_json::to_string(&project).unwrap()).as_bytes());
        if let Some(error) = static_expected(case, &project) {
            retain(
                &directory,
                &format!("{index:02}-{id}-EXPECTED_ADMISSION.ORIGINAL.txt"),
                format!("{error:#?}\n").as_bytes(),
            );
            original_checks.push((id.to_owned(), admission == Err(error) && lowered.is_err()));
            continue;
        }
        let program = lowered.expect("admitted literal lowers; complete original retained");
        let artifacts = crate::emit(
            &program,
            &crate::Options {
                package_name: "filter-map192".into(),
                runtime_dependency: crate::RuntimeDependency::Path(runtime.display().to_string()),
            },
        );
        retain(
            &directory,
            &format!("{index:02}-{id}-ARTIFACTS.ORIGINAL.txt"),
            format!("{artifacts:#?}\n").as_bytes(),
        );
        let artifacts = artifacts.expect("admitted Rust literal emits; complete original retained");
        let source = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap();
        retain(
            &directory,
            &format!("{index:02}-{id}-EXACT_PRODUCTION_SOURCE.ORIGINAL.rs"),
            &source.contents,
        );
        let suffix = delegate(case, index);
        retain(
            &directory,
            &format!("{index:02}-{id}-TEST_DELEGATE_SUFFIX.ORIGINAL.rs"),
            suffix.as_bytes(),
        );
        let mut compiled = source.contents.clone();
        compiled.extend_from_slice(suffix.as_bytes());
        retain(
            &directory.join("src"),
            &format!("case_{index:02}.rs"),
            &compiled,
        );
        original_checks.push((
            format!("{id}-compiled-prefix"),
            compiled.starts_with(&source.contents),
        ));
        modules.push_str(&format!("pub mod case_{index:02};\n"));
        calls.push_str(&format!(
            "    qualify_module!(case_{index:02}, &controls[{index}], &mut originals);\n"
        ));
        controls.push((index, id.to_owned()));
    }
    let manifest = format!(
        "[package]\nname = \"filter-map192-host\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\ncodegen-runtime = {{ path = {:?} }}\nserde_json = \"1\"\n",
        runtime.display().to_string()
    );
    retain(&directory, "Cargo.toml", manifest.as_bytes());
    retain(&directory, "controls.json", EXPECTED.as_bytes());
    let host = format!(
        "{modules}\n{HOST}\nfn main() {{\n    let controls: serde_json::Value = serde_json::from_str(include_str!(\"../controls.json\")).unwrap();\n    let controls = controls[\"controls\"].as_array().unwrap();\n    let mut originals = Vec::new();\n{calls}    finish(originals);\n}}\n"
    );
    retain(&directory.join("src"), "main.rs", host.as_bytes());
    retain(
        &directory,
        "COMPILED_CONTROL_INDEX.ORIGINAL.txt",
        format!("{controls:#?}\n").as_bytes(),
    );
    let launched = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings")
        .env("FERRULE_FILTER_MAP192_HOST_EVIDENCE", &evidence)
        .current_dir(&directory)
        .generated_host_output(&directory);
    retain(
        &directory,
        "WHOLE_HOST_PROCESS_RESULT.ORIGINAL.txt",
        format!("{launched:#?}\n").as_bytes(),
    );
    if let Ok(original) = &launched {
        retain(
            &directory,
            "WHOLE_HOST_STDOUT.ORIGINAL.bin",
            &original.stdout,
        );
        retain(
            &directory,
            "WHOLE_HOST_STDERR.ORIGINAL.bin",
            &original.stderr,
        );
    }
    retain(
        &directory,
        "STATIC_COMPARISONS.ORIGINAL.txt",
        format!("{original_checks:#?}\n").as_bytes(),
    );
    let original = launched.expect("host launch failed; complete result retained");
    assert!(
        original_checks.iter().all(|(_, equal)| *equal),
        "static literal controls disagree: {original_checks:#?}"
    );
    assert!(
        original.status.success(),
        "compiled filter/map host failed; all originals at {}",
        directory.display()
    );
    assert_eq!(
        controls.len(),
        40,
        "31 admitted new literals, two legacy, three named outputs, depth64 and three reducers"
    );
    if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_none() {
        fs::remove_dir_all(&directory).unwrap();
    }
}
