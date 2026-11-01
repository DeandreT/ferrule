//! Schema-backed source path choices shared by scope and graph editors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use egui::Ui;
use ir::{SchemaKind, SchemaNode};
use mapping::{NamedSource, OwnedLookup, OwnedSourceRef, SourceOwner};

use crate::source_ownership::SourceOwnership;

#[path = "owned_aggregate_editor.rs"]
mod owned_aggregate_editor;
pub(crate) use owned_aggregate_editor::OWNED_AGGREGATE_OPS;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathChoice {
    path: Vec<String>,
    label: String,
}

#[derive(Debug, Default)]
struct CollectionEntry {
    value_paths: BTreeSet<Vec<String>>,
}

#[derive(Debug, Clone)]
struct CollectionChoice {
    path: PathChoice,
    values: Vec<PathChoice>,
}

#[derive(Debug, Clone)]
struct OwnedLookupChoice {
    source: OwnedSourceRef,
    values: Vec<PathChoice>,
}

/// Repeating source paths and their per-item scalar paths. Relative paths
/// are included for every schema node that can become an iteration frame.
#[derive(Debug, Clone)]
pub struct SourcePathCatalog {
    root_scope_paths: Vec<PathChoice>,
    scope_paths: Vec<PathChoice>,
    collections: Vec<CollectionChoice>,
    primary_root_fields: Vec<PathChoice>,
    primary_root_types: Vec<String>,
    open_scalar_objects: Vec<PathChoice>,
    primary_source_document_path: bool,
    xml_source_elements: Vec<XmlSourceElement>,
    ownership: SourceOwnership,
    lookup_choices: Vec<OwnedLookupChoice>,
    aggregate_choices: Vec<OwnedLookupChoice>,
    lookup_excluded_roots: BTreeSet<mapping::NodeId>,
}

impl SourcePathCatalog {
    pub fn new(source: &SchemaNode, extras: &[NamedSource]) -> Self {
        let ownership = SourceOwnership::new(source, extras);
        let aggregate_choices = owned_collection_choices(source, extras, &ownership);
        let lookup_choices = aggregate_choices
            .iter()
            .filter(|choice| !choice.values.is_empty())
            .cloned()
            .collect();
        let mut entries = BTreeMap::new();
        let mut root_scope_paths = BTreeSet::new();
        let mut scope_paths = BTreeSet::new();
        entries.insert(Vec::new(), CollectionEntry::default());
        root_scope_paths.insert(Vec::new());
        scope_paths.insert(Vec::new());

        add_scope_paths(source, &[], &mut root_scope_paths);
        add_item_context(source, &mut entries, &mut scope_paths);
        add_repeating_contexts(source, &mut entries, &mut scope_paths);

        let primary_root_fields = entries[&Vec::new()]
            .value_paths
            .iter()
            .filter(|path| {
                let borrowed = path.iter().map(String::as_str).collect::<Vec<_>>();
                ir::primary_root_schema_has_scalar(source, &borrowed)
            })
            .map(|path| PathChoice {
                path: path.clone(),
                label: path.join(" / "),
            })
            .collect();
        let open_scalar_objects = open_scalar_object_choices(source);
        let xml_source_elements = xml_source_element_choices(source);
        let primary_root_types = source
            .alternatives()
            .iter()
            .filter(|alternative| ir::primary_root_schema_has_type(source, &alternative.name))
            .map(|alternative| alternative.name.clone())
            .collect();

        for extra in extras {
            let prefix = vec![extra.name.clone()];
            add_scope_paths(&extra.schema, &prefix, &mut root_scope_paths);
            add_scope_paths(&extra.schema, &prefix, &mut scope_paths);
            add_resolvable_collections(&extra.schema, &prefix, &mut entries);
            if is_row_collection(extra) {
                add_collection(&prefix, &extra.schema, &mut entries);
                root_scope_paths.insert(prefix.clone());
                scope_paths.insert(prefix);
                add_item_context(&extra.schema, &mut entries, &mut scope_paths);
            }
            add_repeating_contexts(&extra.schema, &mut entries, &mut scope_paths);
        }

        let to_choices = |paths: BTreeSet<Vec<String>>| {
            paths
                .into_iter()
                .map(|path| PathChoice {
                    label: collection_label(&path),
                    path,
                })
                .collect()
        };
        let root_scope_paths = to_choices(root_scope_paths);
        let scope_paths = to_choices(scope_paths);
        let collections = entries
            .into_iter()
            .map(|(path, entry)| CollectionChoice {
                path: PathChoice {
                    label: collection_label(&path),
                    path,
                },
                values: entry
                    .value_paths
                    .into_iter()
                    .map(|path| PathChoice {
                        label: value_label(&path),
                        path,
                    })
                    .collect(),
            })
            .collect();
        Self {
            root_scope_paths,
            scope_paths,
            collections,
            primary_root_fields,
            primary_root_types,
            open_scalar_objects,
            primary_source_document_path: false,
            xml_source_elements,
            ownership,
            lookup_choices,
            aggregate_choices,
            lookup_excluded_roots: BTreeSet::new(),
        }
    }

