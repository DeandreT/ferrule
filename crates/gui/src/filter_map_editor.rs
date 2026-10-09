//! Focused controls for the supported scalar filter/map descriptor.

use egui::{Ui, WidgetInfo, WidgetType};
use ir::{ScalarType, Value};
use mapping::{
    FilterMapCapture, FilterMapV1, FunctionId, Graph, Node, NodeId, SequenceExpr, UserFunction,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) type Functions = BTreeMap<FunctionId, UserFunction>;
pub(crate) struct Items<'a> {
    pub all: &'a BTreeSet<NodeId>,
    pub inputs: &'a BTreeSet<NodeId>,
}

const TYPES: [ScalarType; 4] = [
    ScalarType::Int,
    ScalarType::Float,
    ScalarType::Bool,
    ScalarType::String,
];

pub(crate) fn sequence(node: &Node) -> Option<&SequenceExpr> {
    match node {
        Node::SequenceExists { sequence, .. }
        | Node::SequenceItemAt { sequence, .. }
        | Node::SequenceAggregate { sequence, .. } => Some(sequence),
        _ => None,
    }
}

pub(crate) fn editable(sequence: &SequenceExpr) -> bool {
    matches!(sequence, SequenceExpr::FilterMapV1(value)
        if matches!(value.source.as_ref(), SequenceExpr::Generate { .. })
            && value.source.item() != value.item
            && value.captures.len() <= mapping::MAX_FILTER_MAP_CAPTURES)
}

fn signature(function: &UserFunction, parameters: &[ScalarType], output: ScalarType) -> bool {
    function
        .parameters
        .iter()
        .map(|parameter| parameter.ty)
        .eq(parameters.iter().copied())
        && function.output_type == output
}

pub(crate) fn default_stages(
    functions: &Functions,
) -> Option<(FunctionId, FunctionId, ScalarType)> {
    let parameters = [ScalarType::Int, ScalarType::Int];
    let predicate = functions.iter().find_map(|(&id, function)| {
        signature(function, &parameters, ScalarType::Bool).then_some(id)
    })?;
    let (mapper, output) = TYPES.into_iter().find_map(|output| {
        functions.iter().find_map(|(&id, function)| {
            signature(function, &parameters, output).then_some((id, output))
        })
    })?;
    Some((predicate, mapper, output))
}

pub(crate) fn create(
    ids: &[NodeId],
    functions: &Functions,
) -> Result<(SequenceExpr, Vec<(NodeId, Node)>), String> {
    let [from, to, source_item, output_item] = ids else {
        return Err("Filter/map creation requires four reserved identities.".into());
    };
    if ids.iter().copied().collect::<BTreeSet<_>>().len() != 4 {
        return Err("Filter/map identities must be distinct.".into());
    }
    let Some((predicate, mapper, output_type)) = default_stages(functions) else {
        return Err(
            "Add filter and map functions with input value and input position parameters first."
                .into(),
        );
    };
    let sequence = SequenceExpr::FilterMapV1(FilterMapV1 {
        source: Box::new(SequenceExpr::Generate {
            from: Some(*from),
            to: *to,
            item: *source_item,
        }),
        item: *output_item,
        predicate,
        mapper,
        output_type,
        captures: Vec::new(),
    });
    let nodes = vec![
        (
            *from,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            *to,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            *source_item,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            *output_item,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ];
    Ok((sequence, nodes))
}

/// This checks the entire ordinary parent expression before a wiring mutation.
pub(crate) fn parent_error(
    graph: &Graph,
    root: NodeId,
    forbidden: &BTreeSet<NodeId>,
    consumer: Option<NodeId>,
) -> Option<String> {
    let mut complete = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut pending = vec![(root, false)];
    while let Some((id, leaving)) = pending.pop() {
        if leaving {
            active.remove(&id);
            complete.insert(id);
            continue;
        }
        if forbidden.contains(&id) {
            return Some(
                "This input cannot read a private sequence item. Connect a parent value instead."
                    .into(),
            );
        }
        if consumer == Some(id) || active.contains(&id) {
            return Some("This connection would create a graph cycle.".into());
        }
        if complete.contains(&id) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            return Some(format!("Input node {id} is missing."));
        };
        active.insert(id);
        pending.push((id, true));
        pending.extend(
            node.dependencies()
                .into_iter()
                .rev()
                .map(|input| (input, false)),
        );
    }
    None
}

