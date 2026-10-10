use super::*;
use egui_snarl::ui::SnarlViewer as _;
use ir::{Instance, ScalarType, SchemaNode, Value, XmlTypeOrigin};
use mapping::{
    Binding, FilterMapCapture, FilterMapV1, FunctionParameter, FunctionParameterId, NamedTarget,
};
use serde_json::{Value as Json, json};
use std::io::Write;

struct Originals(std::path::PathBuf);

impl Originals {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-sequence-duplication-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn bytes(&self, name: &str, bytes: &[u8]) {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.0.join(name))
            .unwrap();
        file.write_all(bytes).unwrap();
        file.flush().unwrap();
    }

    fn debug(&self, name: &str, value: &impl std::fmt::Debug) {
        self.bytes(name, format!("{value:#?}\n").as_bytes());
    }

    fn frame(&self, name: &str, output: &egui::FullOutput) {
        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = output;
        let egui::PlatformOutput {
            commands,
            cursor_icon,
            cursor_image,
            events,
            mutable_text_under_cursor,
            ime,
            accesskit_update,
            num_completed_passes,
            request_discard_reasons,
        } = platform_output;
        let mut record = format!(
            "FullOutput {{\n  platform_output: PlatformOutput {{\n    commands: {commands:#?},\n    cursor_icon: {cursor_icon:#?},\n    cursor_image: {cursor_image:#?},\n    events: {events:#?},\n    mutable_text_under_cursor: {mutable_text_under_cursor:#?},\n    ime: {ime:#?},\n    accesskit_update: {accesskit_update:#?},\n    num_completed_passes: {num_completed_passes:#?},\n    request_discard_reasons: {request_discard_reasons:#?},\n  }},\n  textures_delta: {textures_delta:#?},\n  shapes: {shapes:#?},\n  pixels_per_point: {pixels_per_point:#?},\n  viewport_output: [\n"
        );
        for (id, viewport) in viewport_output {
            let egui::ViewportOutput {
                parent,
                class,
                builder,
                viewport_ui_cb,
                commands,
                repaint_delay,
            } = viewport;
            // Keyboard history uses immediate viewports; an opaque deferred callback
            // must fail capture rather than disappear from the retained output.
            assert!(
                viewport_ui_cb.is_none(),
                "unexpected deferred viewport callback"
            );
            let class = match class {
                egui::ViewportClass::Root => "Root",
                egui::ViewportClass::Deferred => "Deferred",
                egui::ViewportClass::Immediate => "Immediate",
                egui::ViewportClass::EmbeddedWindow => "EmbeddedWindow",
            };
            record.push_str(&format!(
                "    ({id:#?}, ViewportOutput {{\n      parent: {parent:#?},\n      class: {class},\n      builder: {builder:#?},\n      viewport_ui_cb: None,\n      commands: {commands:#?},\n      repaint_delay: {repaint_delay:#?},\n    }}),\n"
            ));
        }
        record.push_str("  ],\n}\n");
        self.bytes(name, record.as_bytes());
    }
}

impl Drop for Originals {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "Retained complete sequence duplication originals: {}",
                self.0.display()
            );
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn literals(originals: &Originals) -> Json {
    let bytes = include_bytes!("sequence_consumer_duplication_literals.json");
    originals.bytes("complete-frozen-literals.original.json", bytes);
    serde_json::from_slice(bytes).unwrap()
}

fn text(value: &Json) -> &str {
    value.as_str().unwrap()
}
fn id(value: &Json) -> NodeId {
    value.as_u64().unwrap().try_into().unwrap()
}
fn optional_id(value: &Json) -> Option<NodeId> {
    (!value.is_null()).then(|| id(value))
}
fn strings(value: &Json) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| text(value).to_owned())
        .collect()
}
fn ty(value: &Json) -> ScalarType {
    match text(value) {
        "Bool" => ScalarType::Bool,
        "Int" => ScalarType::Int,
        "Float" => ScalarType::Float,
        "String" => ScalarType::String,
        other => panic!("unfrozen scalar type {other}"),
    }
}

fn scalar(value: &Json) -> Value {
    match text(&value["type"]) {
        "Null" => Value::Null,
        "JsonNull" => Value::JsonNull(ir::JsonNull),
        "XmlNil" => Value::XmlNil(ir::XmlNil),
        "Bool" => Value::Bool(value["value"].as_bool().unwrap()),
        "Int" => Value::Int(value["value"].as_i64().unwrap()),
        "String" => Value::String(text(&value["value"]).into()),
        "Float" => Value::Float(f64::from_bits(
            u64::from_str_radix(text(&value["binary64_bits_hex"]), 16).unwrap(),
        )),
        other => panic!("unfrozen value {other}"),
    }
}

fn schema(value: &Json) -> SchemaNode {
    let args = value["arguments"].as_array().unwrap();
    let mut node = match text(&value["constructor"]) {
        "ir::SchemaNode::group" => SchemaNode::group(
            text(&args[0]),
            args[1].as_array().unwrap().iter().map(schema).collect(),
        ),
        "ir::SchemaNode::scalar" => SchemaNode::scalar(text(&args[0]), ty(&args[1])),
        other => panic!("unfrozen constructor {other}"),
    };
    for (key, value) in value["post_constructor_overrides"].as_object().unwrap() {
        match key.as_str() {
            "repeating" => node.repeating = value.as_bool().unwrap(),
            other => panic!("unfrozen schema field {other}"),
        }
    }
    node
}

fn options(value: &Json) -> mapping::FormatOptions {
    match (
        text(&value["constructor"]),
        value["post_constructor_overrides"]
            .as_object()
            .unwrap()
            .len(),
    ) {
        ("mapping::FormatOptions::default", 0) => Default::default(),
        _ => panic!("unfrozen format options"),
    }
}

fn sequence(value: &Json) -> SequenceExpr {
    match text(&value["variant"]) {
        "Generate" => SequenceExpr::Generate {
            from: optional_id(&value["from"]),
            to: id(&value["to"]),
            item: id(&value["item"]),
        },
        "FilterMapV1" => SequenceExpr::FilterMapV1(FilterMapV1 {
            source: Box::new(sequence(&value["source"])),
            item: id(&value["item"]),
            predicate: FunctionId::new(value["predicate"].as_u64().unwrap()),
            mapper: FunctionId::new(value["mapper"].as_u64().unwrap()),
            output_type: ty(&value["output_type"]),
            captures: value["captures"]
                .as_array()
                .unwrap()
                .iter()
                .map(|capture| FilterMapCapture {
                    node: id(&capture["node"]),
                    ty: ty(&capture["ty"]),
                })
                .collect(),
        }),
        "Tokenize" => SequenceExpr::Tokenize {
            input: id(&value["input"]),
            delimiter: id(&value["delimiter"]),
            item: id(&value["item"]),
        },
        other => panic!("unfrozen sequence {other}"),
    }
}

