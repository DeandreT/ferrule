//! Controls for migrating import presence and null proofs onto effective
//! views.
//!
//! Each pre-migration predicate is retained as an oracle and compared with its
//! view-derived replacement over every role-flag combination and every schema
//! node of every imported fixture design under both import profiles.

use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaKind, SchemaNode};

use super::{
    json_non_null, json_object_items_non_null, json_property_required, json_values_non_null,
};
use crate::import::group_by_key::ordinary;
use crate::import::json_variable::present_record_step;
use crate::import::lazy_string_substitution::{substitution_ancestor, substitution_leaf};
use crate::import::schema::plain_entry_tree_leaf;
use crate::import::target_iteration::{
    framed_source_step, json_frame_anchor, json_scalar_leaf, ordinary_scalar_domain_node,
    plain_schema, plain_source_group, value_map_domain_node,
};
use crate::{ImportOptions, ImportProfile};

/// The hand-read predicates replaced by the views, one per migrated site.
mod legacy {
    use ir::{ScalarType, SchemaKind, SchemaNode};

    fn group(node: &SchemaNode) -> bool {
        matches!(node.kind, SchemaKind::Group { .. })
    }

    /// `implicit_branches::nullable_object_item_frame` for one node.
    pub(super) fn nullable_object_item_frame(node: &SchemaNode) -> bool {
        node.repeating
            && group(node)
            && !node.json_array_wrapper
            && !node.json_any
            && node.json_single_container_nullable()
    }

    pub(super) fn group_by_key_ordinary(node: &SchemaNode) -> bool {
        !node.nillable
            && !node.nullable
            && !node.container_nullable
            && !node.json_single_container_nullable()
            && !node.json_any
            && !node.json_array_wrapper
            && node.recursive_ref.is_none()
            && node.alternatives().is_empty()
            && node.dynamic_fields().is_none()
            && node.default.is_none()
            && node.value_generation.is_none()
    }

    pub(super) fn required_property(parent: &SchemaNode, name: &str) -> bool {
        parent.required_fields().iter().any(|field| field == name)
    }