pub(crate) fn input_error(
    node: &Node,
    consumer: NodeId,
    index: usize,
    from: NodeId,
    graph: &Graph,
    owned: &BTreeSet<NodeId>,
) -> Option<String> {
    let sequence = sequence(node)?;
    let SequenceExpr::FilterMapV1(value) = sequence else {
        return None;
    };
    let source_inputs = value.source.inputs().len();
    let mut forbidden: BTreeSet<_> = sequence.owned_items().into_iter().collect();
    if matches!(node, Node::SequenceItemAt { .. }) || index >= source_inputs {
        // Captures never inherit an item context. ItemAt's source and index are
        // both evaluated in the existing empty item context.
        forbidden.extend(owned);
    }
    let item_expression = match node {
        Node::SequenceExists { .. } => index == sequence.inputs().len(),
        Node::SequenceAggregate {
            predicate,
            expression,
            ..
        } => {
            let item_inputs = usize::from(predicate.is_some()) + usize::from(expression.is_some());
            index >= sequence.inputs().len() && index < sequence.inputs().len() + item_inputs
        }
        _ => false,
    };
    if item_expression {
        // Exists predicates and aggregate item expressions see the output only.
        forbidden.remove(&value.item);
    }
    parent_error(graph, from, &forbidden, Some(consumer))
}

fn label(graph: &Graph, id: NodeId) -> String {
    match graph.nodes.get(&id) {
        Some(Node::Const { value }) => format!("{id}: constant {value:?}"),
        Some(Node::SourceField { path, .. }) => format!("{id}: source {}", path.join("/")),
        Some(Node::Position { .. }) => format!("{id}: position"),
        Some(_) => format!("{id}: expression"),
        None => format!("{id}: missing input"),
    }
}

fn picker(
    ui: &mut Ui,
    name: &str,
    value: &mut NodeId,
    graph: &Graph,
    forbidden: &BTreeSet<NodeId>,
    consumer: Option<NodeId>,
) {
    let response = egui::ComboBox::from_id_salt(ui.id().with(name))
        .selected_text(label(graph, *value))
        .width(180.0)
        .show_ui(ui, |ui| {
            for (&id, node) in &graph.nodes {
                if !matches!(node, Node::Unconnected)
                    && parent_error(graph, id, forbidden, consumer).is_none()
                {
                    ui.selectable_value(value, id, label(graph, id));
                }
            }
        })
        .response;
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, ui.is_enabled(), name));
    #[cfg(test)]
    observations::record(name, &response, ui.clip_rect());
}

fn type_picker(ui: &mut Ui, name: &str, value: &mut ScalarType) {
    let response = egui::ComboBox::from_id_salt(ui.id().with(name))
        .selected_text(format!("{value:?}").to_lowercase())
        .show_ui(ui, |ui| {
            for candidate in TYPES {
                ui.selectable_value(value, candidate, format!("{candidate:?}").to_lowercase());
            }
        })
        .response;
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, ui.is_enabled(), name));
    #[cfg(test)]
    observations::record(name, &response, ui.clip_rect());
}

fn stage(
    ui: &mut Ui,
    name: &str,
    id: &mut FunctionId,
    functions: &Functions,
    parameters: &[ScalarType],
    output: ScalarType,
) {
    let selected = functions
        .get(id)
        .map_or("missing function", |function| function.name.as_str());
    ui.label(name);
    let response = egui::ComboBox::from_id_salt(ui.id().with(name))
        .selected_text(selected)
        .width(210.0)
        .show_ui(ui, |ui| {
            for (&candidate, function) in functions {
                if signature(function, parameters, output) {
                    ui.selectable_value(
                        id,
                        candidate,
                        format!("{} / {}", function.library, function.name),
                    );
                }
            }
        })
        .response;
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, ui.is_enabled(), name));
    #[cfg(test)]
    observations::record(name, &response, ui.clip_rect());
    if functions
        .get(id)
        .is_none_or(|function| !signature(function, parameters, output))
    {
        ui.colored_label(ui.visuals().error_fg_color, format!("{name} needs {parameters:?} → {output:?}. Its current selection is kept for repair."));
    }
}