fn node(value: &Json) -> Node {
    let args = || value["args"].as_array().unwrap().iter().map(id).collect();
    match text(&value["variant"]) {
        "Const" => Node::Const {
            value: scalar(&value["value"]),
        },
        "SourceField" => Node::SourceField {
            path: strings(&value["path"]),
            frame: (!value["frame"].is_null()).then(|| strings(&value["frame"])),
        },
        "Position" => Node::Position {
            collection: strings(&value["collection"]),
        },
        "Call" => Node::Call {
            function: text(&value["function"]).into(),
            args: args(),
        },
        "UserFunctionCall" => Node::UserFunctionCall {
            function: FunctionId::new(value["function"].as_u64().unwrap()),
            args: args(),
        },
        "FunctionParameter" => Node::FunctionParameter {
            parameter: FunctionParameterId::new(value["parameter"].as_u64().unwrap()),
        },
        "Raise" => Node::Raise {
            message: optional_id(&value["message"]),
        },
        "SequenceExists" => Node::SequenceExists {
            sequence: sequence(&value["sequence"]),
            predicate: id(&value["predicate"]),
        },
        "SequenceItemAt" => Node::SequenceItemAt {
            sequence: sequence(&value["sequence"]),
            index: id(&value["index"]),
        },
        "SequenceAggregate" => Node::SequenceAggregate {
            function: match text(&value["function"]) {
                "Sum" => mapping::AggregateOp::Sum,
                other => panic!("unfrozen aggregate {other}"),
            },
            sequence: sequence(&value["sequence"]),
            predicate: optional_id(&value["predicate"]),
            expression: optional_id(&value["expression"]),
            arg: optional_id(&value["arg"]),
        },
        other => panic!("unfrozen graph node {other}"),
    }
}

fn graph(value: &Json) -> Graph {
    Graph {
        nodes: value["nodes"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.parse().unwrap(), node(value)))
            .collect(),
    }
}

