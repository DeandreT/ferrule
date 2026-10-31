//! Bounded authoring of zero-or-one constructed groups, without creating
//! a new item frame or changing any of the group's content.
//!
//! Offers Boolean constants, typed source/host inputs, conditionals and catalog
//! Boolean calls over ordinary scalar dependencies. Static source ancestors
//! retain their frame identity; owned, join and generated-item routes stay
//! outside this picker. Inspection never repairs a retained condition.

use std::collections::BTreeSet;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    Graph, IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration,
};

const MAX_GRAPH_NODES: usize = 4096;
const MAX_EXPRESSION_NODES: usize = 256;
const MAX_EXPRESSION_DEPTH: usize = 64;

pub(crate) struct Profile {
    reason: Option<&'static str>,
    conditions: Vec<NodeId>,
}

#[cfg(test)]
impl Profile {
    pub(crate) fn is_editable(&self) -> bool {
        self.reason.is_none()
    }
    pub(crate) fn condition_ids(&self) -> &[NodeId] {
        &self.conditions
    }
}

fn plain_content(scope: &Scope) -> bool {
    scope.construction == ScopeConstruction::Constructed
        && scope.iteration_output == IterationOutput::Repeated
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && !scope.merge_dynamic_fields
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
}

fn at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    path.iter().try_fold(schema, |node, name| node.child(name))
}

struct ContextItem<'a> {
    /// Absolute schema location, not the identity used by runtime positions.
    schema_path: Vec<String>,
    schema: &'a SchemaNode,
    /// Only a real source iteration creates a live collection frame. The
    /// initial source root is available for lookup, but has no position.
    collection: Option<Vec<String>>,
}

/// Only ordinary static source frames are admitted here. Keeping this small
/// prevents a picker from offering private sequence items or owned-source reads.
struct Context<'a> {
    project: &'a Project,
    items: Vec<ContextItem<'a>>,
    private_items: BTreeSet<NodeId>,
    frame_bindings_proven: bool,
}

/// Flat input adapters carry their row collection outside the schema. Do not
/// mistake an explicitly declared structured document with a tabular filename
/// for rows; declaration precedence is the same as at the input boundary.
fn flat_rows(schema: &SchemaNode, options: &mapping::FormatOptions, path: Option<&str>) -> bool {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    if !children.iter().all(SchemaNode::is_scalar)
        || options.xml_document
        || options.json_document
        || options.json5
        || options.json_lines
        || options.local_xml_file_set
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.flextext.is_some()
        || options.protobuf.is_some()
        || options.pdf.is_some()
        || options.xbrl.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.wsdl.is_some()
    {
        return false;
    }
    if options.fixed_width.is_some() {
        return true;
    }
    let extension = path
        .and_then(|path| std::path::Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" | "txt" | "xlsx" | "db" | "sqlite" | "sqlite3" => true,
        "xml" | "json" | "json5" | "jsonl" | "ndjson" | "pdf" | "edi" | "x12" | "edifact"
        | "hl7" | "idoc" | "fin" | "swift" => false,
        _ => options.tabular_kind.is_some(),
    }
}

impl<'a> Context<'a> {
    fn iterate(&mut self, path: &[String]) -> Option<()> {
        // The runtime chooses a base by the first field, not by whether the
        // entire path resolves. Each walk starts a fresh collection identity:
        // a relative Lines walk inside Rows owns Lines, not Rows/Lines.
        let base = self.items.iter().rev().find(|item| {
            path.first()
                .is_none_or(|first| item.schema.child(first).is_some())
        });
        let (mut schema, mut schema_path, mut collection, rest, root_rows) =
            if let Some(base) = base {
                let root_rows = base.collection.is_none()
                    && base.schema_path.is_empty()
                    && flat_rows(
                        base.schema,
                        &self.project.source_options,
                        self.project.source_path.as_deref(),
                    );
                (
                    base.schema,
                    base.schema_path.clone(),
                    Vec::new(),
                    path,
                    root_rows,
                )
            } else if let Some((name, rest)) = path.split_first()
                && let Some(source) = self
                    .project
                    .extra_sources
                    .iter()
                    .find(|source| &source.name == name && source.dynamic_path.is_none())
            {
                (
                    &source.schema,
                    vec![name.clone()],
                    vec![name.clone()],
                    rest,
                    flat_rows(&source.schema, &source.options, Some(&source.path)),
                )
            } else {
                return None;
            };
        // A repeated initial root contributes a frame only when it is walked.
        // An already active item is no longer a collection at its own root.
        if (schema.repeating || root_rows) && base.is_none_or(|base| base.collection.is_none()) {
            self.items.push(ContextItem {
                schema_path: schema_path.clone(),
                schema,
                collection: Some(collection.clone()),
            });
        }
        for segment in rest {
            schema = schema.child(segment)?;
            schema_path.push(segment.clone());
            collection.push(segment.clone());
            if schema.repeating {
                self.items.push(ContextItem {
                    schema_path: schema_path.clone(),
                    schema,
                    collection: Some(collection.clone()),
                });
            }
        }
        if !collection.is_empty() && !schema.repeating {
            // A singleton walk appends a context item but no position. The
            // runtime associates retained positions from the end of that
            // context, so their owners can shift. Do not infer a pinned or
            // collection-qualified field's type from its earlier schema.
            if self.items.iter().any(|item| item.collection.is_some()) {
                self.frame_bindings_proven = false;
            }
            self.items.push(ContextItem {
                schema_path,
                schema,
                collection: None,
            });
        }
        Some(())
    }

