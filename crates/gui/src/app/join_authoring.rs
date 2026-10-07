//! Staged root-context inner joins and exact-owner output projections.

use super::*;
use ir::{SchemaKind, SchemaNode};
use mapping::{
    Binding, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, Node, ScopeIteration,
};

pub(super) const JOIN_REMOVAL_REASON: &str =
    "Joined output fields depend on this scope; removal is not available yet.";

#[derive(Clone)]
pub(super) struct JoinAuthoringDraft {
    owner: Owner,
    edit: Edit,
}

#[derive(Clone)]
struct Owner {
    document: MappingDocument,
    target_name: Option<String>,
    path: ScopePath,
    scope_bytes: Vec<u8>,
}

#[derive(Clone)]
enum Edit {
    Create {
        left: Vec<String>,
        right: Vec<String>,
        keys: Vec<EqualityPair>,
    },
    Keys {
        join: JoinId,
        left: Vec<String>,
        right: Vec<String>,
        keys: Vec<EqualityPair>,
    },
    Project {
        join: JoinId,
        collection: Vec<String>,
        field: Vec<String>,
        target: String,
        position: bool,
    },
}

#[derive(Clone)]
struct EqualityPair {
    left: Vec<String>,
    right: Vec<String>,
}

#[derive(Clone)]
struct Collection {
    path: Vec<String>,
    fields: Vec<Vec<String>>,
}

fn scalar_paths(schema: &SchemaNode, prefix: &[String], paths: &mut Vec<Vec<String>>) {
    if schema.repeating {
        return;
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => paths.push(prefix.to_vec()),
        SchemaKind::Group { children, .. } => {
            for child in children {
                let mut path = prefix.to_vec();
                path.push(child.name.clone());
                scalar_paths(child, &path, paths);
            }
        }
    }
}

fn collections(schema: &SchemaNode, prefix: &[String], choices: &mut Vec<Collection>) {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return;
    };
    if schema.repeating {
        let mut fields = Vec::new();
        for child in children {
            scalar_paths(child, std::slice::from_ref(&child.name), &mut fields);
        }
        if !fields.is_empty() {
            choices.push(Collection {
                path: prefix.to_vec(),
                fields,
            });
        }
        // A root-context collection cannot cross another repetition frame.
        return;
    }
    for child in children {
        let mut path = prefix.to_vec();
        path.push(child.name.clone());
        collections(child, &path, choices);
    }
}

fn choices(project: &Project) -> Vec<Collection> {
    let mut choices = Vec::new();
    collections(&project.source, &[], &mut choices);
    for source in &project.extra_sources {
        if source.dynamic_path.is_none() {
            collections(
                &source.schema,
                std::slice::from_ref(&source.name),
                &mut choices,
            );
        }
    }
    choices.sort_by(|left, right| left.path.cmp(&right.path));
    choices
}

fn label(path: &[String]) -> String {
    if path.is_empty() {
        "<current rows>".into()
    } else {
        path.join(" / ")
    }
}

fn picker(ui: &mut egui::Ui, id: &str, path: &mut Vec<String>, paths: &[Vec<String>]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(path))
        .width(200.0)
        .show_ui(ui, |ui| {
            for candidate in paths {
                ui.selectable_value(path, candidate.clone(), label(candidate));
            }
        });
}

fn field_paths(choices: &[Collection], collection: &[String]) -> Vec<Vec<String>> {
    choices
        .iter()
        .find(|choice| choice.path.as_slice() == collection)
        .map_or_else(Vec::new, |choice| choice.fields.clone())
}

fn pick_collection(
    ui: &mut egui::Ui,
    id: &str,
    collection: &mut Vec<String>,
    field: &mut Vec<String>,
    choices: &[Collection],
) {
    let before = collection.clone();
    let paths = choices
        .iter()
        .map(|choice| choice.path.clone())
        .collect::<Vec<_>>();
    picker(ui, id, collection, &paths);
    if *collection != before {
        *field = field_paths(choices, collection)
            .into_iter()
            .next()
            .unwrap_or_default();
    }
}

fn pick_key_collection(
    ui: &mut egui::Ui,
    id: &str,
    collection: &mut Vec<String>,
    keys: &mut [EqualityPair],
    left: bool,
    choices: &[Collection],
) {
    let before = collection.clone();
    let mut first = keys.first().map_or_else(Vec::new, |key| {
        if left {
            key.left.clone()
        } else {
            key.right.clone()
        }
    });
    pick_collection(ui, id, collection, &mut first, choices);
    if *collection != before {
        for key in keys {
            if left {
                key.left = first.clone();
            } else {
                key.right = first.clone();
            }
        }
    }
}