fn scope(value: &Json) -> Scope {
    Scope {
        target_field: text(&value["target_field"]).into(),
        iteration: match text(&value["iteration"]["variant"]) {
            "None" => ScopeIteration::None,
            "Source" => ScopeIteration::Source(strings(&value["iteration"]["collection"])),
            "Sequence" => ScopeIteration::Sequence(sequence(&value["iteration"]["sequence"])),
            other => panic!("unfrozen scope iteration {other}"),
        },
        construction: match text(&value["construction"]["variant"]) {
            "Constructed" => ScopeConstruction::Constructed,
            other => panic!("unfrozen construction {other}"),
        },
        filter: optional_id(&value["filter"]),
        post_group_filter: optional_id(&value["post_group_filter"]),
        group_by: optional_id(&value["group_by"]),
        group_adjacent_by: optional_id(&value["group_adjacent_by"]),
        group_starting_with: optional_id(&value["group_starting_with"]),
        group_ending_with: optional_id(&value["group_ending_with"]),
        group_into_blocks: optional_id(&value["group_into_blocks"]),
        sort_by: optional_id(&value["sort_by"]),
        sort_descending: value["sort_descending"].as_bool().unwrap(),
        sort_then_by: value["sort_then_by"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::from_value(value.clone()).unwrap())
            .collect(),
        sort_filter_order: match text(&value["sort_filter_order"]) {
            "SortThenFilter" => mapping::SortFilterOrder::SortThenFilter,
            other => panic!("unfrozen sort policy {other}"),
        },
        windows: value["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::from_value(value.clone()).unwrap())
            .collect(),
        iteration_output: match text(&value["iteration_output"]) {
            "Repeated" => mapping::IterationOutput::Repeated,
            other => panic!("unfrozen cardinality {other}"),
        },
        bindings: value["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|binding| Binding {
                target_field: text(&binding["target_field"]).into(),
                node: id(&binding["node"]),
            })
            .collect(),
        dynamic_bindings: value["dynamic_bindings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::from_value(value.clone()).unwrap())
            .collect(),
        children: value["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(scope)
            .collect(),
        dynamic_children: value["dynamic_children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::from_value(value.clone()).unwrap())
            .collect(),
        merge_dynamic_fields: value["merge_dynamic_fields"].as_bool().unwrap(),
    }
}

fn project(value: &Json) -> Project {
    Project {
        source: schema(&value["source"]),
        target: schema(&value["target"]),
        source_path: value["source_path"].as_str().map(str::to_owned),
        target_path: value["target_path"].as_str().map(str::to_owned),
        source_options: options(&value["source_options"]),
        target_options: options(&value["target_options"]),
        extra_sources: value["extra_sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::from_value(value.clone()).unwrap())
            .collect(),
        extra_targets: value["extra_targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| NamedTarget {
                name: text(&value["name"]).into(),
                path: value["path"].as_str().map(str::to_owned),
                schema: schema(&value["schema"]),
                options: options(&value["options"]),
                root: scope(&value["root"]),
            })
            .collect(),
        failure_rules: value["failure_rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| serde_json::from_value(value.clone()).unwrap())
            .collect(),
        user_functions: value["user_functions"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| {
                (
                    FunctionId::new(key.parse().unwrap()),
                    UserFunction {
                        library: text(&value["library"]).into(),
                        name: text(&value["name"]).into(),
                        description: value["description"].as_str().map(str::to_owned),
                        parameters: value["parameters"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|parameter| FunctionParameter {
                                id: FunctionParameterId::new(parameter["id"].as_u64().unwrap()),
                                name: text(&parameter["name"]).into(),
                                ty: ty(&parameter["ty"]),
                            })
                            .collect(),
                        output_name: text(&value["output_name"]).into(),
                        output_type: ty(&value["output_type"]),
                        body: graph(&value["body"]),
                        output: id(&value["output"]),
                    },
                )
            })
            .collect(),
        graph: graph(&value["graph"]),
        root: scope(&value["root"]),
    }
}

fn coordinate(value: &Json) -> f32 {
    if let Some(number) = value.as_f64() {
        number as f32
    } else {
        f32::from_bits(u32::from_str_radix(text(&value["binary32_bits_hex"]), 16).unwrap())
    }
}

fn layout_nodes(value: &Json) -> Vec<CanvasNodeLayout> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| CanvasNodeLayout {
            node: serde_json::from_value(value["node"].clone()).unwrap(),
            x: coordinate(&value["x"]),
            y: coordinate(&value["y"]),
        })
        .collect()
}

fn layout(value: &Json, project: &Project) -> CanvasLayout {
    CanvasLayout {
        version: value["version"].as_u64().unwrap().try_into().unwrap(),
        project_fingerprint: Some(project_fingerprint(project)),
        nodes: layout_nodes(&value["nodes"]),
        function_nodes: value["function_nodes"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, nodes)| (FunctionId::new(id.parse().unwrap()), layout_nodes(nodes)))
            .collect(),
        target_nodes: value["target_nodes"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, nodes)| (id.parse().unwrap(), layout_nodes(nodes)))
            .collect(),
        open_functions: value["open_functions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| FunctionId::new(id.as_u64().unwrap()))
            .collect(),
        open_documents: serde_json::from_value(value["open_documents"].clone()).unwrap(),
        active_function: value["active_function"].as_u64().map(FunctionId::new),
        active_document: serde_json::from_value(value["active_document"].clone()).unwrap(),
    }
}

fn canvas_node(value: &Json) -> CanvasNode {
    match text(&value["kind"]) {
        "source" => CanvasNode::SourceBlock(value["block"].as_u64().unwrap().try_into().unwrap()),
        "target" => CanvasNode::TargetBlock(value["block"].as_u64().unwrap().try_into().unwrap()),
        "graph" => CanvasNode::Graph(id(&value["id"])),
        "placeholder" => CanvasNode::Placeholder(id(&value["id"])),
        other => panic!("unfrozen canvas node {other}"),
    }
}

fn canvas_recipe(value: &Json, positions: &[CanvasNodeLayout]) -> Snarl<CanvasNode> {
    let mut snarl = Snarl::new();
    let mut ids = BTreeMap::new();
    for position in positions {
        let identity = serde_json::to_value(position.node).unwrap();
        let node = canvas_node(&identity);
        let open = value["open_by_stable_node_identity"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["node"] == identity)
            .unwrap()["open"]
            .as_bool()
            .unwrap();
        let position = egui::pos2(position.x, position.y);
        let id = if open {
            snarl.insert_node(position, node)
        } else {
            snarl.insert_node_collapsed(position, node)
        };
        ids.insert(node, id);
    }
    for wire in value["wires"].as_array().unwrap() {
        snarl.connect(
            OutPinId {
                node: ids[&canvas_node(&wire["from"])],
                output: wire["from"]["output"].as_u64().unwrap().try_into().unwrap(),
            },
            InPinId {
                node: ids[&canvas_node(&wire["to"])],
                input: wire["to"]["input"].as_u64().unwrap().try_into().unwrap(),
            },
        );
    }
    snarl
}

fn prepared(snapshot: &Json, refused: bool) -> FerruleApp {
    let project = project(&snapshot["project"]);
    let layout = layout(&snapshot["layout"], &project);
    let main = snapshot
        .get("mapping_canvases")
        .map_or(&snapshot["canvas"], |canvases| &canvases["Main"]);
    let mut app = FerruleApp {
        project,
        ..FerruleApp::default()
    };
    app.main_canvas = CanvasDocumentState::with_snarl(canvas_recipe(main, &layout.nodes));
    app.mapping_workspace = MappingWorkspace::from_layout(&app.project, Some(&layout));
    for (&index, positions) in &layout.target_nodes {
        let table = &snapshot["mapping_canvases"][format!("Target({index})")];
        app.mapping_workspace.target_canvases.insert(
            index,
            CanvasDocumentState::with_snarl(canvas_recipe(table, positions)),
        );
    }
    app.mark_clean();
    let before = editor_snapshot(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    app.history = SnapshotHistory::new(before.clone(), DocumentOrigin::Saved);
    app.observed_editor = before;
    app.pending_history = None;
    if refused {
        let before = app.observed_editor.clone();
        app.history.record(before.clone(), "Retained prior edit");
        app.history.record(before.clone(), "Retained redo");
        let _ = app.history.undo(before);
    }
    app
}

fn stable_node(value: CanvasNode) -> Json {
    match value {
        CanvasNode::SourceBlock(block) => json!({"kind":"source","block":block}),
        CanvasNode::TargetBlock(block) => json!({"kind":"target","block":block}),
        CanvasNode::Graph(id) => json!({"kind":"graph","id":id}),
        CanvasNode::Placeholder(id) => json!({"kind":"placeholder","id":id}),
    }
}

fn sorted(mut values: Vec<Json>) -> Vec<Json> {
    values.sort_by_key(Json::to_string);
    values
}

fn canvas_table(canvas: &CanvasDocumentState, project: &Project, target: &SchemaNode) -> Json {
    let wires = canvas
        .snarl
        .wires()
        .map(|(from, to)| {
            let mut source = stable_node(canvas.snarl[from.node]);
            source["output"] = json!(from.output);
            let mut target = stable_node(canvas.snarl[to.node]);
            target["input"] = json!(to.input);
            json!({"from":source,"to":target})
        })
        .collect();
    let open = canvas.snarl.node_ids().map(|(id, node)| json!({"node":stable_node(*node),"open":canvas.snarl.get_node_info(id).unwrap().open})).collect();
    let source_scroll = source_blocks(&project.source)
        .iter()
        .enumerate()
        .map(|(block, section)| {
            canvas
                .endpoint_scroll
                .offset(CanvasNode::SourceBlock(block), section.leaves.len())
        })
        .collect::<Vec<_>>();
    let target_scroll = target_blocks(target)
        .iter()
        .enumerate()
        .map(|(block, section)| {
            canvas
                .endpoint_scroll
                .offset(CanvasNode::TargetBlock(block), section.leaves.len())
        })
        .collect::<Vec<_>>();
    // Literal fixtures construct no Snarl UI frame or selected-node Context
    // state. Physical selection preservation remains a separate root UI gate.
    json!({"wires":sorted(wires),"selected":[],"open_by_stable_node_identity":sorted(open),"endpoint_scroll":{"source_scroll":source_scroll,"target_scroll":target_scroll}})
}

fn expected_canvas(value: &Json) -> Json {
    let mut value = value.clone();
    value["wires"] = json!(sorted(value["wires"].as_array().unwrap().clone()));
    value["open_by_stable_node_identity"] = json!(sorted(
        value["open_by_stable_node_identity"]
            .as_array()
            .unwrap()
            .clone()
    ));
    value
}

fn layout_bits(layout: &CanvasLayout) -> Json {
    let mut value = serde_json::to_value(layout).unwrap();
    let replace = |values: &mut Json, originals: &[CanvasNodeLayout]| {
        for (entry, original) in values.as_array_mut().unwrap().iter_mut().zip(originals) {
            entry["x"] = json!({"binary32_bits_hex":format!("{:08x}",original.x.to_bits())});
            entry["y"] = json!({"binary32_bits_hex":format!("{:08x}",original.y.to_bits())});
        }
    };
    replace(&mut value["nodes"], &layout.nodes);
    for (id, nodes) in &layout.function_nodes {
        replace(&mut value["function_nodes"][id.get().to_string()], nodes);
    }
    for (id, nodes) in &layout.target_nodes {
        replace(&mut value["target_nodes"][id.to_string()], nodes);
    }
    value
}

fn value_original(value: &Value) -> Json {
    match value {
        Value::Null => json!({"type":"Null"}),
        Value::JsonNull(_) => json!({"type":"JsonNull"}),
        Value::XmlNil(_) => json!({"type":"XmlNil"}),
        Value::Bool(value) => json!({"type":"Bool","value":value}),
        Value::Int(value) => json!({"type":"Int","value":value}),
        Value::String(value) => json!({"type":"String","value":value}),
        Value::Float(value) => {
            json!({"type":"Float","binary64_bits_hex":format!("{:016x}",value.to_bits())})
        }
    }
}

fn instance_original(value: &Instance) -> Json {
    match value {
        Instance::Scalar(value) => json!({"variant":"Scalar","value":value_original(value)}),
        Instance::Group(fields) => {
            let origin = match fields.xml_type_origin() {
                XmlTypeOrigin::Unknown => json!({"tag":"Unknown"}),
                XmlTypeOrigin::Absent => json!({"tag":"Absent"}),
                XmlTypeOrigin::Explicit(identity) => json!({"tag":"Explicit","identity":identity}),
                XmlTypeOrigin::ExplicitPadded {
                    literal,
                    resolved_identity,
                } => {
                    json!({"tag":"ExplicitPadded","literal":literal,"resolved_identity":resolved_identity})
                }
            };
            json!({"variant":"Group","xml_type_origin":origin,"ordered_fields":fields.iter().map(|(name,value)| json!([name,instance_original(value)])).collect::<Vec<_>>()})
        }
        Instance::Repeated(values) => {
            json!({"variant":"Repeated","items":values.iter().map(instance_original).collect::<Vec<_>>()})
        }
        Instance::MappedSequence(values) => {
            json!({"variant":"MappedSequence","items":values.iter().map(instance_original).collect::<Vec<_>>()})
        }
        Instance::DocumentSet(values) => {
            json!({"variant":"DocumentSet","documents":values.iter().map(|value| json!({"path":value.path(),"source_path":value.source_path(),"value":instance_original(value.value())})).collect::<Vec<_>>()})
        }
    }
}

fn error_original(error: &engine::EngineError) -> Json {
    match error {
        engine::EngineError::MappingException { node, message } => {
            json!({"variant":"MappingException","node":node,"message":message,"display":error.to_string()})
        }
        engine::EngineError::FilterMapRuntime { boundary, source } => {
            json!({"variant":"FilterMapRuntime","boundary":format!("{boundary:#?}"),"source":error_original(source),"display":error.to_string()})
        }
        engine::EngineError::FilterMapValueType { expected, found } => {
            json!({"variant":"FilterMapValueType","expected":format!("{expected:?}"),"found":value_original(found),"display":error.to_string()})
        }
        _ => json!({"debug":format!("{error:#?}"),"display":error.to_string()}),
    }
}

fn retain_state(originals: &Originals, label: &str, app: &FerruleApp) -> Json {
    let project_key = crate::project_state::project_snapshot_key(&app.project);
    let layout =
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    originals.debug(&format!("{label}.whole-project.original.txt"), &app.project);
    originals.bytes(
        &format!("{label}.project-key.original.txt"),
        project_key.as_bytes(),
    );
    originals.debug(
        &format!("{label}.codec-pretty.original.txt"),
        &mapping::project_file::encode_pretty(&app.project),
    );
    originals.debug(
        &format!("{label}.filter-map-admission.original.txt"),
        &app.project.validate_filter_map_v1(),
    );
    originals.debug(
        &format!("{label}.whole-main-snarl.original.txt"),
        &app.main_canvas.snarl,
    );
    originals.debug(&format!("{label}.whole-layout.original.txt"), &layout);
    let mut canvases = serde_json::Map::new();
    canvases.insert(
        "Main".into(),
        canvas_table(&app.main_canvas, &app.project, &app.project.target),
    );
    for (&index, canvas) in &app.mapping_workspace.target_canvases {
        originals.debug(
            &format!("{label}.whole-target-{index}-snarl.original.txt"),
            &canvas.snarl,
        );
        canvases.insert(
            format!("Target({index})"),
            canvas_table(
                canvas,
                &app.project,
                &app.project.extra_targets[index].schema,
            ),
        );
    }
    let mut float_values = Vec::new();
    for (&id, node) in &app.project.graph.nodes {
        if let Node::Const { value } = node {
            float_values.push(json!({"graph_node":id,"value":value_original(value)}));
        }
    }
    for (&function, definition) in &app.project.user_functions {
        for (&id, node) in &definition.body.nodes {
            if let Node::Const { value } = node {
                float_values.push(
                    json!({"function":function.get(),"body_node":id,"value":value_original(value)}),
                );
            }
        }
    }
    let result = json!({"project_key":project_key,"layout":layout_bits(&layout),"mapping_canvases":canvases,"typed_literals":float_values,"history":{"undo":app.history.undo_len(),"redo":app.history.redo_len(),"dirty":app.history.is_dirty(&app.observed_editor),"pending":app.pending_history.as_ref().map(|value| json!({"project_key":value.before.state.serialized_project,"layout":layout_bits(&value.before.state.layout)})),"observed_project_key":app.observed_editor.state.serialized_project,"observed_layout":layout_bits(&app.observed_editor.state.layout)}});
    originals.bytes(
        &format!("{label}.complete-state.original.json"),
        &serde_json::to_vec_pretty(&result).unwrap(),
    );
    result
}

fn assert_snapshot(originals: &Originals, label: &str, app: &FerruleApp, expected: &Json) {
    let actual = retain_state(originals, label, app);
    let project = project(&expected["project"]);
    let layout = layout(&expected["layout"], &project);
    originals.debug(
        &format!("{label}.whole-expected-project.original.txt"),
        &project,
    );
    originals.debug(
        &format!("{label}.whole-expected-layout.original.txt"),
        &layout,
    );
    assert_eq!(
        actual["project_key"],
        crate::project_state::project_snapshot_key(&project),
        "{label}"
    );
    assert_eq!(actual["layout"], layout_bits(&layout), "{label}");
    let main = expected
        .get("mapping_canvases")
        .map_or(&expected["canvas"], |canvases| &canvases["Main"]);
    assert_eq!(
        actual["mapping_canvases"]["Main"],
        expected_canvas(main),
        "{label}"
    );
    for &index in app.mapping_workspace.target_canvases.keys() {
        assert_eq!(
            actual["mapping_canvases"][format!("Target({index})")],
            expected_canvas(&expected["mapping_canvases"][format!("Target({index})")]),
            "{label}"
        );
    }
}

fn preview(originals: &Originals, label: &str, app: &FerruleApp, expected: &Json) {
    let input = Instance::Group(Vec::new().into());
    originals.debug(&format!("{label}.native-input.original.txt"), &input);
    originals.bytes(
        &format!("{label}.native-input-typed.original.json"),
        &serde_json::to_vec_pretty(&instance_original(&input)).unwrap(),
    );
    let outcome = engine::run(&app.project, &input);
    originals.debug(
        &format!("{label}.native-whole-outcome.original.txt"),
        &outcome,
    );
    let actual = match &outcome {
        Ok(value) => json!({"Ok":instance_original(value)}),
        Err(error) => json!({"Err":error_original(error)}),
    };
    originals.bytes(
        &format!("{label}.native-complete-outcome.original.json"),
        &serde_json::to_vec_pretty(&actual).unwrap(),
    );
    let named = expected.get("named").map(|expected| {
        let outcome = engine::run_selected_target(&app.project, &input, engine::TargetSelection::Named("Mirror"));
        originals.debug(&format!("{label}.named-native-whole-outcome.original.txt"), &outcome);
        let actual = match &outcome {
            Ok(engine::SelectedTargetOutput::Named(value)) => json!({"Ok":{"variant":"SelectedTargetOutput::Named","name":value.name,"instance":instance_original(&value.instance)}}),
            Ok(engine::SelectedTargetOutput::Primary(value)) => json!({"Ok":{"variant":"SelectedTargetOutput::Primary","instance":instance_original(value)}}),
            Err(error) => json!({"Err":error_original(error)}),
        };
        originals.bytes(&format!("{label}.named-native-complete-outcome.original.json"), &serde_json::to_vec_pretty(&actual).unwrap());
        (actual, expected)
    });
    let wanted = expected.get("primary").unwrap_or(expected);
    assert_eq!(actual, *wanted, "{label}");
    if let Some((actual, expected)) = named {
        assert_eq!(actual, *expected, "{label}");
    }
}

fn refusal_original(error: &DuplicationError) -> Json {
    let value = match error {
        DuplicationError::UnavailableContext { context } => {
            json!({"variant":"UnavailableContext","context":context})
        }
        DuplicationError::MissingNode { node, role } => {
            json!({"variant":"MissingNode","node":node,"role":role})
        }
        DuplicationError::UnsupportedSequence { kind } => {
            json!({"variant":"UnsupportedSequence","kind":kind})
        }
        DuplicationError::DuplicatePrivateOwner { item, count } => {
            json!({"variant":"DuplicatePrivateOwner","item":item,"count":count})
        }
        DuplicationError::InvalidPrivateOwner { item } => {
            json!({"variant":"InvalidPrivateOwner","item":item})
        }
        DuplicationError::PrivateOwnerOutsideContext {
            expression,
            item,
            consumer,
            context,
        } => {
            json!({"variant":"PrivateOwnerOutsideContext","expression":expression,"item":item,"consumer":consumer,"context":context})
        }
        DuplicationError::ExpressionCycle { path } => {
            json!({"variant":"ExpressionCycle","path":path})
        }
        DuplicationError::AmbiguousPrivateContext { node, owners } => {
            json!({"variant":"AmbiguousPrivateContext","node":node,"owners":owners})
        }
        DuplicationError::IdExhaustion { message, required } => {
            json!({"variant":"IdExhaustion","message":message,"required":required})
        }
        DuplicationError::MissingCanvasFrame { canvas, node } => {
            json!({"variant":"MissingCanvasFrame","canvas":canvas,"node":node})
        }
        DuplicationError::InvalidFramePosition { node, axis, bits } => {
            json!({"variant":"InvalidFramePosition","node":node,"axis":axis,"binary32_bits_hex":format!("{bits:08x}")})
        }
        DuplicationError::AmbiguousParentContext {
            node,
            consumer,
            source_frames,
        } => {
            json!({"variant":"AmbiguousParentContext","node":node,"consumer":consumer,"source_frames":source_frames})
        }
        DuplicationError::FilterMapAdmission(error) => {
            let kind = match &error.kind {
                mapping::FilterMapAdmissionKind::PrivateCapture {
                    capture,
                    node,
                    item,
                } => json!({"PrivateCapture":{"capture":capture,"node":node,"item":item}}),
                mapping::FilterMapAdmissionKind::MissingFunction { function } => {
                    json!({"MissingFunction":{"function":function.get()}})
                }
                other => json!({"unexpected":format!("{other:#?}")}),
            };
            json!({"variant":"FilterMapAdmission","item":error.item,"location":error.location,"kind":kind})
        }
        other => json!({"unexpected":format!("{other:#?}")}),
    };
    json!({"Err":value})
}

fn document(value: &Json) -> MappingDocument {
    match text(&value["kind"]) {
        "Main" => MappingDocument::Main,
        "Target" | "Named" => {
            MappingDocument::Target(value["index"].as_u64().unwrap().try_into().unwrap())
        }
        "Function" => MappingDocument::Function(FunctionId::new(value["id"].as_u64().unwrap())),
        other => panic!("unfrozen context {other}"),
    }
}

fn explicit_binding(
    app: &mut FerruleApp,
    copied: NodeId,
    document: MappingDocument,
) -> Result<(), String> {
    let context = app.project.clone();
    let target = match document {
        MappingDocument::Main => &context.target,
        MappingDocument::Target(index) => &context.extra_targets[index].schema,
        MappingDocument::Function(_) => unreachable!(),
    };
    let source_blocks = source_blocks(&context.source);
    let target_blocks = target_blocks(target);
    let source_paths = SourcePathCatalog::new(&context.source, &context.extra_sources)
        .with_primary_source_options(&context.source_options);
    let inactive = match document {
        MappingDocument::Main => Vec::new(),
        MappingDocument::Target(index) => {
            let (before, current_and_after) = context.extra_targets.split_at(index);
            let (_, after) = current_and_after.split_first().unwrap();
            crate::graph_viewer::inactive_target_scopes(&context.root, before, after)
        }
        MappingDocument::Function(_) => unreachable!(),
    };
    let function_names = app.function_names();
    let function_inputs = app.function_inputs();
    let primary_root_authoring = matches!(document, MappingDocument::Main)
        && crate::primary_root_authoring::available(&app.project);
    let colors = app.appearance.resolved_colors(app.palette);
    let wire_color_mode = app.appearance.wire().color_mode();
    let canvas = match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(index) => app
            .mapping_workspace
            .target_canvases
            .get_mut(&index)
            .unwrap(),
        MappingDocument::Function(_) => unreachable!(),
    };
    let from = canvas
        .snarl
        .node_ids()
        .find_map(|(id, node)| (*node == CanvasNode::Graph(copied)).then_some(id))
        .unwrap();
    let to = canvas
        .snarl
        .node_ids()
        .find_map(|(id, node)| (*node == CanvasNode::TargetBlock(0)).then_some(id))
        .unwrap();
    let from = canvas.snarl.out_pin(OutPinId {
        node: from,
        output: 0,
    });
    let to = canvas.snarl.in_pin(InPinId { node: to, input: 1 });
    let root = match document {
        MappingDocument::Main => &mut app.project.root,
        MappingDocument::Target(index) => &mut app.project.extra_targets[index].root,
        MappingDocument::Function(_) => unreachable!(),
    };
    let error = {
        let mut viewer = GraphViewer {
            graph: &mut app.project.graph,
            root_scope: root,
            primary_root_authoring,
            extra_targets: if matches!(document, MappingDocument::Main) {
                &context.extra_targets
            } else {
                &[]
            },
            inactive_target_scopes: &inactive,
            project_references: crate::graph_viewer::ProjectGraphReferences::new(
                &context.failure_rules,
                &context.extra_sources,
            )
            .with_user_functions(&context.user_functions),
            source_blocks: &source_blocks,
            target_blocks: &target_blocks,
            source_x12: false,
            target_x12: false,
            source_paths: &source_paths,
            function_names,
            function_inputs,
            parameter_names: Default::default(),
            protected_output: None,
            function_output: None,
            requested_function_open: None,
            colors,
            wire_color_mode,
            endpoint_scroll: &mut canvas.endpoint_scroll,
            value_map_wheel: None,
            endpoint_search_match: None,
            node_sizes: Some(&mut canvas.node_sizes),
            hovered_node: None,
            hovered_node_this_frame: None,
            camera_pan: egui::Vec2::ZERO,
            camera_focus: None,
            canvas_transform: None,
            pin_interaction_ids: Vec::new(),
            error: None,
        };
        viewer.connect(&from, &to, &mut canvas.snarl);
        viewer.error.take()
    };
    app.observe_editor_history(std::time::Instant::now(), false);
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn connect_argument(
    app: &mut FerruleApp,
    document: MappingDocument,
    consumer: NodeId,
    input: usize,
    argument: NodeId,
) {
    // Retain the actual scope context; this action only connects graph nodes.
    let context = app.project.clone();
    let (mut root, target) = match document {
        MappingDocument::Main => (context.root.clone(), &context.target),
        MappingDocument::Target(index) => (
            context.extra_targets[index].root.clone(),
            &context.extra_targets[index].schema,
        ),
        MappingDocument::Function(_) => unreachable!(),
    };
    let root_before = serde_json::to_value(&root).unwrap();
    let source_blocks = source_blocks(&context.source);
    let target_blocks = target_blocks(target);
    let source_paths = SourcePathCatalog::new(&context.source, &context.extra_sources)
        .with_primary_source_options(&context.source_options);
    let inactive = match document {
        MappingDocument::Main => Vec::new(),
        MappingDocument::Target(index) => {
            let (before, current_and_after) = context.extra_targets.split_at(index);
            let (_, after) = current_and_after.split_first().unwrap();
            crate::graph_viewer::inactive_target_scopes(&context.root, before, after)
        }
        MappingDocument::Function(_) => unreachable!(),
    };
    let function_names = app.function_names();
    let function_inputs = app.function_inputs();
    let primary_root_authoring = matches!(document, MappingDocument::Main)
        && crate::primary_root_authoring::available(&app.project);
    let canvas = match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(index) => app
            .mapping_workspace
            .target_canvases
            .get_mut(&index)
            .unwrap(),
        MappingDocument::Function(_) => unreachable!(),
    };
    let find = |mapping| {
        canvas
            .snarl
            .node_ids()
            .find_map(|(id, node)| (*node == CanvasNode::Graph(mapping)).then_some(id))
            .unwrap()
    };
    let from = canvas.snarl.out_pin(OutPinId {
        node: find(argument),
        output: 0,
    });
    let to = canvas.snarl.in_pin(InPinId {
        node: find(consumer),
        input,
    });
    let mut viewer = GraphViewer {
        graph: &mut app.project.graph,
        root_scope: &mut root,
        primary_root_authoring,
        extra_targets: if matches!(document, MappingDocument::Main) {
            &context.extra_targets
        } else {
            &[]
        },
        inactive_target_scopes: &inactive,
        project_references: crate::graph_viewer::ProjectGraphReferences::new(
            &context.failure_rules,
            &context.extra_sources,
        )
        .with_user_functions(&context.user_functions),
        source_blocks: &source_blocks,
        target_blocks: &target_blocks,
        source_x12: false,
        target_x12: false,
        source_paths: &source_paths,
        function_names,
        function_inputs,
        parameter_names: Default::default(),
        protected_output: None,
        function_output: None,
        requested_function_open: None,
        colors: app.appearance.resolved_colors(app.palette),
        wire_color_mode: app.appearance.wire().color_mode(),
        endpoint_scroll: &mut canvas.endpoint_scroll,
        value_map_wheel: None,
        endpoint_search_match: None,
        node_sizes: Some(&mut canvas.node_sizes),
        hovered_node: None,
        hovered_node_this_frame: None,
        camera_pan: egui::Vec2::ZERO,
        camera_focus: None,
        canvas_transform: None,
        pin_interaction_ids: Vec::new(),
        error: None,
    };
    viewer.connect(&from, &to, &mut canvas.snarl);
    assert!(
        viewer.error.is_none(),
        "wire edit failed: {:?}",
        viewer.error
    );
    assert_eq!(serde_json::to_value(&root).unwrap(), root_before);
}

// Decode only the literal corpus's independently requested edit variants.
// Wire changes are applied through GraphViewer::connect, never an expected canvas.
fn edited_node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Const { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        other => panic!("unsupported literal edit variant: {other:?}"),
    }
}