    /// Records whether the project already holds owned forms, so every new
    /// read stays owner-qualified once ownership is in use.
    pub(crate) fn with_project_ownership(mut self, project: &mapping::Project) -> Self {
        self.lookup_excluded_roots = crate::source_ownership::lookup_excluded_roots(project);
        self.ownership = std::mem::take(&mut self.ownership)
            .with_project_usage(crate::source_ownership::project_uses_ownership(project));
        self
    }

    pub(crate) fn ownership(&self) -> &SourceOwnership {
        &self.ownership
    }

    pub(crate) fn lookup_editable(&self, graph: &mapping::Graph, node: mapping::NodeId) -> bool {
        !matches!(graph.nodes.get(&node), Some(mapping::Node::OwnedLookup(lookup))
            if !self.lookup_owner_available(&lookup.collection.owner))
            && !matches!(graph.nodes.get(&node), Some(mapping::Node::OwnedAggregate(aggregate))
                if !self.aggregate_form_supported(aggregate)
                    || !self.aggregate_owner_available(&aggregate.collection.owner, aggregate.function))
            && !crate::source_ownership::lookup_read_only_in_graph(
                graph,
                &self.lookup_excluded_roots,
            )
            .contains(&node)
    }

    pub(crate) fn lookup_binding_supported(&self, root: &mapping::Scope, chain: &[String]) -> bool {
        crate::source_ownership::lookup_binding_supported(root, chain, &self.ownership)
    }

    pub(crate) fn first_owned_lookup(&self, matches: mapping::NodeId) -> Option<OwnedLookup> {
        let choice = self.lookup_choices.first()?;
        let path = choice.values.first()?.path.clone();
        Some(OwnedLookup {
            collection: choice.source.clone(),
            key: path.clone(),
            matches,
            value: path,
        })
    }

    pub(crate) fn lookup_owner_available(&self, owner: &SourceOwner) -> bool {
        self.lookup_choices
            .iter()
            .any(|choice| &choice.source.owner == owner)
    }

    /// All three pickers share one exact endpoint-local catalogue. Saved paths
    /// are never repaired during inspection, even when they are no longer offered.
    pub(crate) fn show_owned_lookup_editor(&self, ui: &mut Ui, lookup: &mut OwnedLookup) {
        // Snarl gives inline editors an unbounded measuring Ui. Bound the
        // first combo before it computes its text position inside the grid.
        ui.set_max_width(crate::theme::editor_size::TYPE_PATH_PICKER);
        let owners = self.ownership.lookup_owners();
        let owner_available = self.lookup_owner_available(&lookup.collection.owner);
        if !owner_available {
            ui.label(crate::source_ownership::collection_label(
                &lookup.collection.owner,
                &lookup.collection.path,
            ));
            ui.label(format!("Key: {}", value_label(&lookup.key)));
            ui.label(format!("Return field: {}", value_label(&lookup.value)));
            ui.weak("This lookup owner is unavailable. Stored paths and wiring are unchanged.");
            return;
        }
        egui::Grid::new(ui.scope_id().with("owned_lookup_paths")).show(ui, |ui| {
            ui.label("Input");
            let mut owner = lookup.collection.owner.clone();
            egui::ComboBox::from_id_salt(ui.scope_id().with("lookup_owner"))
                .selected_text(lookup_owner_label(&owner))
                .width(crate::theme::editor_size::PATH_PICKER)
                .show_ui(ui, |ui| {
                    for candidate in &owners {
                        if self
                            .lookup_choices
                            .iter()
                            .any(|choice| &choice.source.owner == candidate)
                        {
                            ui.selectable_value(
                                &mut owner,
                                candidate.clone(),
                                lookup_owner_label(candidate),
                            );
                        }
                    }
                });
            if owner != lookup.collection.owner {
                let choice = self
                    .lookup_choices
                    .iter()
                    .find(|choice| {
                        choice.source.owner == owner && choice.source.path == lookup.collection.path
                    })
                    .or_else(|| {
                        self.lookup_choices
                            .iter()
                            .find(|choice| choice.source.owner == owner)
                    });
                if let Some(choice) = choice {
                    lookup.collection = choice.source.clone();
                    reset_lookup_fields(lookup, choice);
                }
            }
            ui.end_row();
            ui.label("Collection");
            let previous = lookup.collection.path.clone();
            egui::ComboBox::from_id_salt(ui.scope_id().with("lookup_owned_collection"))
                .selected_text(collection_label(&previous))
                .width(crate::theme::editor_size::PATH_PICKER)
                .show_ui(ui, |ui| {
                    for choice in &self.lookup_choices {
                        if choice.source.owner == lookup.collection.owner {
                            ui.selectable_value(
                                &mut lookup.collection.path,
                                choice.source.path.clone(),
                                collection_label(&choice.source.path),
                            );
                        }
                    }
                });
            let choice = self
                .lookup_choices
                .iter()
                .find(|choice| choice.source == lookup.collection);
            if previous != lookup.collection.path
                && let Some(choice) = choice
            {
                reset_lookup_fields(lookup, choice);
            }
            ui.end_row();
            let fields = choice.map_or_else(Vec::new, |choice| choice.values.iter().collect());
            ui.label("Key");
            show_path_picker(
                ui,
                ui.scope_id().with("lookup_owned_key"),
                &mut lookup.key,
                &fields,
                value_label,
            );
            ui.end_row();
            ui.label("Return field");
            show_path_picker(
                ui,
                ui.scope_id().with("lookup_owned_value"),
                &mut lookup.value,
                &fields,
                value_label,
            );
            ui.end_row();
        });
    }

