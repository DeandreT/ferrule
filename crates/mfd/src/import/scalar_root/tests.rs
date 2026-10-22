//! Controls for migrating the singleton-root proof onto effective schema views.
//!
//! The pre-migration predicate is retained here as an oracle and compared with
//! the view-derived proof over every role-flag combination and every schema
//! node of every imported fixture design. The proof reads no other schema
//! facts, so agreement on each node implies identical imports.

use std::path::{Path, PathBuf};

use ir::{ScalarType, ScalarTypeSet, SchemaKind, SchemaNode};
use mapping::ScopeConstruction;

use crate::{ImportOptions, ImportProfile};

/// The hand-derived predicate replaced by `EffectiveSchemaView`.
fn legacy_required_singleton(node: &SchemaNode) -> bool {
    !node.repeating
        && !node.xml_optional
        && !node.nillable
        && !node.nullable
        && !node.container_nullable
        && (!node.attribute || node.xml_attribute_required)
}

fn both(node: &SchemaNode) -> (bool, bool) {
    (
        legacy_required_singleton(node),
        super::required_singleton(node),
    )
}

fn shapes() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("Value", ScalarType::String),
        SchemaNode::scalar_union(
            "Value",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        ),
        SchemaNode::group("Value", vec![]),
        SchemaNode::group("Value", vec![SchemaNode::scalar("Leaf", ScalarType::Int)]),
    ]
}

#[test]
fn every_role_flag_combination_matches_or_conservatively_refuses() {
    let mut valid = 0;
    let mut refused_contradictions = 0;
    for shape in shapes() {
        for bits in 0u32..(1 << 8) {
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
                node.json_object_item_nullable = item;
                let (legacy, effective) = both(&node);
                if node.effective_view().is_ok() {
                    valid += 1;
                    assert_eq!(effective, legacy, "{node:?}");
                } else {
                    // Contradictory saved roles are never widened into a proof.
                    assert!(!effective, "{node:?}");
                    refused_contradictions += usize::from(legacy);
                }
            }
        }
    }
    assert!(valid > 100, "{valid}");
    // The legacy predicate ignored misplaced object-item and attribute-use
    // flags; those contradictory saved shapes are refused rather than proved.
    assert!(refused_contradictions > 0);
}

#[test]
fn independently_authored_positive_and_negative_schema_controls() {
    let element = || SchemaNode::scalar("Total", ScalarType::Float);
    let mut required_attribute = SchemaNode::scalar("code", ScalarType::String).attribute();
    assert!(required_attribute.set_xml_attribute_required(true));
    let mut optional_element = element();
    assert!(optional_element.set_xml_optional(true));
    let mut nullable_object = SchemaNode::group("Order", vec![]);
    nullable_object.container_nullable = true;
    let mut object_items = SchemaNode::group("Order", vec![]).repeating();
    object_items.json_object_item_nullable = Some(true);
    let accepted = [
        element(),
        SchemaNode::scalar("Flag", ScalarType::Bool),
        required_attribute,
        SchemaNode::scalar("#text", ScalarType::String).text(),
        SchemaNode::group("Order", vec![element()]),
    ];
    let rejected = [
        optional_element,
        element().nillable(),
        element().repeating(),
        element().nullable().unwrap(),
        SchemaNode::scalar("code", ScalarType::String).attribute(),
        nullable_object,
        object_items,
        SchemaNode::scalar("tags", ScalarType::String)
            .repeating()
            .nullable_container()
            .unwrap(),
    ];
    for node in accepted {
        assert_eq!(both(&node), (true, true), "{node:?}");
    }
    for node in rejected {
        assert_eq!(both(&node), (false, false), "{node:?}");
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

fn walk<'a>(node: &'a SchemaNode, visit: &mut impl FnMut(&'a SchemaNode)) {
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
fn whole_design_imports_match_before_and_after_migration() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths = Vec::new();
    designs(&root, &mut paths);
    assert!(paths.len() > 250, "{}", paths.len());
    let (mut scalar_roots, mut refusals, mut nodes) = (0, 0, 0);
    for path in &paths {
        for profile in [ImportProfile::BestEffort, ImportProfile::Executable] {
            let Ok(outcome) = crate::import_with_profile(path, &ImportOptions::default(), profile)
            else {
                continue;
            };
            let imported = &outcome.imported;
            let project = &imported.project;
            if profile == ImportProfile::BestEffort {
                scalar_roots += std::iter::once(&project.root)
                    .chain(project.extra_targets.iter().map(|target| &target.root))
                    .filter(|root| matches!(root.construction, ScopeConstruction::Scalar { .. }))
                    .count();
                refusals += imported
                    .warnings
                    .iter()
                    .filter(|warning| warning.contains("scalar document root"))
                    .count();
            }
            let mut schemas = vec![&project.source, &project.target];
            schemas.extend(project.extra_sources.iter().map(|source| &source.schema));
            schemas.extend(project.extra_targets.iter().map(|target| &target.schema));
            for schema in schemas {
                walk(schema, &mut |node| {
                    nodes += 1;
                    let (legacy, effective) = both(node);
                    assert_eq!(effective, legacy, "{}: {node:?}", path.display());
                });
            }
        }
    }
    // The corpus exercises both admitted and refused singleton-root proofs.
    assert!(scalar_roots >= 20, "{scalar_roots}");
    assert!(refusals >= 20, "{refusals}");
    assert!(nodes > 2_000, "{nodes}");
}