    fn field(&self, path: &[String], frame: Option<&[String]>) -> Option<&'a SchemaNode> {
        if let Some(pin) = frame {
            if !self.frame_bindings_proven {
                return None;
            }
            return self
                .items
                .iter()
                .rev()
                .find(|item| {
                    item.collection.as_ref().is_some_and(|collection| {
                        collection == pin || !collection.is_empty() && pin.ends_with(collection)
                    })
                })
                .and_then(|item| at(item.schema, path));
        }
        // Match scalar_in_active_collection: an active prefix owns the path,
        // including its missing/non-scalar result. Do not retry a nested
        // same-named field or a different outer item when that lookup fails.
        if let Some((item, collection)) = self.items.iter().rev().find_map(|item| {
            item.collection
                .as_ref()
                .filter(|collection| !collection.is_empty() && path.starts_with(collection))
                .map(|collection| (item, collection))
        }) {
            if !self.frame_bindings_proven {
                return None;
            }
            return at(item.schema, &path[collection.len()..]);
        }
        self.items
            .iter()
            .rev()
            .find_map(|item| at(item.schema, path).filter(|schema| schema.is_scalar()))
            .or_else(|| {
                let (name, rest) = path.split_first()?;
                let source = self
                    .project
                    .extra_sources
                    .iter()
                    .find(|source| &source.name == name && source.dynamic_path.is_none())?;
                at(&source.schema, rest)
            })
    }

    fn available(
        &self,
        id: NodeId,
        seen: &mut BTreeSet<NodeId>,
        budget: &mut usize,
        depth: usize,
    ) -> bool {
        if depth > MAX_EXPRESSION_DEPTH
            || *budget == 0
            || self.private_items.contains(&id)
            || !seen.insert(id)
        {
            return false;
        }
        *budget -= 1;
        let Some(node) = self.project.graph.nodes.get(&id) else {
            return false;
        };
        let available = match node {
            Node::Const { .. } | Node::RuntimeValue { .. } | Node::RuntimeParameter { .. } => true,
            Node::SourceField { path, frame } => self
                .field(path, frame.as_deref())
                .is_some_and(SchemaNode::is_scalar),
            Node::Position { collection } => self.items.iter().any(|item| {
                item.collection
                    .as_ref()
                    .is_some_and(|live| live.ends_with(collection))
            }),
            Node::Call { function, args } => functions::builtin(function)
                .is_some_and(|builtin| builtin.accepts_arity(args.len())),
            Node::If { .. } => true,
            _ => false,
        } && node
            .dependencies()
            .iter()
            .all(|dependency| self.available(*dependency, seen, budget, depth + 1));
        seen.remove(&id);
        available
    }

    fn boolean(&self, id: NodeId, depth: usize) -> bool {
        if depth > MAX_EXPRESSION_DEPTH {
            return false;
        }
        match self.project.graph.nodes.get(&id) {
            Some(Node::Const {
                value: Value::Bool(_),
            }) => true,
            Some(Node::SourceField { path, frame }) => {
                self.field(path, frame.as_deref()).is_some_and(|schema| {
                    matches!(
                        schema.kind,
                        SchemaKind::Scalar {
                            ty: ScalarType::Bool
                        }
                    )
                })
            }
            Some(Node::Call { function, .. }) => functions::builtin(function)
                .is_some_and(|builtin| builtin.return_domain == functions::ScalarDomain::Boolean),
            Some(Node::RuntimeParameter {
                ty: ScalarType::Bool,
                ..
            }) => true,
            Some(Node::If { then, else_, .. }) => {
                self.boolean(*then, depth + 1) && self.boolean(*else_, depth + 1)
            }
            _ => false,
        }
    }
}