    pub(super) fn plain_entry_tree_leaf(node: &SchemaNode) -> bool {
        node.xml_namespace.is_none()
            && !node.repeating
            && node.recursive_ref.is_none()
            && !node.attribute
            && !node.text
            && !node.nillable
            && !node.nullable
            && !node.container_nullable
            && !node.json_any
            && node.fixed.is_none()
            && node.default.is_none()
            && node.value_generation.is_none()
            && matches!(node.alternative_mode, ir::GroupAlternativeMode::Exclusive)
            && matches!(node.xml_alternative_kind, ir::XmlAlternativeKind::XsiType)
            && node.xml_repeating_sequences.is_empty()
            && node.database_relation.is_none()
            && matches!(
                node.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
    }

    pub(super) fn value_map_domain_node(node: &SchemaNode) -> bool {
        !(node.nullable
            || node.container_nullable
            || node.recursive_ref.is_some()
            || !node.alternatives().is_empty()
            || node.dynamic_fields().is_some()
            || node.json_any
            || nullable_object_item_frame(node))
    }

    pub(super) fn ordinary_scalar_domain_node(node: &SchemaNode) -> bool {
        !node.nullable
            && !node.container_nullable
            && node.recursive_ref.is_none()
            && node.alternatives().is_empty()
            && node.dynamic_fields().is_none()
            && !node.json_any
            && !node.json_array_wrapper
            && !nullable_object_item_frame(node)
    }

    /// The parse-date input path step.
    pub(super) fn parse_date_step(node: &SchemaNode) -> bool {
        !(node.nullable || node.container_nullable || nullable_object_item_frame(node))
    }

    pub(super) fn json_scalar_leaf(node: &SchemaNode) -> bool {
        node.is_scalar() && !node.repeating && !node.nullable && !node.container_nullable
    }

    /// The `guaranteed_scalar` step below the required-property check.
    pub(super) fn guaranteed_step(node: &SchemaNode) -> bool {
        !node.nullable && !node.container_nullable
    }

    pub(super) fn json_frame_anchor(node: &SchemaNode) -> bool {
        group(node)
            && !node.container_nullable
            && node.alternatives().is_empty()
            && node.dynamic_fields().is_none()
    }

    pub(super) fn plain_schema(node: &SchemaNode, depth: usize, remaining: &mut usize) -> bool {
        if depth > 64
            || *remaining == 0
            || node.container_nullable
            || node.json_single_container_nullable()
        {
            return false;
        }
        *remaining -= 1;
        let mut expected = match &node.kind {
            SchemaKind::Scalar { ty }
                if matches!(ty, ScalarType::Bool | ScalarType::Int | ScalarType::String)
                    && !node.repeating =>
            {
                SchemaNode::scalar(node.name.clone(), *ty)
            }
            SchemaKind::Group {
                children, required, ..
            } if children
                .iter()
                .all(|child| plain_schema(child, depth + 1, remaining)) =>
            {
                let Some(group) = SchemaNode::group(node.name.clone(), children.clone())
                    .with_required_fields(required.clone())
                else {
                    return false;
                };
                group
            }
            _ => return false,
        };
        expected.repeating = node.repeating;
        if node.repeating && matches!(node.kind, SchemaKind::Group { .. }) {
            expected.json_object_item_nullable = node.json_object_item_nullable;
        }
        node == &expected
    }

    pub(super) fn plain_source_group(node: &SchemaNode) -> bool {
        !node.container_nullable
            && !node.nullable
            && !node.json_single_container_nullable()
            && !node.json_any
            && !node.json_array_wrapper
            && group(node)
    }

    pub(super) fn framed_source_step(node: &SchemaNode) -> bool {
        !node.json_array_wrapper
            && !node.json_any
            && (!group(node)
                || !node.nullable
                    && !node.container_nullable
                    && !node.json_single_container_nullable())
    }

    pub(super) fn present_record_step(node: &SchemaNode) -> bool {
        !node.nullable && !node.container_nullable && node.alternatives().is_empty()
    }

    pub(super) fn substitution_leaf(leaf: &SchemaNode, nullable: bool) -> bool {
        !(leaf.repeating
            || !matches!(
                leaf.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
            || (nullable && !leaf.nullable)
            || leaf.json_any
            || leaf.json_array_wrapper
            || leaf.container_nullable
            || leaf.json_object_item_nullable.is_some()
            || leaf.default.is_some()
            || leaf.value_generation.is_some()
            || leaf.recursive_ref.is_some())
    }

    pub(super) fn substitution_ancestor(node: &SchemaNode) -> bool {
        !(!group(node)
            || node.container_nullable
            || node.json_single_container_nullable()
            || node.json_array_wrapper
            || node.json_any
            || node.recursive_ref.is_some()
            || !node.alternatives().is_empty()
            || node.dynamic_fields().is_some())
    }
}

type Predicate = fn(&SchemaNode) -> bool;

fn plain_schema_current(node: &SchemaNode) -> bool {
    plain_schema(node, 0, &mut 64)
}

fn plain_schema_legacy(node: &SchemaNode) -> bool {
    legacy::plain_schema(node, 0, &mut 64)
}

fn object_items_non_null_legacy(node: &SchemaNode) -> bool {
    !legacy::nullable_object_item_frame(node)
}

/// (site, migrated predicate, retained oracle)
const SITES: [(&str, Predicate, Predicate); 18] = [
    (
        "group_by_key ordinary",
        ordinary,
        legacy::group_by_key_ordinary,
    ),
    (
        "xml_ports plain entry-tree leaf",
        plain_entry_tree_leaf,
        legacy::plain_entry_tree_leaf,
    ),
    (
        "implicit value-map domain",
        value_map_domain_node,
        legacy::value_map_domain_node,
    ),
    (
        "implicit ordinary scalar domain",
        ordinary_scalar_domain_node,
        legacy::ordinary_scalar_domain_node,
    ),
    (
        "implicit parse-date step",
        json_non_null,
        legacy::parse_date_step,
    ),
    (
        "implicit JSON scalar leaf",
        json_scalar_leaf,
        legacy::json_scalar_leaf,
    ),
    (
        "implicit guaranteed step",
        json_values_non_null,
        legacy::guaranteed_step,
    ),
    (
        "implicit JSON frame anchor",
        json_frame_anchor,
        legacy::json_frame_anchor,
    ),
    (
        "implicit object items without null",
        json_object_items_non_null,
        object_items_non_null_legacy,
    ),
    (
        "implicit plain JSON schema",
        plain_schema_current,
        plain_schema_legacy,
    ),
    (
        "sparse plain source group",
        plain_source_group,
        legacy::plain_source_group,
    ),
    (
        "sparse framed source step",
        framed_source_step,
        legacy::framed_source_step,
    ),
    (
        "JSON record presence step",
        present_record_step,
        legacy::present_record_step,
    ),
    (
        "lazy substitution ancestor",
        substitution_ancestor,
        legacy::substitution_ancestor,
    ),
    (
        "lazy substitution non-null leaf",
        |leaf| substitution_leaf(leaf, false),
        |leaf| legacy::substitution_leaf(leaf, false),
    ),
    (
        "lazy substitution nullable leaf",
        |leaf| substitution_leaf(leaf, true),
        |leaf| legacy::substitution_leaf(leaf, true),
    ),
    (
        "required Leaf property",
        |node| json_property_required(node, "Leaf"),
        |node| legacy::required_property(node, "Leaf"),
    ),
    (
        "required item property",
        |node| json_property_required(node, "item"),
        |node| legacy::required_property(node, "item"),
    ),
];

/// Role metadata engine validation rejects on this node.
fn engine_rejected_roles(node: &SchemaNode) -> bool {
    !node.xml_optional_is_valid()
        || !node.xml_attribute_required_is_valid()
        || !node.nullable_is_valid()
        || !node.container_nullable_is_valid()
        || !node.json_object_item_nullable_is_valid()
        || !node.json_any_is_valid()
}

#[derive(Default)]
struct Differences {
    engine_rejected: usize,
    malformed_wrapper_only: usize,
}

/// Compares every site on one node. A valid node must match exactly; a
/// contradictory one may only lose a proof.
fn compare(node: &SchemaNode, differences: &mut Differences) {
    for (site, current, legacy) in SITES {
        let (current, legacy) = (current(node), legacy(node));
        if current == legacy {
            continue;
        }
        assert!(!current, "{site} newly proves {node:?}");
        if engine_rejected_roles(node) {
            differences.engine_rejected += 1;
        } else {
            // Nested-array wrapper frames are validated by saved-project
            // deserialization and every JSON reader and writer.
            assert!(!node.json_array_wrapper_is_valid(), "{site}: {node:?}");
            differences.malformed_wrapper_only += 1;
        }
    }
}

fn shapes() -> Vec<SchemaNode> {
    let leaf = || SchemaNode::scalar("Leaf", ScalarType::Int);
    vec![
        SchemaNode::scalar("Value", ScalarType::Int),
        SchemaNode::scalar("Value", ScalarType::String),
        SchemaNode::group("Value", vec![]),
        SchemaNode::group("Value", vec![leaf()]),
        SchemaNode::group("Value", vec![leaf()])
            .with_required_fields(vec!["Leaf".into()])
            .unwrap(),
        // The exact nested-array wrapper frame.
        SchemaNode::group(
            "Value",
            vec![SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int).repeating()],
        ),
    ]
}

#[test]
fn every_role_flag_combination_matches_or_conservatively_refuses() {
    let mut differences = Differences::default();
    let mut nodes = 0;
    for shape in shapes() {
        for bits in 0u32..(1 << 10) {
            for item in [None, Some(false), Some(true)] {
                let mut node = shape.clone();
                node.repeating = bits & 1 != 0;
                node.attribute = bits & 2 != 0;
                node.text = bits & 4 != 0;
                node.nillable = bits & 8 != 0;
                node.xml_optional = bits & 16 != 0;
                node.xml_attribute_required = bits & 32 != 0;
                node.nullable = bits & 64 != 0;
                node.container_nullable = bits & 128 != 0;
                node.json_array_wrapper = bits & 256 != 0;
                node.json_any = bits & 512 != 0;
                node.json_object_item_nullable = item;
                compare(&node, &mut differences);
                nodes += 1;
            }
        }
    }
    assert_eq!(nodes, 6 * 1024 * 3);
    // The retained readings proved some saved shapes whose roles contradict
    // each other; those are refused rather than proved.
    assert!(differences.engine_rejected > 0);
    assert!(differences.malformed_wrapper_only > 0);
}

#[test]
fn independently_authored_presence_and_null_controls() {
    let text = || SchemaNode::scalar("Name", ScalarType::String);
    let object_array = |container: bool, item: Option<bool>| {
        let mut node = SchemaNode::group("Rows", vec![text()]).repeating();
        node.container_nullable = container;
        node.json_object_item_nullable = item;
        node
    };
    let wrapper = |container: bool| {
        let mut node = SchemaNode::group(
            "Rows",
            vec![SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int).repeating()],
        )
        .repeating()
        .with_json_array_wrapper()
        .unwrap();
        node.container_nullable = container;
        node
    };
    let mut nullable_object = SchemaNode::group("Order", vec![text()]);
    nullable_object.container_nullable = true;

