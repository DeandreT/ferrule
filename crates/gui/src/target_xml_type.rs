//! Schema-guided writer type selection, independent of source-root expressions.

use ir::{SchemaKind, SchemaNode, Value, XML_TYPE_FIELD, XmlAlternativeKind};
use mapping::{Binding, Graph, Node, NodeId, Project, Scope, ScopeConstruction};

#[derive(Clone, Debug)]
pub(crate) enum Action {
    Declared(String),
    Infer,
}

pub(crate) fn choices(schema: &SchemaNode) -> Vec<String> {
    if !schema.xml_type_alternatives
        || schema.xml_alternative_kind != XmlAlternativeKind::XsiType
        || !schema.metadata_is_valid()
        || schema.attribute
        || schema.text
        || schema.recursive_ref.is_some()
        || !matches!(schema.kind, SchemaKind::Group { .. })
        || schema.child(XML_TYPE_FIELD).is_some()
        || !schema
            .alternatives()
            .iter()
            .all(|alternative| ir::primary_root_type_identity_is_valid(&alternative.name))
    {
        return Vec::new();
    }
    schema
        .alternatives()
        .iter()
        .map(|alternative| alternative.name.clone())
        .collect()
}

pub(crate) fn schema_at<'a>(schema: &'a SchemaNode, chain: &[String]) -> Option<&'a SchemaNode> {
    let mut schema = schema;
    for field in chain {
        schema = schema.child(field)?;
    }
    Some(schema)
}

pub(crate) fn field_label(field: &str) -> &str {
    if field == XML_TYPE_FIELD {
        "XML type"
    } else {
        field
    }
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    graph: &Graph,
    scope: &Scope,
    choices: &[String],
    default: Option<&str>,
    editable: bool,
) -> Option<Action> {
    let bindings: Vec<_> = scope
        .bindings
        .iter()
        .filter(|binding| binding.target_field == XML_TYPE_FIELD)
        .collect();
    if choices.is_empty() && bindings.is_empty() {
        return None;
    }
    let selected = bindings
        .first()
        .and_then(|binding| graph.nodes.get(&binding.node));
    let label = if bindings.len() > 1 {
        "Multiple XML type bindings".to_string()
    } else {
        match selected {
            Some(Node::Const {
                value: Value::String(value),
            }) if choices.contains(value) => value.clone(),
            Some(Node::Const {
                value: Value::String(value),
            }) => {
                let preview: String = value.chars().take(120).collect();
                let suffix = if value.chars().nth(120).is_some() {
                    "…"
                } else {
                    ""
                };
                format!("Invalid XML type: {preview:?}{suffix}")
            }
            Some(Node::Const { value }) => format!("Invalid XML type: {value:?}"),
            Some(_) => "Computed expression".to_string(),
            None if !bindings.is_empty() => "Missing expression".to_string(),
            None => "Schema inference".to_string(),
        }
    };
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label("XML type:");
        egui::ComboBox::from_id_salt(ui.id().with("target_xml_type"))
            .selected_text(label)
            .width(250.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(bindings.is_empty(), "Schema inference").clicked() {
                    action = Some(Action::Infer);
                }
                ui.add_enabled_ui(editable && bindings.len() <= 1, |ui| {
                    for identity in choices {
                        let current = matches!(selected, Some(Node::Const { value: Value::String(value) }) if value == identity);
                        if ui.selectable_label(current, identity).clicked() {
                            action = Some(Action::Declared(identity.clone()));
                        }
                    }
                });
            });
    });
    if let Some(default) = default {
        ui.weak(format!("Schema inference removes the type binding and uses {default} when the populated fields fit; otherwise it selects a matching declared type."));
    } else {
        ui.weak("Schema inference removes the type binding and selects a declared type from populated fields. Ambiguous selections fail when writing XML.");
    }
    if choices.is_empty() {
        ui.colored_label(ui.visuals().warn_fg_color, "No supported declared XML types; the imported binding is retained for removal or repair.");
    } else if !editable {
        ui.weak(
            "Choose a constructed scope outside whole source group copies to bind an XML type.",
        );
    }
    if bindings.len() > 1 {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Remove duplicate XML type bindings before selecting a declared type.",
        );
    } else if matches!(selected, Some(Node::Const { value }) if !matches!(value, Value::String(identity) if choices.contains(identity)))
    {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Choose a declared XML type or schema inference to repair this value.",
        );
    }
    action
}