    /// Offers owner-qualified collections. Returns a selector only after an
    /// explicit click, so inspecting a legacy or owned scope never converts it.
    pub(crate) fn show_owned_collection_picker(
        &self,
        ui: &mut Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        current: &str,
        selected: Option<&OwnedSourceRef>,
    ) -> Option<OwnedSourceRef> {
        let mut chosen = None;
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(current)
            .width(ui.available_width().max(80.0))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, |ui| {
                for choice in self.ownership.collections() {
                    if ui
                        .selectable_label(selected == Some(&choice.source), &choice.label)
                        .clicked()
                    {
                        chosen = Some(choice.source.clone());
                    }
                }
            });
        chosen
    }

    pub(crate) fn with_primary_source_options(mut self, options: &mapping::FormatOptions) -> Self {
        self.primary_source_document_path = options.local_xml_file_set;
        if options.local_xml_file_set {
            self.lookup_choices
                .retain(|choice| choice.source.owner != SourceOwner::Primary);
            self.aggregate_choices
                .retain(|choice| choice.source.owner != SourceOwner::Primary);
        }
        self
    }

    pub(crate) fn primary_source_document_path_available(&self) -> bool {
        self.primary_source_document_path
    }

    pub(crate) fn xml_source_elements_available(&self) -> bool {
        !self.xml_source_elements.is_empty()
    }

    pub(crate) fn first_xml_source_element(&self) -> Option<(Vec<String>, Box<SchemaNode>)> {
        self.xml_source_elements
            .first()
            .map(|choice| (choice.path.path.clone(), choice.schema.clone()))
    }

    // Return a replacement only after an explicit click, including a click on
    // the current path. Inspection never normalizes imported frame/schema data.
    pub(crate) fn show_xml_source_element_picker(
        &self,
        ui: &mut Ui,
        path: &[String],
    ) -> Option<(Vec<String>, Box<SchemaNode>)> {
        let mut chosen = None;
        ui.add_enabled_ui(self.xml_source_elements_available(), |ui| {
            egui::ComboBox::from_id_salt(ui.scope_id().with("xml_source_element"))
                .selected_text("Choose source element")
                .width(crate::theme::editor_size::PATH_PICKER)
                .show_ui(ui, |ui| {
                    for choice in &self.xml_source_elements {
                        if ui
                            .selectable_label(choice.path.path == path, &choice.path.label)
                            .clicked()
                        {
                            chosen = Some((choice.path.path.clone(), choice.schema.clone()));
                        }
                    }
                });
        });
        chosen
    }

    pub(crate) fn first_open_scalar_object(&self) -> Option<Vec<String>> {
        self.open_scalar_objects
            .first()
            .map(|choice| choice.path.clone())
    }

    // A click is an explicit replacement of the stored object/frame selection.
    // Merely opening this picker must leave imported or framed metadata intact.
    pub(crate) fn show_open_scalar_object_picker(
        &self,
        ui: &mut Ui,
        path: &mut Vec<String>,
    ) -> bool {
        let selected = self
            .open_scalar_objects
            .iter()
            .find(|choice| choice.path == *path)
            .map_or_else(|| open_object_label(path), |choice| choice.label.clone());
        let mut chosen = false;
        egui::ComboBox::from_id_salt(ui.scope_id().with("dynamic_source_object"))
            .selected_text(selected)
            .width(crate::theme::editor_size::PATH_PICKER)
            .show_ui(ui, |ui| {
                for choice in &self.open_scalar_objects {
                    if ui
                        .selectable_label(choice.path == *path, &choice.label)
                        .clicked()
                    {
                        *path = choice.path.clone();
                        chosen = true;
                    }
                }
            });
        chosen
    }

    pub(crate) fn first_primary_root_field(&self) -> Option<Vec<String>> {
        self.primary_root_fields
            .first()
            .map(|choice| choice.path.clone())
    }

    pub(crate) fn first_primary_root_type(&self) -> Option<String> {
        self.primary_root_types.first().cloned()
    }

    pub(crate) fn show_primary_root_field_picker(&self, ui: &mut Ui, path: &mut Vec<String>) {
        let choices = self.primary_root_fields.iter().collect::<Vec<_>>();
        show_path_picker(
            ui,
            ui.scope_id().with("primary_root_field"),
            path,
            &choices,
            value_label,
        );
        if !self
            .primary_root_fields
            .iter()
            .any(|choice| choice.path == *path)
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Choose a supported primary source field.",
            );
        }
    }

    pub(crate) fn show_primary_root_type_picker(&self, ui: &mut Ui, identity: &mut String) {
        egui::ComboBox::from_id_salt(ui.scope_id().with("primary_root_type"))
            .selected_text(identity.as_str())
            .width(crate::theme::editor_size::PATH_PICKER)
            .show_ui(ui, |ui| {
                for choice in &self.primary_root_types {
                    ui.selectable_value(identity, choice.clone(), choice);
                }
            });
        if !self.primary_root_types.contains(identity) {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Choose a declared primary XML type.",
            );
        }
    }

    pub fn show_scope_picker(
        &self,
        ui: &mut Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        path: &mut Vec<String>,
        nested: bool,
    ) {
        let paths = if nested {
            &self.scope_paths
        } else {
            &self.root_scope_paths
        };
        let choices: Vec<&PathChoice> = paths.iter().collect();
        show_path_picker(ui, id_salt, path, &choices, collection_label);
    }

    pub fn show_collection_picker(
        &self,
        ui: &mut Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        path: &mut Vec<String>,
    ) {
        let choices: Vec<&PathChoice> =
            self.collections.iter().map(|choice| &choice.path).collect();
        show_path_picker(ui, id_salt, path, &choices, collection_label);
    }

    pub fn show_value_picker(
        &self,
        ui: &mut Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        collection: &[String],
        path: &mut Vec<String>,
    ) {
        let choices: Vec<&PathChoice> = self
            .collections
            .iter()
            .find(|choice| choice.path.path == collection)
            .or_else(|| self.collections.first())
            .map_or_else(Vec::new, |choice| choice.values.iter().collect());
        show_path_picker(ui, id_salt, path, &choices, value_label);
    }
}