    // Each layer's null stays its own fact.
    assert!(json_values_non_null(&object_array(false, Some(true))));
    assert!(!json_object_items_non_null(&object_array(
        false,
        Some(true)
    )));
    assert!(!json_values_non_null(&object_array(true, Some(false))));
    assert!(json_object_items_non_null(&object_array(true, Some(false))));
    // Omitted item metadata inherits the container's null.
    assert!(!json_object_items_non_null(&object_array(true, None)));
    assert!(json_non_null(&object_array(false, Some(false))));
    assert!(!json_non_null(&nullable_object));
    assert!(!json_non_null(&text().nullable().unwrap()));
    assert!(!json_non_null(&text().repeating().nullable().unwrap()));
    // Nested-array items belong to the wrapper's item child.
    assert!(json_non_null(&wrapper(false)));
    assert!(!json_non_null(&wrapper(true)));
    assert!(!ordinary_scalar_domain_node(&wrapper(false)));
    assert!(value_map_domain_node(&wrapper(false)));
    // XML nil and absence are not JSON null.
    assert!(json_non_null(&text().nillable()));
    assert!(!ordinary(&text().nillable()));
    let mut optional = text();
    assert!(optional.set_xml_optional(true));
    assert!(json_non_null(&optional));
    assert!(ordinary(&optional));
    assert!(plain_entry_tree_leaf(&optional));
    // A nullable substitution needs a nullable leaf; a non-null one takes either.
    assert!(substitution_leaf(&text(), false));
    assert!(!substitution_leaf(&text(), true));
    assert!(substitution_leaf(&text().nullable().unwrap(), true));
    assert!(substitution_leaf(&text().nullable().unwrap(), false));
    // Scalar values keep their own null on a framed source step.
    assert!(framed_source_step(&text().nullable().unwrap()));
    assert!(!framed_source_step(&nullable_object));

