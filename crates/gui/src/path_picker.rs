//! Schema-backed source path choices shared by scope and graph editors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use egui::Ui;
use ir::{SchemaKind, SchemaNode};
use mapping::NamedSource;

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

/// Repeating source paths and their per-item scalar paths. Relative paths
/// are included for every schema node that can become an iteration frame.
#[derive(Debug, Clone)]
pub struct SourcePathCatalog {
    root_scope_paths: Vec<PathChoice>,
    scope_paths: Vec<PathChoice>,
    collections: Vec<CollectionChoice>,
}

impl SourcePathCatalog {
    pub fn new(source: &SchemaNode, extras: &[NamedSource]) -> Self {
        let mut entries = BTreeMap::new();
        let mut root_scope_paths = BTreeSet::new();
        let mut scope_paths = BTreeSet::new();
        entries.insert(Vec::new(), CollectionEntry::default());
        root_scope_paths.insert(Vec::new());
        scope_paths.insert(Vec::new());

        add_scope_paths(source, &[], &mut root_scope_paths);
        add_item_context(source, &mut entries, &mut scope_paths);
        add_repeating_contexts(source, &mut entries, &mut scope_paths);

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
        .width(170.0)
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(path, choice.path.clone(), &choice.label);
            }
        });
}

fn collection_label(path: &[String]) -> String {
    if path.is_empty() {
        "<current rows>".to_string()
    } else {
        path.join(" / ")
    }
}

fn value_label(path: &[String]) -> String {
    if path.is_empty() {
        "<item value>".to_string()
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
        SchemaKind::Scalar { .. } => {
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
    use ir::ScalarType;

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
}