fn key_controls(ui: &mut egui::Ui, keys: &mut Vec<EqualityPair>) {
    let mut remove = None;
    let mut move_pair = None;
    let count = keys.len();
    for index in 0..count {
        ui.horizontal(|ui| {
            ui.label(format!("Equality pair {}", index + 1));
            if ui
                .add_enabled(
                    count > 1,
                    egui::Button::new(format!("Remove key pair {}", index + 1)),
                )
                .clicked()
            {
                remove = Some(index);
            }
            if ui
                .add_enabled(
                    index > 0,
                    egui::Button::new(format!("Move key pair {} up", index + 1)),
                )
                .clicked()
            {
                move_pair = Some((index, index - 1));
            }
            if ui
                .add_enabled(
                    index + 1 < count,
                    egui::Button::new(format!("Move key pair {} down", index + 1)),
                )
                .clicked()
            {
                move_pair = Some((index, index + 1));
            }
        });
    }
    if let Some(index) = remove {
        keys.remove(index);
    } else if let Some((from, to)) = move_pair {
        keys.swap(from, to);
    }
}

fn target_root(
    project: &Project,
    document: MappingDocument,
) -> Result<(&Scope, &SchemaNode), String> {
    match document {
        MappingDocument::Main => Ok((&project.root, &project.target)),
        MappingDocument::Target(index) => project
            .extra_targets
            .get(index)
            .map(|target| (&target.root, &target.schema))
            .ok_or_else(|| "The selected target no longer exists".into()),
        MappingDocument::Function(_) => Err("Join authoring belongs to a mapping target".into()),
    }
}

fn scope_parts<'a>(
    project: &'a Project,
    owner: &Owner,
) -> Result<(&'a Scope, &'a SchemaNode), String> {
    if let MappingDocument::Target(index) = owner.document
        && project.extra_targets.get(index).map(|target| &target.name) != owner.target_name.as_ref()
    {
        return Err("The selected target changed; start the join edit again".into());
    }
    let (root, schema) = target_root(project, owner.document)?;
    let mut scope = root;
    let mut schema = schema;
    for &index in &owner.path {
        if !matches!(scope.iteration, ScopeIteration::None)
            || scope.construction != mapping::ScopeConstruction::Constructed
        {
            return Err("Choose a scope in the ordinary root context".into());
        }
        scope = scope
            .children
            .get(index)
            .ok_or("The selected scope no longer exists")?;
        schema = schema
            .child(&scope.target_field)
            .ok_or("The selected target group no longer exists")?;
    }
    if !schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err("Choose a repeating target group".into());
    }
    if project
        .extra_sources
        .iter()
        .any(|source| source.dynamic_path.is_some())
    {
        return Err("This join editor uses static source collections".into());
    }
    Ok((scope, schema))
}

fn owner(app: &FerruleApp) -> Result<Owner, String> {
    let document = app.mapping_workspace.active;
    let (root, _) = target_root(&app.project, document)?;
    let scope = crate::auto_connect::scope_at(root, &app.selected_scope)
        .ok_or("The selected scope no longer exists")?;
    Ok(Owner {
        document,
        target_name: match document {
            MappingDocument::Target(index) => Some(app.project.extra_targets[index].name.clone()),
            _ => None,
        },
        path: app.selected_scope.clone(),
        scope_bytes: serde_json::to_vec(scope).map_err(|error| error.to_string())?,
    })
}