fn independent_edit(app: &mut FerruleApp, document: MappingDocument, before: &Json, after: &Json) {
    let original = before["project"]["graph"]["nodes"].as_object().unwrap();
    let edited = after["project"]["graph"]["nodes"].as_object().unwrap();
    for (id, value) in edited {
        if original.get(id) != Some(value) {
            let id = id.parse().unwrap();
            let before_inputs = edited_node_inputs(&app.project.graph.nodes[&id]);
            let updated = node(value);
            let after_inputs = edited_node_inputs(&updated);
            assert_eq!(
                before_inputs.len(),
                after_inputs.len(),
                "argument edit changes pin count"
            );
            for (input, (&before, &after)) in before_inputs.iter().zip(&after_inputs).enumerate() {
                if before != after {
                    connect_argument(app, document, id, input, after);
                }
            }
            assert_eq!(
                edited_node_inputs(&app.project.graph.nodes[&id]),
                after_inputs
            );
            app.project.graph.nodes.insert(id, updated);
        }
    }
    app.observe_editor_history(std::time::Instant::now(), false);
}

#[test]
fn all_ten_complete_literal_copies_edits_history_saved_reopen_and_native_outcomes() {
    let originals = Originals::new();
    let corpus = literals(&originals);
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "admitted")
    {
        let name = text(&case["id"]);
        let snapshots = &case["snapshots"];
        // Materialize every complete expected Project/layout before the action.
        for (state, expected) in snapshots.as_object().unwrap() {
            let project = project(&expected["project"]);
            let layout = layout(&expected["layout"], &project);
            originals.debug(
                &format!("{name}.{state}.independent-project.original.txt"),
                &project,
            );
            originals.debug(
                &format!("{name}.{state}.independent-layout.original.txt"),
                &layout,
            );
        }
        let mut app = prepared(&snapshots["before"], false);
        let context = document(&case["active_context"]);
        assert_snapshot(
            &originals,
            &format!("{name}.before"),
            &app,
            &snapshots["before"],
        );
        preview(
            &originals,
            &format!("{name}.before"),
            &app,
            &case["native_preview"]["before"],
        );
        let action = app.duplicate_sequence_consumer(context, id(&case["selected_consumer"]));
        originals.debug(&format!("{name}.complete-action.original.txt"), &action);
        let after_action = retain_state(&originals, &format!("{name}.immediate-action"), &app);
        let plan = action.unwrap();
        let map = case["manual_id_map"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(old, new)| (old.parse::<NodeId>().unwrap(), id(new)))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(plan.ids, map, "{name}");
        assert_eq!(
            json!({"Ok":{"duplicate_consumer":plan.duplicate,"added_nodes":plan.nodes.len()}}),
            case["expected_action"],
            "{name}"
        );
        assert_eq!(after_action["history"]["undo"], 0);
        assert_eq!(after_action["history"]["redo"], 0);
        app.observe_editor_history(std::time::Instant::now(), false);
        assert_snapshot(
            &originals,
            &format!("{name}.after_duplicate"),
            &app,
            &snapshots["after_duplicate"],
        );
        assert_eq!(app.history.undo_len(), 1);
        assert_eq!(app.history.redo_len(), 0);
        assert!(app.pending_history.is_none());
        assert!(app.history.is_dirty(&app.observed_editor));
        preview(
            &originals,
            &format!("{name}.after_duplicate"),
            &app,
            &case["native_preview"]["after_duplicate"],
        );
        let binding = explicit_binding(&mut app, plan.duplicate, context);
        originals.debug(
            &format!("{name}.complete-binding-action.original.txt"),
            &binding,
        );
        assert_eq!(binding, Ok(()), "{name}");
        assert_snapshot(
            &originals,
            &format!("{name}.after_explicit_copy_binding"),
            &app,
            &snapshots["after_explicit_copy_binding"],
        );
        preview(
            &originals,
            &format!("{name}.after_explicit_copy_binding"),
            &app,
            &case["native_preview"]["after_explicit_copy_binding"],
        );
        if let Some(expected) = snapshots.get("duplicate_only_diagnostic") {
            let mut diagnostic = prepared(expected, false);
            diagnostic.project = app.project.clone();
            diagnostic.project.root.bindings = project(&expected["project"]).root.bindings;
            assert_snapshot(
                &originals,
                &format!("{name}.duplicate_only_diagnostic"),
                &diagnostic,
                expected,
            );
            preview(
                &originals,
                &format!("{name}.duplicate_only_diagnostic"),
                &diagnostic,
                &case["native_preview"]["duplicate_only_diagnostic"],
            );
        }
        let binding_project = app.project.clone();
        independent_edit(
            &mut app,
            context,
            &snapshots["after_explicit_copy_binding"],
            &snapshots["after_independent_copy_edit"],
        );
        assert_snapshot(
            &originals,
            &format!("{name}.after_independent_copy_edit"),
            &app,
            &snapshots["after_independent_copy_edit"],
        );
        preview(
            &originals,
            &format!("{name}.after_independent_copy_edit"),
            &app,
            &case["native_preview"]["after_independent_copy_edit"],
        );
        for (index, expected) in case["history"]["three_undo_restored_snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            app.undo_project();
            assert_snapshot(
                &originals,
                &format!("{name}.undo-{index}"),
                &app,
                &snapshots[text(expected)],
            );
            if text(expected) == "before" {
                assert!(!app.history.is_dirty(&app.observed_editor));
            }
        }
        for (index, expected) in case["history"]["three_redo_restored_snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            app.redo_project();
            assert_snapshot(
                &originals,
                &format!("{name}.redo-{index}"),
                &app,
                &snapshots[text(expected)],
            );
        }
        let path = originals.0.join(format!("{name}.saved-project.json"));
        let saved = app.save_document_to(&path);
        originals.bytes(
            &format!("{name}.whole-save-outcome.original.txt"),
            match &saved {
                Ok(value) => format!(
                    "Ok {{ validation_issues: {:#?}, layout_warning: {:#?} }}",
                    value.validation_issues, value.layout_warning
                ),
                Err(error) => format!("Err {error:#?}"),
            }
            .as_bytes(),
        );
        let layout_path = crate::layout_store::layout_path(&path);
        if path.exists() {
            originals.bytes(
                &format!("{name}.saved-project-bytes.original.bin"),
                &std::fs::read(&path).unwrap(),
            );
        }
        if layout_path.exists() {
            originals.bytes(
                &format!("{name}.saved-layout-bytes.original.bin"),
                &std::fs::read(&layout_path).unwrap(),
            );
        }
        saved.unwrap();
        assert_snapshot(
            &originals,
            &format!("{name}.saved"),
            &app,
            &snapshots["after_independent_copy_edit"],
        );
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        assert_snapshot(
            &originals,
            &format!("{name}.reopened"),
            &reopened,
            &snapshots["after_independent_copy_edit"],
        );
        assert!(!reopened.history.is_dirty(&reopened.observed_editor));
        preview(
            &originals,
            &format!("{name}.reopened"),
            &reopened,
            &case["native_preview"]["after_independent_copy_edit"],
        );
        app.undo_project();
        assert_eq!(
            crate::project_state::project_snapshot_key(&app.project),
            crate::project_state::project_snapshot_key(&binding_project),
        );
        independent_edit(
            &mut app,
            context,
            &snapshots["after_explicit_copy_binding"],
            &snapshots["after_independent_original_edit"],
        );
        assert_snapshot(
            &originals,
            &format!("{name}.after_independent_original_edit"),
            &app,
            &snapshots["after_independent_original_edit"],
        );
        preview(
            &originals,
            &format!("{name}.after_independent_original_edit"),
            &app,
            &case["native_preview"]["after_independent_original_edit"],
        );
    }
}