fn lookup_owner_label(owner: &SourceOwner) -> String {
    match owner {
        SourceOwner::Primary => "Primary input".into(),
        SourceOwner::Named { name } => format!("Named input: {name}"),
    }
}

fn reset_lookup_fields(lookup: &mut OwnedLookup, choice: &OwnedLookupChoice) {
    let Some(first) = choice.values.first() else {
        return;
    };
    for path in [&mut lookup.key, &mut lookup.value] {
        if !choice.values.iter().any(|value| value.path == *path) {
            *path = first.path.clone();
        }
    }
}

/// Unlike the legacy catalogue, these paths never merge aliases or iteration
/// contexts. A nested repeating path remains absolute within its selected owner.
fn owned_collection_choices(
    primary: &SchemaNode,
    extras: &[NamedSource],
    ownership: &SourceOwnership,
) -> Vec<OwnedLookupChoice> {
    let mut choices = Vec::new();
    for owner in ownership.lookup_owners() {
        let schema = match &owner {
            SourceOwner::Primary => primary,
            SourceOwner::Named { name } => {
                let Some(source) = extras.iter().find(|source| &source.name == name) else {
                    continue;
                };
                if source.options.local_xml_file_set {
                    continue;
                }
                &source.schema
            }
        };
        if !lookup_tree_supported(schema) {
            continue;
        }
        let mut pending = vec![(schema, Vec::new())];
        let mut endpoint = Vec::new();
        while let Some((node, path)) = pending.pop() {
            if path.is_empty() || node.repeating {
                let mut fields = BTreeSet::new();
                add_scalar_paths(node, &mut Vec::new(), &mut fields);
                endpoint.push(OwnedLookupChoice {
                    source: OwnedSourceRef {
                        owner: owner.clone(),
                        path: path.clone(),
                    },
                    values: fields
                        .into_iter()
                        .map(|path| PathChoice {
                            label: value_label(&path),
                            path,
                        })
                        .collect(),
                });
            }
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children.iter().rev() {
                    let mut child_path = path.clone();
                    child_path.push(child.name.clone());
                    pending.push((child, child_path));
                }
            }
        }
        choices.extend(endpoint);
    }
    choices
}