fn empty(scope: &Scope) -> bool {
    matches!(scope.iteration, ScopeIteration::None)
        && scope.construction == mapping::ScopeConstruction::Constructed
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.iteration_output == mapping::IterationOutput::Repeated
        && scope.bindings.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.children.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

pub(super) fn subtree_has_join(scope: &Scope) -> bool {
    scope.join().is_some()
        || scope.children.iter().any(subtree_has_join)
        || scope
            .dynamic_children
            .iter()
            .any(|child| subtree_has_join(&child.scope))
        || scope
            .concatenated()
            .is_some_and(|segments| segments.iter().any(subtree_has_join))
}

fn next_join_id(project: &Project) -> Result<JoinId, String> {
    fn visit(scope: &Scope, maximum: &mut Option<u64>) {
        if let Some((id, _)) = scope.join() {
            *maximum = Some(maximum.map_or(id.get(), |old| old.max(id.get())));
        }
        for child in &scope.children {
            visit(child, maximum);
        }
        for child in &scope.dynamic_children {
            visit(&child.scope, maximum);
        }
        if let Some(segments) = scope.concatenated() {
            for segment in segments.iter() {
                visit(segment, maximum);
            }
        }
    }
    let mut maximum = None;
    visit(&project.root, &mut maximum);
    for target in &project.extra_targets {
        visit(&target.root, &mut maximum);
    }
    for node in project.graph.nodes.values().chain(
        project
            .user_functions
            .values()
            .flat_map(|function| function.body.nodes.values()),
    ) {
        if let Node::JoinField { join, .. }
        | Node::JoinPosition { join }
        | Node::JoinAggregate { join, .. } = node
        {
            maximum = Some(maximum.map_or(join.get(), |old| old.max(join.get())));
        }
    }
    maximum.map_or(Ok(JoinId::new(1)), |id| {
        id.checked_add(1)
            .map(JoinId::new)
            .ok_or_else(|| "Join IDs are exhausted".into())
    })
}

fn projection_fields(schema: &SchemaNode, scope: &Scope) -> Vec<String> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Vec::new();
    };
    children
        .iter()
        .filter(|child| child.is_scalar() && !child.repeating)
        .filter(|child| {
            !scope
                .bindings
                .iter()
                .any(|binding| binding.target_field == child.name)
        })
        .map(|child| child.name.clone())
        .collect()
}

