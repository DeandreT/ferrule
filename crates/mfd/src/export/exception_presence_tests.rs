//! Controls for migrating the exception-export presence proofs onto effective
//! XML views.
//!
//! The pre-migration predicates are retained as oracles and compared with the
//! view-derived proofs over every role-flag combination and every schema node
//! of every imported fixture design.

use std::path::{Path, PathBuf};

use ir::{EffectivePresence, ScalarType, SchemaKind, SchemaNode};

use super::exception_ordering::{plain_shape, singular_presence};
use super::scoped_exception::required_xml_value;
use crate::{ImportOptions, ImportProfile};

/// The hand-derived scoped-exception predicate replaced by the view.
fn legacy_required_xml_value(node: &SchemaNode) -> bool {
    !node.nillable && !node.xml_optional && (!node.attribute || node.xml_attribute_required)
}

/// The hand-derived ordering-proof presence replaced by the view; its plain
/// shape admits only repetition and optional element occurrence.
fn legacy_singular_presence(node: &SchemaNode) -> Option<EffectivePresence> {
    (!node.repeating).then_some(if node.xml_optional {
        EffectivePresence::Optional
    } else {
        EffectivePresence::Required
    })
}

/// Compares both proofs on one node. Returns whether the node has an
/// interpretable XML occurrence.
fn compare(node: &SchemaNode) -> bool {
    let valid = node.effective_xml_view().is_ok();
    if valid {
        assert_eq!(
            required_xml_value(node),
            legacy_required_xml_value(node),
            "{node:?}"
        );
        if plain_shape(node) {
            assert_eq!(
                singular_presence(node),
                legacy_singular_presence(node),
                "{node:?}"
            );
        }
    } else {
        // Contradictory saved roles are never widened into a proof.
        assert!(!required_xml_value(node), "{node:?}");
        assert_eq!(singular_presence(node), None, "{node:?}");
    }
    valid
}

#[test]
fn every_role_flag_combination_matches_or_conservatively_refuses() {
    let shapes = [
        SchemaNode::scalar("Value", ScalarType::Int),
        SchemaNode::scalar("Value", ScalarType::String),
        SchemaNode::group("Value", vec![]),
        SchemaNode::group("Value", vec![SchemaNode::scalar("Leaf", ScalarType::Int)]),
    ];
    let (mut valid, mut refused_legacy_proofs) = (0, 0);
    for shape in shapes {
        for bits in 0u32..(1 << 8) {
            let mut node = shape.clone();
            node.repeating = bits & 1 != 0;
            node.attribute = bits & 2 != 0;
            node.text = bits & 4 != 0;
            node.nillable = bits & 8 != 0;
            node.xml_optional = bits & 16 != 0;
            node.xml_attribute_required = bits & 32 != 0;
            node.nullable = bits & 64 != 0;
            node.container_nullable = bits & 128 != 0;
            if compare(&node) {
                valid += 1;
            } else {
                refused_legacy_proofs += usize::from(
                    legacy_required_xml_value(&node)
                        || plain_shape(&node) && legacy_singular_presence(&node).is_some(),
                );
            }
        }
    }
    assert!(valid > 200, "{valid}");
    // The legacy predicates ignored misplaced optional-occurrence and
    // attribute-use flags; those saved shapes are refused rather than proved.
    assert!(refused_legacy_proofs > 0);
}

#[test]
fn independently_authored_positive_and_negative_presence_controls() {
    let element = || SchemaNode::scalar("Total", ScalarType::Int);
    let mut required_attribute = SchemaNode::scalar("code", ScalarType::String).attribute();
    assert!(required_attribute.set_xml_attribute_required(true));
    let mut optional_element = element();
    assert!(optional_element.set_xml_optional(true));
    let mut optional_repeating = element().repeating();
    optional_repeating.xml_optional = true;
    let mut required_element_use = element();
    required_element_use.xml_attribute_required = true;
    let present = [
        element(),
        element().repeating(),
        required_attribute,
        SchemaNode::scalar("#text", ScalarType::String).text(),
        // JSON null metadata is not an XML occurrence fact.
        element().nullable().unwrap(),
    ];
    let unproved = [
        optional_element.clone(),
        element().nillable(),
        element().repeating().nillable(),
        SchemaNode::scalar("code", ScalarType::String).attribute(),
        SchemaNode::scalar("code", ScalarType::String)
            .attribute()
            .repeating(),
        optional_repeating.clone(),
        required_element_use,
    ];
    for node in present {
        assert!(required_xml_value(&node), "{node:?}");
        assert!(legacy_required_xml_value(&node), "{node:?}");
    }
    for node in unproved {
        assert!(!required_xml_value(&node), "{node:?}");
    }
    assert_eq!(
        singular_presence(&element()),
        Some(EffectivePresence::Required)
    );
    assert_eq!(
        singular_presence(&optional_element),
        Some(EffectivePresence::Optional)
    );
    assert_eq!(singular_presence(&element().repeating()), None);
    // The plain shape admits this pair of flags, but no proof reads it.
    assert!(plain_shape(&optional_repeating));
    assert_eq!(singular_presence(&optional_repeating), None);
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
                        node.nillable || node.xml_optional || node.xml_attribute_required,
                    );
                    assert!(compare(node), "{}: {node:?}", path.display());
                });
            }
        }
    }
    assert!(nodes > 4_000, "{nodes}");
    assert!(qualified > 100, "{qualified}");
}