#[test]
fn real_target_binding_keeps_raw_and_transitive_stage_inputs_private() {
    let originals = Originals::new();
    let corpus = literals(&originals);
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| text(&case["id"]) == "filter-map-item-at-stage-choice")
        .unwrap();
    let expected = Err::<(), _>(
        "Stage input values are private. Connect the mapped output in its owning context instead."
            .to_string(),
    );
    originals.debug(
        "private-target-binding.independent-error.original.txt",
        &expected,
    );
    for (name, from) in [("raw-stage-input", 21), ("transitive-stage-input", 90)] {
        let mut app = prepared(&case["snapshots"]["after_duplicate"], false);
        if from == 90 {
            app.project.graph.nodes.insert(
                90,
                Node::Call {
                    function: "add".into(),
                    args: vec![21, 4],
                },
            );
        }
        app.main_canvas
            .snarl
            .insert_node(egui::pos2(300.0, 300.0), CanvasNode::Graph(from));
        let setup = editor_snapshot(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        app.history = SnapshotHistory::new(setup.clone(), DocumentOrigin::Saved);
        app.observed_editor = setup;
        app.pending_history = None;
        let before = retain_state(&originals, &format!("{name}.before"), &app);
        let result = explicit_binding(&mut app, from, MappingDocument::Main);
        originals.debug(
            &format!("{name}.complete-binding-action.original.txt"),
            &result,
        );
        let after = retain_state(&originals, &format!("{name}.after"), &app);
        assert_eq!(result, expected, "{name}");
        assert_eq!(before, after, "{name}");
    }
}