fn editable_keys(scope: &Scope, available: &[Collection]) -> Option<Edit> {
    if scope.construction != mapping::ScopeConstruction::Constructed {
        return None;
    }
    let (join, plan) = scope.join()?;
    let mut sources = plan.sources();
    let left = sources.next()?;
    let right = sources.next()?;
    if sources.next().is_some()
        || left.cardinality() != mapping::JoinSourceCardinality::Repeating
        || right.cardinality() != mapping::JoinSourceCardinality::Repeating
    {
        return None;
    }
    let left_fields = field_paths(available, left.collection());
    let right_fields = field_paths(available, right.collection());
    if left_fields.is_empty() || right_fields.is_empty() {
        return None;
    }
    let keys = plan
        .stages()
        .flat_map(|(_, conditions)| conditions.iter())
        .map(|key| {
            (key.left_collection() == left.collection()
                && left_fields
                    .iter()
                    .any(|path| path.as_slice() == key.left_path())
                && right_fields
                    .iter()
                    .any(|path| path.as_slice() == key.right_path()))
            .then(|| EqualityPair {
                left: key.left_path().to_vec(),
                right: key.right_path().to_vec(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Edit::Keys {
        join,
        left: left.collection().to_vec(),
        right: right.collection().to_vec(),
        keys,
    })
}

fn key_plan(
    left: JoinSource,
    right: JoinSource,
    keys: &[EqualityPair],
    available: &[Collection],
) -> Result<JoinPlan, String> {
    let Some((first, rest)) = keys.split_first() else {
        return Err("Choose at least one equality pair".into());
    };
    let left_fields = field_paths(available, left.collection());
    let right_fields = field_paths(available, right.collection());
    for (index, key) in keys.iter().enumerate() {
        if !left_fields.contains(&key.left) || !right_fields.contains(&key.right) {
            return Err(format!(
                "Equality pair {}: choose exact collection keys from the source schema",
                index + 1
            ));
        }
    }
    let mut conditions = JoinConditions::new(JoinKey::new(
        left.collection().to_vec(),
        first.left.clone(),
        first.right.clone(),
    ));
    for key in rest {
        conditions = conditions.and(JoinKey::new(
            left.collection().to_vec(),
            key.left.clone(),
            key.right.clone(),
        ));
    }
    JoinPlan::new(left, right, conditions).map_err(|error| error.to_string())
}

impl FerruleApp {
    pub(super) fn show_join_authoring(&mut self, ui: &mut egui::Ui, supplied_enabled: bool) {
        let enabled = supplied_enabled && self.ui_project_editing_enabled();
        let Ok(current) = owner(self) else {
            self.join_authoring_draft = None;
            return;
        };
        if self.join_authoring_draft.as_ref().is_some_and(|draft| {
            draft.owner.document != current.document
                || draft.owner.target_name != current.target_name
                || draft.owner.path != current.path
                || draft.owner.scope_bytes != current.scope_bytes
        }) {
            self.join_authoring_draft = None;
        }
        let available = choices(&self.project);
        let parts = scope_parts(&self.project, &current);
        let mut start = None;
        if self.join_authoring_draft.is_none() {
            let create =
                parts.as_ref().is_ok_and(|(scope, _)| empty(scope)) && available.len() >= 2;
            if ui
                .add_enabled(enabled && create, egui::Button::new("Join two collections"))
                .clicked()
            {
                let left = &available[0];
                let right = &available[1];
                start = Some(Edit::Create {
                    left: left.path.clone(),
                    right: right.path.clone(),
                    keys: vec![EqualityPair {
                        left: left.fields[0].clone(),
                        right: right.fields[0].clone(),
                    }],
                });
            }
            let keys = parts
                .as_ref()
                .ok()
                .and_then(|(scope, _)| editable_keys(scope, &available));
            if ui
                .add_enabled(
                    enabled && keys.is_some(),
                    egui::Button::new("Edit equality keys"),
                )
                .clicked()
            {
                start = keys;
            }
            let project = parts.as_ref().ok().and_then(|(scope, schema)| {
                let (join, plan) = scope.join()?;
                let inputs = plan
                    .sources()
                    .map(|source| source.collection().to_vec())
                    .collect::<Vec<_>>();
                if inputs.len() != 2
                    || plan.sources().any(|source| {
                        source.cardinality() != mapping::JoinSourceCardinality::Repeating
                    })
                {
                    return None;
                }
                let collection = available.iter().find(|choice| choice.path == inputs[0])?;
                if !available.iter().any(|choice| choice.path == inputs[1]) {
                    return None;
                }
                Some(Edit::Project {
                    join,
                    collection: collection.path.clone(),
                    field: collection.fields[0].clone(),
                    target: projection_fields(schema, scope).into_iter().next()?,
                    position: false,
                })
            });
            if ui
                .add_enabled(
                    enabled && project.is_some(),
                    egui::Button::new("Add joined output"),
                )
                .clicked()
            {
                start = project;
            }
            match parts {
                Err(reason) => {
                    ui.weak(reason);
                }
                Ok((scope, _)) if !create && !subtree_has_join(scope) => {
                    ui.weak("Start from an empty repeating target group with two static row collections.");
                }
                _ => {}
            }
        }
        if let Some(edit) = start {
            self.join_authoring_draft = Some(JoinAuthoringDraft {
                owner: current,
                edit,
            });
        }
        let Some(mut draft) = self.join_authoring_draft.take() else {
            return;
        };
        let mut commit = false;
        let mut cancel = false;
        ui.add_enabled_ui(enabled, |ui| {
            match &mut draft.edit {
                Edit::Create { left, right, keys } => {
                    ui.strong("Join rows with equal keys");
                    ui.horizontal(|ui| { ui.label("Left collection:"); pick_key_collection(ui, "join_left", left, keys, true, &available); });
                    if let Some(first) = keys.first_mut() {
                        ui.horizontal(|ui| { ui.label("Left key:"); picker(ui, "join_left_key", &mut first.left, &field_paths(&available, left)); });
                    }
                    ui.horizontal(|ui| { ui.label("Right collection:"); pick_key_collection(ui, "join_right", right, keys, false, &available); });
                    if let Some(first) = keys.first_mut() {
                        ui.horizontal(|ui| { ui.label("Right key:"); picker(ui, "join_right_key", &mut first.right, &field_paths(&available, right)); });
                    }
                    for (index, key) in keys.iter_mut().enumerate().skip(1) {
                        ui.label(format!("Additional equality pair {}", index + 1));
                        ui.horizontal(|ui| {
                            ui.label("Left key:");
                            picker(ui, &format!("join_left_key_{index}"), &mut key.left, &field_paths(&available, left));
                            ui.label("Right key:");
                            picker(ui, &format!("join_right_key_{index}"), &mut key.right, &field_paths(&available, right));
                        });
                    }
                    key_controls(ui, keys);
                    if ui.button("Add equality pair").clicked() {
                        keys.push(EqualityPair {
                            left: field_paths(&available, left).into_iter().next().unwrap_or_default(),
                            right: field_paths(&available, right).into_iter().next().unwrap_or_default(),
                        });
                    }
                    ui.weak("Every pair must match. Duplicate matches retain left-row order. Missing and nil keys do not match.");
                    commit = ui.button("Create inner join").clicked();
                }
                Edit::Keys { join, left, right, keys } => {
                    ui.strong(format!("Equality keys for join #{}", join.get()));
                    ui.label(format!("Left collection: {}", label(left)));
                    ui.label(format!("Right collection: {}", label(right)));
                    for (index, key) in keys.iter_mut().enumerate() {
                        ui.label(format!("Equality pair {}", index + 1));
                        ui.horizontal(|ui| {
                            ui.label("Left key:");
                            picker(ui, &format!("join_edit_left_key_{index}"), &mut key.left, &field_paths(&available, left));
                            ui.label("Right key:");
                            picker(ui, &format!("join_edit_right_key_{index}"), &mut key.right, &field_paths(&available, right));
                        });
                    }
                    key_controls(ui, keys);
                    if ui.button("Add equality pair").clicked() {
                        keys.push(EqualityPair {
                            left: field_paths(&available, left).into_iter().next().unwrap_or_default(),
                            right: field_paths(&available, right).into_iter().next().unwrap_or_default(),
                        });
                    }
                    ui.weak("Collections and joined output fields stay fixed. Equality pairs are checked in this order.");
                    commit = ui.button("Apply equality keys").clicked();
                }
                Edit::Project { join, collection, field, target, position } => {
                    ui.strong(format!("Output from join #{}", join.get()));
                    ui.checkbox(position, "Tuple position");
                    if !*position {
                        let inputs = scope_parts(&self.project, &draft.owner).ok().and_then(|(scope, _)| scope.join()).map(|(_, plan)| plan.sources().map(|source| source.collection().to_vec()).collect::<Vec<_>>()).unwrap_or_default();
                        let choices = available.iter().filter(|choice| inputs.contains(&choice.path)).cloned().collect::<Vec<_>>();
                        ui.horizontal(|ui| { ui.label("Joined collection:"); pick_collection(ui, "join_projection_source", collection, field, &choices); });
                        ui.horizontal(|ui| { ui.label("Joined field:"); picker(ui, "join_projection_field", field, &field_paths(&choices, collection)); });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Target field:");
                        let fields = scope_parts(&self.project, &draft.owner).map(|(scope, schema)| projection_fields(schema, scope)).unwrap_or_default();
                        egui::ComboBox::from_id_salt("join_projection_target").selected_text(target.as_str()).show_ui(ui, |ui| {
                            for field in fields { ui.selectable_value(target, field.clone(), field); }
                        });
                    });
                    commit = ui.button("Bind joined output").clicked();
                }
            }
            cancel = ui.button("Cancel join edit").clicked();
        });
        if commit {
            self.apply_join_draft(&draft, enabled);
        } else if !cancel {
            self.join_authoring_draft = Some(draft);
        }
    }

    fn apply_join_draft(&mut self, draft: &JoinAuthoringDraft, supplied_enabled: bool) {
        if !supplied_enabled || !self.ui_project_editing_enabled() {
            self.join_authoring_draft = Some(draft.clone());
            return;
        }
        match self.stage_join_draft(draft) {
            Ok((project, node)) => {
                self.project = project;
                self.join_authoring_draft = None;
                self.clear_diagnostic_navigation();
                // Key-only changes retain graph nodes, target bindings and every
                // live canvas. History restores the existing project/layout snapshots.
                if !matches!(&draft.edit, Edit::Keys { .. }) {
                    self.rebuild_mapping_canvases_after_retirement();
                }
                if let Some(node) = node {
                    let canvas = match self.mapping_workspace.active {
                        MappingDocument::Main => Some(&mut self.main_canvas),
                        MappingDocument::Target(index) => {
                            self.mapping_workspace.target_canvases.get_mut(&index)
                        }
                        MappingDocument::Function(_) => None,
                    };
                    if let Some(canvas) = canvas {
                        crate::canvas_layout::place_graph_node_without_overlap(
                            &mut canvas.snarl,
                            &canvas.node_sizes,
                            node,
                        );
                    }
                }
                self.diagnostics.clear();
                self.status = "join updated".into();
            }
            Err(error) => {
                self.join_authoring_draft = Some(draft.clone());
                self.status = "join edit failed".into();
                self.diagnostics.error("Join edit failed", error);
            }
        }
    }

    fn stage_join_draft(
        &self,
        draft: &JoinAuthoringDraft,
    ) -> Result<(Project, Option<NodeId>), String> {
        let current = owner(self)?;
        if current.document != draft.owner.document
            || current.target_name != draft.owner.target_name
            || current.path != draft.owner.path
            || current.scope_bytes != draft.owner.scope_bytes
        {
            return Err("The selected scope changed; start the join edit again".into());
        }
        let (scope, schema) = scope_parts(&self.project, &draft.owner)?;
        let available = choices(&self.project);
        let mut project = self.project.clone();
        let (iteration, binding) = match &draft.edit {
            Edit::Create { left, right, keys } => {
                if !empty(scope) {
                    return Err("Choose an empty target scope".into());
                }
                let Some((first, rest)) = keys.split_first() else {
                    return Err("Choose at least one equality pair".into());
                };
                let left_fields = field_paths(&available, left);
                let right_fields = field_paths(&available, right);
                for (index, key) in keys.iter().enumerate() {
                    if !left_fields.contains(&key.left) || !right_fields.contains(&key.right) {
                        return Err(format!(
                            "Equality pair {}: choose exact collection keys from the source schema",
                            index + 1
                        ));
                    }
                }
                let mut conditions = JoinConditions::new(JoinKey::new(
                    left.clone(),
                    first.left.clone(),
                    first.right.clone(),
                ));
                for key in rest {
                    conditions = conditions.and(JoinKey::new(
                        left.clone(),
                        key.left.clone(),
                        key.right.clone(),
                    ));
                }
                let plan = JoinPlan::new(
                    JoinSource::new(left.clone()),
                    JoinSource::new(right.clone()),
                    conditions,
                )
                .map_err(|error| error.to_string())?;
                (
                    Some(ScopeIteration::InnerJoin {
                        id: next_join_id(&self.project)?,
                        plan,
                    }),
                    None,
                )
            }
            Edit::Keys {
                join,
                left,
                right,
                keys,
            } => {
                let (actual, plan) = scope
                    .join()
                    .ok_or("The selected scope is no longer joined")?;
                if *join != actual || editable_keys(scope, &available).is_none() {
                    return Err("Choose an exact two-static-collection root join".into());
                }
                let mut sources = plan.sources();
                let actual_left = sources
                    .next()
                    .ok_or("The left joined collection is missing")?;
                let actual_right = sources
                    .next()
                    .ok_or("The right joined collection is missing")?;
                if actual_left.collection() != left.as_slice()
                    || actual_right.collection() != right.as_slice()
                {
                    return Err("Joined collections stay fixed during key editing".into());
                }
                let plan = key_plan(actual_left.clone(), actual_right.clone(), keys, &available)?;
                (Some(ScopeIteration::InnerJoin { id: actual, plan }), None)
            }
            Edit::Project {
                join,
                collection,
                field,
                target,
                position,
            } => {
                let (actual, plan) = scope
                    .join()
                    .ok_or("The selected scope is no longer joined")?;
                if *join != actual
                    || plan.sources().count() != 2
                    || plan.sources().any(|source| {
                        source.cardinality() != mapping::JoinSourceCardinality::Repeating
                    })
                {
                    return Err("Choose an exact two-collection join".into());
                }
                if !projection_fields(schema, scope).contains(target) {
                    return Err("Choose an unbound scalar target field".into());
                }
                if !*position
                    && (!plan
                        .sources()
                        .any(|source| source.collection() == collection.as_slice())
                        || !field_paths(&available, collection).contains(field))
                {
                    return Err("Choose a field from this exact joined collection".into());
                }
                let id = crate::graph_viewer::reserve_project_node_ids(&self.project, 1)?[0];
                project.graph.nodes.insert(
                    id,
                    if *position {
                        Node::JoinPosition { join: *join }
                    } else {
                        Node::JoinField {
                            join: *join,
                            collection: collection.clone(),
                            path: field.clone(),
                        }
                    },
                );
                (
                    None,
                    Some(Binding {
                        target_field: target.clone(),
                        node: id,
                    }),
                )
            }
        };
        let root = match draft.owner.document {
            MappingDocument::Main => &mut project.root,
            MappingDocument::Target(index) => &mut project.extra_targets[index].root,
            MappingDocument::Function(_) => {
                return Err("Join authoring belongs to a mapping target".into());
            }
        };
        let scope = scope_at_mut(root, &draft.owner.path);
        if let Some(iteration) = iteration {
            scope.iteration = iteration;
        }
        let node = binding.as_ref().map(|binding| binding.node);
        if let Some(binding) = binding {
            scope.bindings.push(binding);
        }
        let issues = cli::validate(&project);
        if !issues.is_empty() {
            return Err(issues
                .into_iter()
                .map(|issue| issue.message)
                .collect::<Vec<_>>()
                .join("; "));
        }
        Ok((project, node))
    }
}

#[cfg(test)]
#[path = "join_authoring_tests.rs"]
mod tests;