fn lookup_tree_supported(schema: &SchemaNode) -> bool {
    let mut pending = vec![(schema, 0)];
    let mut visited = 0;
    while let Some((node, depth)) = pending.pop() {
        visited += 1;
        if visited > 4096 || depth > 64 || node.recursive_ref.is_some() {
            return false;
        }
        if let SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } = &node.kind
        {
            if !alternatives.is_empty()
                || dynamic.is_some()
                || children.len() + pending.len() > 4096
            {
                return false;
            }
            let mut names = BTreeSet::new();
            if children.iter().any(|child| !names.insert(&child.name)) {
                return false;
            }
            pending.extend(children.iter().map(|child| (child, depth + 1)));
        }
    }
    true
}

#[derive(Debug, Clone)]
struct XmlSourceElement {
    path: PathChoice,
    schema: Box<SchemaNode>,
}

// These are conservative authoring limits, not XML writer limits. Census the
// complete primary tree before the recursive public input-policy check or any
// schema clone. Owned metadata outside this small ordinary subset is refused
// without walking or copying it. Existing imported serializer nodes stay valid.
const XML_CHOICE_SCHEMA_NODES: usize = 256;
const XML_CHOICE_SCHEMA_DEPTH: usize = 16;
const XML_CHOICE_SCHEMA_TEXT_BYTES: usize = 64 * 1024;
const XML_SOURCE_CHOICES: usize = 32;

fn xml_choice_tree_is_bounded(source: &SchemaNode) -> bool {
    let mut pending = vec![(source, 1)];
    let mut nodes = 0;
    let mut text_bytes = 0usize;
    while let Some((schema, depth)) = pending.pop() {
        nodes += 1;
        if nodes > XML_CHOICE_SCHEMA_NODES
            || depth > XML_CHOICE_SCHEMA_DEPTH
            || !xml_choice_metadata_is_small(schema)
        {
            return false;
        }
        let namespace_bytes = schema
            .xml_namespace
            .as_ref()
            .and_then(ir::XmlNamespace::uri)
            .map_or(0, str::len);
        if schema.name.len() > 4096 || namespace_bytes > 4096 {
            return false;
        }
        text_bytes += schema.name.len() + namespace_bytes;
        if text_bytes > XML_CHOICE_SCHEMA_TEXT_BYTES {
            return false;
        }
        if let SchemaKind::Group {
            children,
            alternatives,
            required,
            xml_restricted_alternatives,
            dynamic,
        } = &schema.kind
        {
            if !alternatives.is_empty()
                || !required.is_empty()
                || !xml_restricted_alternatives.is_empty()
                || dynamic.is_some()
                || children.len() > XML_CHOICE_SCHEMA_NODES - nodes
                || pending.len() + children.len() > XML_CHOICE_SCHEMA_NODES - nodes
            {
                return false;
            }
            pending.extend(children.iter().map(|child| (child, depth + 1)));
        }
    }
    true
}

fn xml_choice_metadata_is_small(schema: &SchemaNode) -> bool {
    schema.xml_name_alternatives.is_empty()
        && schema.xml_wildcard_namespace.is_none()
        && schema.recursive_ref.is_none()
        && schema.fixed.is_none()
        && schema.default.is_none()
        && schema.json_allowed_values.is_none()
        && schema.numeric_range.is_none()
        && schema.json_multiple_of.is_none()
        && schema.item_count_range.is_none()
        && schema.json_contains.is_none()
        && schema.json_dependent_schemas.is_none()
        && schema.property_count_range.is_none()
        && schema.json_property_dependencies.is_none()
        && schema.json_pattern_property_names.is_none()
        && schema.json_property_names.is_none()
        && schema.string_length_range.is_none()
        && schema.json_patterns.is_none()
        && schema.json_formats.is_empty()
        && schema.value_generation.is_none()
        && schema.xml_default_type.is_none()
        && schema.xml_repeating_sequences.is_empty()
        && schema.xml_repeating_choices.is_empty()
        && schema.database_relation.is_none()
}