/// Edits only the descriptor. Owned item identities and UDF bodies are immutable here.
pub(crate) fn show(
    ui: &mut Ui,
    sequence: &mut SequenceExpr,
    graph: &Graph,
    functions: &Functions,
    items: Items<'_>,
    consumer: Option<NodeId>,
    item_at: bool,
) {
    if !matches!(sequence, SequenceExpr::FilterMapV1(_)) {
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("filter_map_editor_scroll"))
        .max_height(ui.available_height().clamp(160.0, 480.0))
        .show(ui, |ui| {
            show_body(ui, sequence, graph, functions, items, consumer, item_at);
        });
}

fn show_body(
    ui: &mut Ui,
    sequence: &mut SequenceExpr,
    graph: &Graph,
    functions: &Functions,
    items: Items<'_>,
    consumer: Option<NodeId>,
    item_at: bool,
) {
    let SequenceExpr::FilterMapV1(value) = sequence else {
        return;
    };
    ui.strong("Filter/map stages");
    ui.small("Stages receive input value, input position, then captures. Output positions are dense after filtering.");
    let SequenceExpr::Generate {
        from,
        to,
        item: source_item,
    } = value.source.as_mut()
    else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "This stored source is unsupported. Its complete settings are kept.",
        );
        return;
    };
    let mut forbidden = [*source_item, value.item]
        .into_iter()
        .collect::<BTreeSet<_>>();
    forbidden.extend(items.inputs);
    if item_at {
        forbidden.extend(items.all);
    }
    let eligible = graph.nodes.iter().find_map(|(&id, node)| {
        (!matches!(node, Node::Unconnected)
            && parent_error(graph, id, &forbidden, consumer).is_none())
        .then_some(id)
    });
    let mut default_start = from.is_none();
    let default_response = ui.add_enabled(
        !default_start || eligible.is_some(),
        egui::Checkbox::new(&mut default_start, "Default start (1)"),
    );
    #[cfg(test)]
    observations::record("Default start (1)", &default_response, ui.clip_rect());
    if default_response.changed() {
        *from = if default_start { None } else { eligible };
    }
    if let Some(from) = from {
        ui.label("From");
        picker(ui, "From", from, graph, &forbidden, consumer);
    }
    ui.label("To");
    picker(ui, "To", to, graph, &forbidden, consumer);
    ui.label("Output type");
    type_picker(ui, "Output type", &mut value.output_type);
    let mut remove = None;
    let mut move_capture = None;
    let capture_count = value.captures.len();
    egui::ScrollArea::vertical()
        .id_salt(ui.id().with("filter_map_captures"))
        .max_height(220.0)
        .show(ui, |ui| {
            for (index, capture) in value.captures.iter_mut().enumerate() {
                let number = index + 1;
                ui.separator();
                ui.label(format!("Capture {number} (evaluated in this order)"));
                picker(
                    ui,
                    &format!("Capture {number} expression"),
                    &mut capture.node,
                    graph,
                    items.all,
                    consumer,
                );
                type_picker(ui, &format!("Capture {number} type"), &mut capture.ty);
                ui.horizontal(|ui| {
                    let response = crate::icons::button(
                        ui,
                        index > 0,
                        lucide_icons::Icon::ArrowUp,
                        format!("Move capture {number} up"),
                    );
                    #[cfg(test)]
                    observations::record(
                        &format!("Move capture {number} up"),
                        &response,
                        ui.clip_rect(),
                    );
                    if response.clicked() {
                        move_capture = Some((index, index - 1));
                    }
                    let response = crate::icons::button(
                        ui,
                        index + 1 < capture_count,
                        lucide_icons::Icon::ArrowDown,
                        format!("Move capture {number} down"),
                    );
                    #[cfg(test)]
                    observations::record(
                        &format!("Move capture {number} down"),
                        &response,
                        ui.clip_rect(),
                    );
                    if response.clicked() {
                        move_capture = Some((index, index + 1));
                    }
                    let response = crate::icons::button(
                        ui,
                        true,
                        lucide_icons::Icon::Minus,
                        format!("Remove capture {number}"),
                    );
                    #[cfg(test)]
                    observations::record(
                        &format!("Remove capture {number}"),
                        &response,
                        ui.clip_rect(),
                    );
                    if response.clicked() {
                        remove = Some(index);
                    }
                });
            }
        });
    if let Some(index) = remove {
        value.captures.remove(index);
    } else if let Some((from, to)) = move_capture {
        value.captures.swap(from, to);
    }
    let candidate = graph.nodes.iter().find_map(|(&id, node)| {
        (!matches!(node, Node::Unconnected)
            && parent_error(graph, id, items.all, consumer).is_none())
        .then_some(id)
    });
    let add = crate::icons::button(
        ui,
        value.captures.len() < mapping::MAX_FILTER_MAP_CAPTURES && candidate.is_some(),
        lucide_icons::Icon::Plus,
        "Add capture",
    );
    #[cfg(test)]
    observations::record("Add capture", &add, ui.clip_rect());
    if add.clicked()
        && let Some(node) = candidate
    {
        value.captures.push(FilterMapCapture {
            node,
            ty: ScalarType::Int,
        });
    }
    if value.captures.len() > mapping::MAX_FILTER_MAP_CAPTURES {
        ui.colored_label(ui.visuals().error_fg_color, "At most 16 captures are supported. Existing captures are retained; remove one to repair.");
    }
    let parameters = [ScalarType::Int, ScalarType::Int]
        .into_iter()
        .chain(value.captures.iter().map(|capture| capture.ty))
        .collect::<Vec<_>>();
    stage(
        ui,
        "Filter stage",
        &mut value.predicate,
        functions,
        &parameters,
        ScalarType::Bool,
    );
    stage(
        ui,
        "Map stage",
        &mut value.mapper,
        functions,
        &parameters,
        value.output_type,
    );
    ui.small("Validate checks stage bodies and expression context. Save the mapping before generating a library.");
}