pub(crate) fn profile(
    project: &Project,
    root: &Scope,
    target: &SchemaNode,
    path: &[usize],
    mapping_document: bool,
) -> Profile {
    let unavailable = |reason| Profile {
        reason: Some(reason),
        conditions: Vec::new(),
    };
    if !mapping_document {
        return unavailable("Optional presence is available on primary and named targets only.");
    }
    if project.graph.nodes.len() > MAX_GRAPH_NODES || path.len() > MAX_EXPRESSION_DEPTH {
        return unavailable("This mapping exceeds the bounded presence picker's size limit.");
    }
    let mut context = Context {
        project,
        items: vec![ContextItem {
            schema_path: Vec::new(),
            schema: &project.source,
            collection: None,
        }],
        private_items: crate::graph_viewer::project_sequence_item_ids(project),
        frame_bindings_proven: true,
    };
    let mut scope = root;
    let mut schema = target;
    for &index in path {
        if scope.construction != ScopeConstruction::Constructed {
            return unavailable("This construction keeps its presence settings read-only.");
        }
        match &scope.iteration {
            ScopeIteration::None | ScopeIteration::OptionalPresence { .. } => {}
            ScopeIteration::Source(path) => {
                if context.iterate(path).is_none() {
                    return unavailable("The parent source frame is unavailable.");
                }
            }
            _ => return unavailable("This source route keeps its presence settings read-only."),
        }
        let Some(child) = scope.children.get(index) else {
            return unavailable("The selected target scope is unavailable.");
        };
        let Some(child_schema) = schema.child(&child.target_field) else {
            return unavailable("The selected target schema is unavailable.");
        };
        scope = child;
        schema = child_schema;
    }
    if !schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return unavailable("Optional presence requires a repeating target group.");
    }
    if !plain_content(scope)
        || !matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::OptionalPresence { .. }
        )
    {
        return unavailable(
            "Optional presence requires a plain constructed group without item controls or computed properties.",
        );
    }
    let conditions = project
        .graph
        .nodes
        .keys()
        .copied()
        .filter(|id| {
            let mut budget = MAX_EXPRESSION_NODES;
            context.available(*id, &mut BTreeSet::new(), &mut budget, 0) && context.boolean(*id, 0)
        })
        .collect();
    Profile {
        reason: None,
        conditions,
    }
}

pub(crate) fn show(ui: &mut egui::Ui, scope: &mut Scope, graph: &Graph, profile: &Profile) {
    if let Some(reason) = profile.reason {
        if scope.presence().is_some() {
            ui.weak(reason);
        }
        return;
    }
    // Another authoring control can change this owner earlier in the frame.
    if !plain_content(scope)
        || !matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::OptionalPresence { .. }
        )
    {
        return;
    }
    let mut enabled = scope.presence().is_some();
    if ui
        .add_enabled(
            enabled || !profile.conditions.is_empty(),
            egui::Checkbox::new(&mut enabled, "Optional presence"),
        )
        .on_disabled_hover_text("Add an existing Boolean expression in this parent context first.")
        .changed()
    {
        scope.iteration = if enabled && let Some(&condition) = profile.conditions.first() {
            ScopeIteration::OptionalPresence { condition }
        } else {
            ScopeIteration::None
        };
    }
    let Some(mut condition) = scope.presence() else {
        return;
    };
    crate::ui_kit::form_grid(ui, "presence_form", |form| {
        form.row("Presence condition", |ui| {
            let current = graph.nodes.get(&condition).map_or_else(
                || egui::WidgetText::from(format!("Missing expression · #{condition}")),
                |node| super::picker_label(ui, node, condition),
            );
            egui::ComboBox::from_id_salt("presence_condition")
                .selected_text(current)
                .width(ui.available_width().max(80.0))
                .wrap_mode(egui::TextWrapMode::Truncate)
                .show_ui(ui, |ui| {
                    for &id in &profile.conditions {
                        if let Some(node) = graph.nodes.get(&id) {
                            let label = super::picker_label(ui, node, id);
                            ui.selectable_value(&mut condition, id, label);
                        }
                    }
                });
        });
    });
    if !profile.conditions.contains(&condition) {
        ui.weak("The saved condition is retained; choose a Boolean expression available in this parent context.");
    }
    scope.iteration = ScopeIteration::OptionalPresence { condition };
}