fn xml_source_element_choices(source: &SchemaNode) -> Vec<XmlSourceElement> {
    if !xml_choice_tree_is_bounded(source)
        || !ir::xml_structured_document_input_is_supported(source)
    {
        return Vec::new();
    }
    let mut pending = vec![(source, Vec::new())];
    let mut selected = Vec::new();
    while let Some((schema, path)) = pending.pop() {
        // Descendants beyond a repeated ancestor require an explicit frame and
        // are not offered by this primary-source creation route.
        if schema.repeating {
            continue;
        }
        if let SchemaKind::Group { children, .. } = &schema.kind {
            if ir::xml_structured_document_input_is_supported(schema) {
                if selected.len() == XML_SOURCE_CHOICES {
                    return Vec::new();
                }
                selected.push((schema, path.clone()));
            }
            for child in children.iter().rev() {
                let mut child_path = path.clone();
                child_path.push(child.name.clone());
                pending.push((child, child_path));
            }
        }
    }
    // At most 32 clones of a census-bounded 256-node/64-KiB tree: no clone
    // precedes either bound, including the public policy's recursive walk.
    selected
        .into_iter()
        .map(|(schema, path)| XmlSourceElement {
            path: PathChoice {
                label: open_object_label(&path),
                path,
            },
            schema: Box::new((*schema).clone()),
        })
        .collect()
}

fn open_object_label(path: &[String]) -> String {
    if path.is_empty() {
        "<source root>".to_owned()
    } else {
        path.join(" / ")
    }
}

fn open_scalar_object_choices(source: &SchemaNode) -> Vec<PathChoice> {
    let mut pending = vec![(source, Vec::new())];
    let mut choices = Vec::new();
    while let Some((schema, path)) = pending.pop() {
        if schema.repeating || !schema.alternatives().is_empty() {
            continue;
        }
        if let SchemaKind::Group { children, .. } = &schema.kind {
            if schema
                .dynamic_fields()
                .is_some_and(|value| !value.repeating && value.is_scalar())
            {
                choices.push(PathChoice {
                    label: open_object_label(&path),
                    path: path.clone(),
                });
            }
            for child in children.iter().rev() {
                let mut child_path = path.clone();
                child_path.push(child.name.clone());
                pending.push((child, child_path));
            }
        }
    }
    choices
}

fn show_path_picker(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    path: &mut Vec<String>,
    choices: &[&PathChoice],
    fallback_label: fn(&[String]) -> String,
) {
    let selected = choices
        .iter()
        .find(|choice| choice.path == *path)
        .map_or_else(|| fallback_label(path), |choice| choice.label.clone());
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected)
        .width(ui.available_width().max(80.0))
        .wrap_mode(egui::TextWrapMode::Truncate)
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(path, choice.path.clone(), &choice.label);
            }
        });
}

fn collection_label(path: &[String]) -> String {
    if path.is_empty() {
        "Current rows".to_string()
    } else {
        path.join(" / ")
    }
}

fn value_label(path: &[String]) -> String {
    if path.is_empty() {
        "Current item value".to_string()
    } else {
        path.join(" / ")
    }
}

fn is_row_collection(source: &NamedSource) -> bool {
    if source.schema.repeating {
        return true;
    }
    let extension = Path::new(&source.path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "csv" | "db" | "sqlite" | "sqlite3"
    )
}

fn add_item_context(
    schema: &SchemaNode,
    entries: &mut BTreeMap<Vec<String>, CollectionEntry>,
    scope_paths: &mut BTreeSet<Vec<String>>,
) {
    let entry = entries.entry(Vec::new()).or_default();
    add_scalar_paths(schema, &mut Vec::new(), &mut entry.value_paths);
    add_scope_paths(schema, &[], scope_paths);
    add_resolvable_collections(schema, &[], entries);
}

fn add_repeating_contexts(
    schema: &SchemaNode,
    entries: &mut BTreeMap<Vec<String>, CollectionEntry>,
    scope_paths: &mut BTreeSet<Vec<String>>,
) {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return;
    };
    for child in children {
        if child.repeating {
            add_item_context(child, entries, scope_paths);
        }
        add_repeating_contexts(child, entries, scope_paths);
    }
}

fn add_scope_paths(schema: &SchemaNode, prefix: &[String], paths: &mut BTreeSet<Vec<String>>) {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return;
    };
    for child in children {
        let mut path = prefix.to_vec();
        path.push(child.name.clone());
        if child.repeating {
            paths.insert(path.clone());
        }
        add_scope_paths(child, &path, paths);
    }
}

/// Lookup and aggregate paths cannot cross a repeated value: the repeated
/// node must be the last segment. A scope can first iterate that node, after
/// which its descendants appear as relative choices from a new context.
fn add_resolvable_collections(
    schema: &SchemaNode,
    prefix: &[String],
    entries: &mut BTreeMap<Vec<String>, CollectionEntry>,
) {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return;
    };
    for child in children {
        let mut path = prefix.to_vec();
        path.push(child.name.clone());
        if child.repeating {
            add_collection(&path, child, entries);
        } else {
            add_resolvable_collections(child, &path, entries);
        }
    }
}