#[test]
fn all_eighteen_literal_refusals_preserve_whole_project_canvas_saved_history_and_diagnostics() {
    let originals = Originals::new();
    let corpus = literals(&originals);
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "refused")
    {
        let name = text(&case["id"]);
        let expected = &case["snapshots"]["before"];
        let mut app = prepared(expected, true);
        let before = retain_state(&originals, &format!("{name}.before"), &app);
        let before_snarl = format!("{:#?}", app.main_canvas.snarl);
        let action = app.duplicate_sequence_consumer(
            document(&case["active_context"]),
            id(&case["selected_consumer"]),
        );
        originals.debug(&format!("{name}.complete-action.original.txt"), &action);
        let after = retain_state(&originals, &format!("{name}.after"), &app);
        let actual = match &action {
            Ok(plan) => json!({"unexpected_success":format!("{plan:#?}")}),
            Err(error) => refusal_original(error),
        };
        originals.bytes(
            &format!("{name}.complete-refusal.original.json"),
            &serde_json::to_vec_pretty(&actual).unwrap(),
        );
        assert_eq!(actual, case["expected_action"], "{name}");
        assert_eq!(before, after, "{name}");
        assert_eq!(
            before_snarl,
            format!("{:#?}", app.main_canvas.snarl),
            "{name}"
        );
        assert_snapshot(
            &originals,
            &format!("{name}.full-refusal"),
            &app,
            &case["snapshots"]["after"],
        );
        let snapshot = app.observed_editor.clone();
        let redo = app.history.redo(snapshot.clone()).unwrap();
        originals.bytes(&format!("{name}.retained-redo.original.json"), &serde_json::to_vec_pretty(&json!({"label":redo.label(),"whole_project_debug":format!("{:#?}",redo.snapshot().project),"project_key":crate::project_state::project_snapshot_key(&redo.snapshot().project),"state_project_key":redo.snapshot().state.serialized_project,"layout":layout_bits(&redo.snapshot().state.layout)})).unwrap());
        assert_eq!(
            crate::project_state::project_snapshot_key(&redo.snapshot().project),
            snapshot.state.serialized_project
        );
        assert_eq!(redo.label(), "Retained redo");
        assert_eq!(
            redo.snapshot().state.serialized_project,
            snapshot.state.serialized_project
        );
        assert_eq!(
            layout_bits(&redo.snapshot().state.layout),
            layout_bits(&snapshot.state.layout)
        );
        let undo = app.history.undo(snapshot.clone()).unwrap();
        originals.bytes(&format!("{name}.retained-redo-reversal.original.json"), &serde_json::to_vec_pretty(&json!({"label":undo.label(),"whole_project_debug":format!("{:#?}",undo.snapshot().project),"project_key":crate::project_state::project_snapshot_key(&undo.snapshot().project),"state_project_key":undo.snapshot().state.serialized_project,"layout":layout_bits(&undo.snapshot().state.layout)})).unwrap());
        assert_eq!(
            crate::project_state::project_snapshot_key(&undo.snapshot().project),
            snapshot.state.serialized_project
        );
        assert_eq!(undo.label(), "Retained redo");
        assert_eq!(
            layout_bits(&undo.snapshot().state.layout),
            layout_bits(&snapshot.state.layout)
        );
        let undo = app.history.undo(snapshot.clone()).unwrap();
        originals.bytes(&format!("{name}.retained-prior-undo.original.json"), &serde_json::to_vec_pretty(&json!({"label":undo.label(),"whole_project_debug":format!("{:#?}",undo.snapshot().project),"project_key":crate::project_state::project_snapshot_key(&undo.snapshot().project),"state_project_key":undo.snapshot().state.serialized_project,"layout":layout_bits(&undo.snapshot().state.layout)})).unwrap());
        assert_eq!(
            crate::project_state::project_snapshot_key(&undo.snapshot().project),
            snapshot.state.serialized_project
        );
        assert_eq!(undo.label(), "Retained prior edit");
        assert_eq!(
            undo.snapshot().state.serialized_project,
            snapshot.state.serialized_project
        );
        assert_eq!(
            layout_bits(&undo.snapshot().state.layout),
            layout_bits(&snapshot.state.layout)
        );
    }
}

