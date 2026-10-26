//! The viewer's input pins read the node's evaluated inputs. This compares
//! them with the per-kind lookup they replaced over every node kind, every
//! in-tree `.mfd` import and every in-tree Project.

use std::path::{Path, PathBuf};

use super::*;

#[path = "../../../mfd/tests/node_inputs_corpus/synthetic.rs"]
mod synthetic;

/// `GraphViewer::input_at` as it matched each node kind by hand.
fn legacy_input_at(node: &Node, idx: usize) -> Option<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.get(idx).copied(),
        Node::Raise { message } => (idx == 0).then_some(*message).flatten(),
        Node::RuntimeParameterDefault { default, .. } => (idx == 0).then_some(*default),
        Node::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => [Some(*start), Some(*step), *restart]
            .get(idx)
            .copied()
            .flatten(),
        Node::If {
            condition,
            then,
            else_,
        } => [*condition, *then, *else_].get(idx).copied(),
        Node::ValueMap { input, .. } => (idx == 0).then_some(*input),
        Node::Lookup { matches, .. } => (idx == 0).then_some(*matches),
        Node::OwnedLookup(lookup) => (idx == 0).then_some(lookup.matches),
        Node::OwnedJoinField(_) | Node::OwnedJoinSourcePosition(_) => None,
        Node::OwnedCollectionFind(find) => [find.predicate, find.value].get(idx).copied(),
        Node::OwnedAggregate(aggregate) => aggregate.inputs().get(idx).copied(),
        Node::OwnedJoinAggregate(aggregate) => aggregate.inputs().get(idx).copied(),
        Node::DynamicSourceField { key, .. } => (idx == 0).then_some(*key),
        Node::OwnedDynamicSourceField(field) => (idx == 0).then_some(field.key),
        Node::XmlMixedContent { replacements, .. } => replacements
            .get(idx)
            .map(|replacement| replacement.expression),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .get(idx)
            .map(|replacement| replacement.expression),
        Node::CollectionFind {
            predicate, value, ..
        } => [*predicate, *value].get(idx).copied(),
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .get(idx)
            .copied()
            .or_else(|| (idx == sequence.inputs().len()).then_some(*predicate)),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .get(idx)
            .copied()
            .or_else(|| (idx == sequence.inputs().len()).then_some(*index)),
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence.inputs().get(idx).copied().or_else(|| {
            predicate
                .iter()
                .chain(expression)
                .chain(arg)
                .nth(idx.saturating_sub(sequence.inputs().len()))
                .copied()
        }),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).nth(idx).copied(),
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedSourceDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => None,
    }
}

const PROBE: NodeId = 1_000_000;

/// Every pin index the node has, and two past it.
fn check(fx: &mut Fixture, node: &Node, origin: &str) {
    fx.graph.nodes.insert(PROBE, node.clone());
    let viewer = fx.viewer();
    for idx in 0..node.dependencies().len() + 2 {
        assert_eq!(
            viewer.input_at(PROBE, idx),
            legacy_input_at(node, idx),
            "{origin}: pin {idx} of {node:?}"
        );
    }
}

fn collect(directory: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let skipped = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| ["target", "node_modules", ".git", "bin", "obj"].contains(&name));
        if kind.is_dir() && !skipped {
            collect(&path, extension, files);
        } else if kind.is_file() && path.extension().is_some_and(|found| found == extension) {
            files.push(path);
        }
    }
}

fn project_nodes(project: &mapping::Project) -> Vec<Node> {
    project
        .graph
        .nodes
        .values()
        .chain(
            project
                .user_functions
                .values()
                .flat_map(|function| function.body.nodes.values()),
        )
        .cloned()
        .collect()
}

#[test]
fn every_node_kind_pins_the_inputs_its_per_kind_lookup_pinned() {
    let mut fx = fixture();
    let mut kinds = [false; synthetic::KINDS];
    let mut sequence_kinds = [false; synthetic::SEQUENCE_KINDS];
    for node in synthetic::synthetic_nodes() {
        kinds[synthetic::kind(&node)] = true;
        if let Node::SequenceExists { sequence, .. } = &node {
            sequence_kinds[synthetic::sequence_kind(sequence)] = true;
        }
        check(&mut fx, &node, "synthetic");
    }
    assert!(kinds.iter().all(|covered| *covered), "{kinds:?}");
    assert!(sequence_kinds.iter().all(|covered| *covered));
}

#[test]
fn every_in_tree_design_pins_the_same_inputs() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut fx = fixture();
    let mut designs = Vec::new();
    collect(&workspace.join("crates"), "mfd", &mut designs);
    designs.sort();
    let mut imported = 0;
    let mut nodes = 0;
    for design in &designs {
        let Ok(import) = mfd::import(design) else {
            continue;
        };
        imported += 1;
        for node in project_nodes(&import.project) {
            check(&mut fx, &node, &design.display().to_string());
            nodes += 1;
        }
    }
    assert!(
        imported > 300,
        "imported {imported} of {} designs",
        designs.len()
    );
    let mut files = Vec::new();
    for root in ["conformance", "crates", "examples"] {
        collect(&workspace.join(root), "json", &mut files);
    }
    files.sort();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !text.contains("\"graph\"") || !text.contains("\"root\"") {
            continue;
        }
        if let Ok(project) = serde_json::from_str::<mapping::Project>(&text) {
            for node in project_nodes(&project) {
                check(&mut fx, &node, &file.display().to_string());
                nodes += 1;
            }
        }
    }
    assert!(nodes > 1500, "checked {nodes} nodes");
}