fn add_collection(
    path: &[String],
    item_schema: &SchemaNode,
    entries: &mut BTreeMap<Vec<String>, CollectionEntry>,
) {
    let entry = entries.entry(path.to_vec()).or_default();
    add_scalar_paths(item_schema, &mut Vec::new(), &mut entry.value_paths);
}

fn add_scalar_paths(
    schema: &SchemaNode,
    prefix: &mut Vec<String>,
    paths: &mut BTreeSet<Vec<String>>,
) {
    match &schema.kind {
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => {
            paths.insert(prefix.clone());
        }
        SchemaKind::Group { children, .. } => {
            for child in children {
                if child.repeating {
                    continue;
                }
                prefix.push(child.name.clone());
                add_scalar_paths(child, prefix, paths);
                prefix.pop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, ScalarTypeSet};

    fn nested_source() -> SchemaNode {
        SchemaNode::group(
            "orders",
            vec![
                SchemaNode::scalar("date", ScalarType::String),
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("id", ScalarType::Int),
                        SchemaNode::group(
                            "Items",
                            vec![
                                SchemaNode::group(
                                    "Item",
                                    vec![
                                        SchemaNode::scalar("sku", ScalarType::String),
                                        SchemaNode::scalar("qty", ScalarType::Int),
                                    ],
                                )
                                .repeating(),
                            ],
                        ),
                    ],
                )
                .repeating(),
            ],
        )
    }

    fn has_collection(catalog: &SourcePathCatalog, path: &[&str]) -> bool {
        catalog.collections.iter().any(|choice| {
            choice
                .path
                .path
                .iter()
                .map(String::as_str)
                .eq(path.iter().copied())
        })
    }

    fn has_scope_path(catalog: &SourcePathCatalog, path: &[&str]) -> bool {
        catalog.scope_paths.iter().any(|choice| {
            choice
                .path
                .iter()
                .map(String::as_str)
                .eq(path.iter().copied())
        })
    }

    fn has_root_scope_path(catalog: &SourcePathCatalog, path: &[&str]) -> bool {
        catalog.root_scope_paths.iter().any(|choice| {
            choice
                .path
                .iter()
                .map(String::as_str)
                .eq(path.iter().copied())
        })
    }

    fn has_value(catalog: &SourcePathCatalog, collection: &[&str], value: &[&str]) -> bool {
        catalog.collections.iter().any(|choice| {
            choice
                .path
                .path
                .iter()
                .map(String::as_str)
                .eq(collection.iter().copied())
                && choice.values.iter().any(|candidate| {
                    candidate
                        .path
                        .iter()
                        .map(String::as_str)
                        .eq(value.iter().copied())
                })
        })
    }

    #[test]
    fn includes_absolute_and_iteration_relative_collection_paths() {
        let catalog = SourcePathCatalog::new(&nested_source(), &[]);
        assert!(has_collection(&catalog, &[]));
        assert!(has_collection(&catalog, &["Order"]));
        assert!(has_scope_path(&catalog, &["Order", "Items", "Item"]));
        assert!(has_root_scope_path(&catalog, &["Order", "Items", "Item"]));
        assert!(!has_root_scope_path(&catalog, &["Items", "Item"]));
        assert!(!has_collection(&catalog, &["Order", "Items", "Item"]));
        assert!(has_collection(&catalog, &["Items", "Item"]));
        assert!(has_value(&catalog, &["Order"], &["id"]));
        assert!(!has_value(&catalog, &["Order"], &["Items", "Item", "sku"]));
        assert!(has_value(&catalog, &["Items", "Item"], &["qty"]));
    }

    #[test]
    fn names_flat_extra_sources_and_exposes_their_columns() {
        let extra = NamedSource {
            name: "customers".to_string(),
            path: "customers.csv".to_string(),
            schema: SchemaNode::group(
                "customer",
                vec![
                    SchemaNode::scalar("id", ScalarType::Int),
                    SchemaNode::scalar("name", ScalarType::String),
                ],
            ),
            options: Default::default(),
            dynamic_path: None,
        };
        let catalog = SourcePathCatalog::new(&nested_source(), &[extra]);
        assert!(has_collection(&catalog, &["customers"]));
        assert!(has_value(&catalog, &["customers"], &["id"]));
        assert!(has_value(&catalog, &["customers"], &["name"]));
    }

    #[test]
    fn non_collection_extra_root_is_only_a_prefix_for_repeating_descendants() {
        let extra = NamedSource {
            name: "catalog".to_string(),
            path: "catalog.xml".to_string(),
            schema: SchemaNode::group(
                "catalog",
                vec![SchemaNode::group(
                    "Products",
                    vec![
                        SchemaNode::group(
                            "Product",
                            vec![SchemaNode::scalar("code", ScalarType::String)],
                        )
                        .repeating(),
                    ],
                )],
            ),
            options: Default::default(),
            dynamic_path: None,
        };
        let catalog = SourcePathCatalog::new(&nested_source(), &[extra]);
        assert!(!has_collection(&catalog, &["catalog"]));
        assert!(has_collection(
            &catalog,
            &["catalog", "Products", "Product"]
        ));
        assert!(has_value(
            &catalog,
            &["catalog", "Products", "Product"],
            &["code"]
        ));
    }

    #[test]
    fn union_leaves_are_available_as_complete_collection_values() {
        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
            panic!("test union is valid");
        };
        let source = SchemaNode::group(
            "root",
            vec![
                SchemaNode::group("rows", vec![SchemaNode::scalar_union("value", types)])
                    .repeating(),
            ],
        );

        let catalog = SourcePathCatalog::new(&source, &[]);

        assert!(has_collection(&catalog, &["rows"]));
        assert!(has_value(&catalog, &["rows"], &["value"]));
    }

    #[test]
    fn owned_lookup_catalog_never_merges_aliases_fields_or_nested_contexts() {
        let row = |field: &str| {
            SchemaNode::group("Rows", vec![SchemaNode::scalar(field, ScalarType::String)])
                .repeating()
        };
        let primary = SchemaNode::group(
            "Root",
            vec![
                row("primary"),
                SchemaNode::group("Ref", vec![row("shadow")]),
            ],
        );
        let named = NamedSource {
            name: "Ref".into(),
            path: "reference.json".into(),
            schema: SchemaNode::group("Root", vec![row("named")]),
            options: Default::default(),
            dynamic_path: None,
        };
        let catalog = SourcePathCatalog::new(&primary, &[named]);
        for (owner, path, expected) in [
            (SourceOwner::Primary, vec!["Rows"], "primary"),
            (SourceOwner::Primary, vec!["Ref", "Rows"], "shadow"),
            (
                SourceOwner::Named { name: "Ref".into() },
                vec!["Rows"],
                "named",
            ),
        ] {
            let choice = catalog
                .lookup_choices
                .iter()
                .find(|choice| {
                    choice.source.owner == owner
                        && choice
                            .source
                            .path
                            .iter()
                            .map(String::as_str)
                            .eq(path.iter().copied())
                })
                .unwrap();
            assert_eq!(
                choice
                    .values
                    .iter()
                    .map(|value| value.label.as_str())
                    .collect::<Vec<_>>(),
                [expected]
            );
        }
    }

    #[test]
    fn owned_lookup_catalog_refuses_unbounded_and_document_set_endpoints() {
        let mut source = SchemaNode::scalar("Value", ScalarType::String);
        for _ in 0..66 {
            source = SchemaNode::group("Nested", vec![source]);
        }
        assert!(
            SourcePathCatalog::new(&source, &[])
                .first_owned_lookup(1)
                .is_none()
        );
        let ordinary = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        assert!(
            SourcePathCatalog::new(&ordinary, &[])
                .with_primary_source_options(&mapping::FormatOptions {
                    local_xml_file_set: true,
                    ..Default::default()
                })
                .first_owned_lookup(1)
                .is_none()
        );
    }
}

#[cfg(test)]
mod primary_root_tests {
    use super::*;

    #[test]
    fn primary_catalog_excludes_named_scalar_paths_and_keeps_canonical_namespaces() {
        let source = crate::primary_root_authoring::test_schema();
        let extra = mapping::NamedSource {
            name: "Other".into(),
            path: "other.xml".into(),
            schema: source.clone(),
            options: Default::default(),
            dynamic_path: None,
        };
        let catalog = SourcePathCatalog::new(&source, &[extra]);
        assert_eq!(
            catalog
                .primary_root_fields
                .iter()
                .map(|choice| choice.path.clone())
                .collect::<Vec<_>>(),
            vec![vec!["Code".to_owned()], vec!["Extra".to_owned()]]
        );
        assert_eq!(catalog.primary_root_types, ["Base", "{urn:root}Derived"]);
    }

    #[test]
    fn unsupported_owner_has_no_root_creation_choices() {
        let mut source = crate::primary_root_authoring::test_schema();
        source.repeating = true;
        let catalog = SourcePathCatalog::new(&source, &[]);
        assert!(catalog.first_primary_root_field().is_none());
        assert!(catalog.first_primary_root_type().is_none());
    }
}