#[cfg(test)]
pub(crate) mod observations {
    #[derive(Clone, Debug)]
    pub(crate) struct Control {
        pub name: String,
        pub response: egui::Response,
        pub clip: egui::Rect,
    }
    thread_local! {
        static CONTROLS: std::cell::RefCell<Option<Vec<Control>>> = const { std::cell::RefCell::new(None) };
    }
    pub(crate) fn begin() {
        CONTROLS.with(|value| *value.borrow_mut() = Some(Vec::new()));
    }
    pub(crate) fn record(name: &str, response: &egui::Response, clip: egui::Rect) {
        CONTROLS.with(|value| {
            if let Some(controls) = value.borrow_mut().as_mut() {
                controls.push(Control {
                    name: name.into(),
                    response: response.clone(),
                    clip,
                });
            }
        });
    }
    pub(crate) fn take() -> Vec<Control> {
        CONTROLS.with(|value| value.borrow_mut().take().unwrap_or_default())
    }
    pub(crate) fn retain_instance(instance: &ir::Instance, path: &str) {
        eprintln!("complete instance path={path:?} original={instance:#?}");
        match instance {
            ir::Instance::Scalar(value) => eprintln!(
                "typed scalar original={value:?} bits={:?}",
                match value {
                    ir::Value::Float(value) => Some(value.to_bits()),
                    _ => None,
                }
            ),
            ir::Instance::Group(group) => {
                eprintln!(
                    "group origin path={path:?} original={:?}",
                    group.xml_type_origin()
                );
                for (index, (_, value)) in group.iter().enumerate() {
                    retain_instance(value, &format!("{path}/field-{index}"));
                }
            }
            ir::Instance::Repeated(values) | ir::Instance::MappedSequence(values) => {
                for (index, value) in values.iter().enumerate() {
                    retain_instance(value, &format!("{path}/item-{index}"));
                }
            }
            ir::Instance::DocumentSet(values) => {
                for (index, value) in values.iter().enumerate() {
                    eprintln!("document original={value:#?}");
                    retain_instance(value.value(), &format!("{path}/document-{index}"));
                }
            }
        }
    }
    pub(crate) fn retain_error(error: &engine::EngineError) {
        eprintln!("complete typed error={error:#?} display={error}");
        match error {
            engine::EngineError::FilterMapRuntime { source, .. } => retain_error(source),
            engine::EngineError::FilterMapValueType { expected, found } => {
                eprintln!(
                    "expected={expected:?} found={found:?} bits={:?}",
                    match found {
                        ir::Value::Float(value) => Some(value.to_bits()),
                        _ => None,
                    }
                );
            }
            _ => {}
        }
    }
}