/// Replace only this exact binding. Shared or missing expressions are never
/// mutated or deleted, including invalid imported graph owners.
pub(crate) fn apply(
    project: &mut Project,
    target: Option<usize>,
    path: &[usize],
    action: &Action,
) -> Result<bool, String> {
    let (root, schema) = match target {
        Some(index) => project
            .extra_targets
            .get(index)
            .map(|target| (&target.root, &target.schema))
            .ok_or("The selected target no longer exists")?,
        None => (&project.root, &project.target),
    };
    let scope =
        crate::auto_connect::scope_at(root, path).ok_or("The selected scope no longer exists")?;
    let marker_bindings: Vec<_> = scope
        .bindings
        .iter()
        .filter(|binding| binding.target_field == XML_TYPE_FIELD)
        .collect();
    let node = match action {
        Action::Infer => {
            if marker_bindings.is_empty() {
                return Ok(false);
            }
            None
        }
        Action::Declared(identity) => {
            if crate::scope_editor::copied_ancestor_at_path(root, path).is_some()
                || scope.construction != ScopeConstruction::Constructed
                || scope.concatenated().is_some()
            {
                return Err("This scope cannot contain an XML type binding".to_string());
            }
            let chain = crate::scope_editor::scope_target_chain(root, path);
            let valid =
                schema_at(schema, &chain).is_some_and(|schema| choices(schema).contains(identity));
            if !valid {
                return Err("Choose an exact declared XML type for this target group".to_string());
            }
            if marker_bindings.len() > 1 {
                return Err("Remove duplicate XML type bindings before choosing a type".to_string());
            }
            if marker_bindings.first().is_some_and(|binding| matches!(project.graph.nodes.get(&binding.node), Some(Node::Const { value: Value::String(value) }) if value == identity)) {
                return Ok(false);
            }
            Some(reserve_id(project)?)
        }
    };
    let root = match target {
        Some(index) => {
            &mut project
                .extra_targets
                .get_mut(index)
                .ok_or("The selected target no longer exists")?
                .root
        }
        None => &mut project.root,
    };
    let scope = crate::auto_connect::scope_at_mut(root, path)
        .ok_or("The selected scope no longer exists")?;
    match (action, node) {
        (Action::Infer, _) => scope
            .bindings
            .retain(|binding| binding.target_field != XML_TYPE_FIELD),
        (Action::Declared(identity), Some(node)) => {
            project.graph.nodes.insert(
                node,
                Node::Const {
                    value: Value::String(identity.clone()),
                },
            );
            if let Some(binding) = scope
                .bindings
                .iter_mut()
                .find(|binding| binding.target_field == XML_TYPE_FIELD)
            {
                binding.node = node;
            } else {
                scope.bindings.push(Binding {
                    target_field: XML_TYPE_FIELD.into(),
                    node,
                });
            }
        }
        _ => return Err("No XML type expression was reserved".to_string()),
    }
    Ok(true)
}

fn reserve_id(project: &Project) -> Result<NodeId, String> {
    let mut candidate = project
        .graph
        .nodes
        .keys()
        .next_back()
        .map_or(Some(0), |id| id.checked_add(1))
        .ok_or("Mapping node IDs are exhausted")?;
    let reserved = reserved_ids(project);
    // Each reserved ID is visited at most once. Work depends on the owned
    // reference set, never on the numeric distance between sparse IDs.
    for &id in reserved.range(candidate..) {
        if id > candidate {
            break;
        }
        candidate = candidate
            .checked_add(1)
            .ok_or("Mapping node IDs are exhausted")?;
    }
    Ok(candidate)
}

/// Project expressions share IDs across targets, controls, failures and
/// dynamic input paths. Function bodies own a separate graph namespace.
fn reserved_ids(project: &Project) -> std::collections::BTreeSet<NodeId> {
    let mut reserved = crate::graph_viewer::project_sequence_item_ids(project);
    reserved.extend(project.graph.nodes.values().flat_map(Node::dependencies));
    let mut scopes = vec![&project.root];
    scopes.extend(project.extra_targets.iter().map(|target| &target.root));
    while let Some(scope) = scopes.pop() {
        reserved.extend(
            [scope.filter, scope.post_group_filter, scope.output_path()]
                .into_iter()
                .flatten(),
        );
        reserved.extend(scope.grouping_nodes());
        reserved.extend(scope.sort_keys().map(|key| key.node));
        reserved.extend(scope.windows.iter().flat_map(|window| window.nodes()));
        if let Some(sequence) = scope.sequence() {
            reserved.extend(sequence.inputs());
        }
        match &scope.construction {
            ScopeConstruction::Scalar { value } => {
                reserved.insert(*value);
            }
            ScopeConstruction::RecursiveFilter { plan } => {
                reserved.insert(plan.predicate());
            }
            ScopeConstruction::AdjacencyTree { plan } => {
                reserved.extend(plan.root());
            }
            ScopeConstruction::Constructed
            | ScopeConstruction::CopyCurrentSource
            | ScopeConstruction::XmlMixedContent { .. }
            | ScopeConstruction::PathHierarchy { .. } => {}
        }
        reserved.extend(scope.bindings.iter().map(|binding| binding.node));
        reserved.extend(
            scope
                .dynamic_bindings
                .iter()
                .flat_map(|binding| [binding.key, binding.value]),
        );
        reserved.extend(scope.dynamic_children.iter().map(|child| child.key));
        scopes.extend(scope.children.iter());
        scopes.extend(scope.dynamic_children.iter().map(|child| &child.scope));
        if let Some(segments) = scope.concatenated() {
            scopes.extend(segments.iter());
        }
    }
    for rule in &project.failure_rules {
        reserved.extend(rule.selection.predicate());
        reserved.extend(rule.message);
        if let mapping::FailureIteration::Sequence { sequence } = &rule.iteration {
            reserved.extend(sequence.inputs());
        }
    }
    reserved.extend(
        project
            .extra_sources
            .iter()
            .filter_map(|source| source.dynamic_path.as_ref().map(|path| path.node)),
    );
    reserved
}

#[cfg(test)]
#[path = "target_xml_type_tests.rs"]
pub(crate) mod tests;