    let unproved = [
        // Contradictory roles: scalar null on a group, container null on a
        // singular scalar, and object-item null outside an object array.
        {
            let mut node = SchemaNode::group("Order", vec![text()]);
            node.nullable = true;
            node
        },
        {
            let mut node = text();
            node.container_nullable = true;
            node
        },
        {
            let mut node = SchemaNode::group("Order", vec![text()]);
            node.json_object_item_nullable = Some(false);
            node
        },
    ];
    for node in unproved {
        for (site, current, _) in SITES {
            assert!(
                !current(&node) || site.starts_with("required"),
                "{site}: {node:?}"
            );
        }
    }
}

fn designs(directory: &Path, found: &mut Vec<PathBuf>) {
    let mut entries = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            designs(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "mfd") {
            found.push(path);
        }
    }
}

fn walk(node: &SchemaNode, visit: &mut impl FnMut(&SchemaNode)) {
    visit(node);
    if let SchemaKind::Group {
        children, dynamic, ..
    } = &node.kind
    {
        for child in children {
            walk(child, visit);
        }
        if let Some(dynamic) = dynamic {
            walk(dynamic, visit);
        }
    }
}

#[test]
fn every_imported_fixture_schema_matches_before_and_after_migration() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths = Vec::new();
    designs(&root, &mut paths);
    assert!(paths.len() > 250, "{}", paths.len());
    let (mut nodes, mut qualified) = (0, 0);
    for path in &paths {
        for profile in [ImportProfile::BestEffort, ImportProfile::Executable] {
            let Ok(outcome) = crate::import_with_profile(path, &ImportOptions::default(), profile)
            else {
                continue;
            };
            let project = &outcome.imported.project;
            let mut schemas = vec![&project.source, &project.target];
            schemas.extend(project.extra_sources.iter().map(|source| &source.schema));
            schemas.extend(project.extra_targets.iter().map(|target| &target.schema));
            for schema in schemas {
                walk(schema, &mut |node| {
                    nodes += 1;
                    qualified += usize::from(
                        node.nillable
                            || node.nullable
                            || node.container_nullable
                            || node.json_object_item_nullable.is_some()
                            || node.json_array_wrapper
                            || !node.required_fields().is_empty(),
                    );
                    for (site, current, legacy) in SITES {
                        assert_eq!(
                            current(node),
                            legacy(node),
                            "{site} at {}: {node:?}",
                            path.display()
                        );
                    }
                    for name in node.required_fields() {
                        assert!(json_property_required(node, name));
                    }
                });
            }
        }
    }
    assert!(nodes > 4_000, "{nodes}");
    assert!(qualified > 100, "{qualified}");
}