#[test]
fn request_is_one_canvas_one_frame_and_keyboard_history_restores_the_complete_copy() {
    let originals = Originals::new();
    let corpus = literals(&originals);
    let case = &corpus["cases"][0];
    let mut app = prepared(&case["snapshots"]["before"], false);
    let context = egui::Context::default();
    request(&context, 999);
    begin(&context);
    let stale = take(&context);
    originals.debug("stale-request.original.txt", &stale);
    assert_eq!(stale, None);
    request(&context, 20);
    let request = take(&context);
    originals.debug("current-request.original.txt", &request);
    assert_eq!(request, Some(20));
    assert_eq!(take(&context), None);
    app.apply_sequence_duplication_request(MappingDocument::Main, request.unwrap());
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_snapshot(
        &originals,
        "request.after",
        &app,
        &case["snapshots"]["after_duplicate"],
    );
    for (redo, state) in [(false, "before"), (true, "after_duplicate")] {
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            shift: redo,
            ..Default::default()
        };
        let output = context.run_ui(
            egui::RawInput {
                modifiers,
                events: [true, false]
                    .into_iter()
                    .map(|pressed| egui::Event::Key {
                        key: egui::Key::Z,
                        physical_key: Some(egui::Key::Z),
                        pressed,
                        repeat: false,
                        modifiers,
                    })
                    .collect(),
                ..Default::default()
            },
            |ui| app.handle_history_shortcuts(ui.ctx(), true),
        );
        originals.frame(
            &format!("keyboard-{redo}.whole-output.original.txt"),
            &output,
        );
        app.observe_editor_history(std::time::Instant::now(), false);
        assert_snapshot(
            &originals,
            &format!("keyboard-{redo}"),
            &app,
            &case["snapshots"][state],
        );
    }
}
