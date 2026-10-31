//! The export's port wiring and position-selector walk and the import's
//! item-ordering depth read node inputs through the shared role visitor.
//! These checks compare them with the per-kind matches they replaced, over
//! every node kind, every in-tree `.mfd` import and every in-tree Project.

use std::path::{Path, PathBuf};

use mapping::{Node, NodeId};

use super::*;

#[path = "../../tests/node_inputs_corpus/synthetic.rs"]
mod synthetic;

/// `connect_inputs`' port arguments as they were matched by hand; a skipped
/// node wires nothing.
fn legacy_port_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => [Some(*start), Some(*step), *restart]
            .into_iter()
            .flatten()
            .collect(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*value, *predicate],
        Node::Aggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::JoinAggregate {
            plan,
            expression,
            arg,
            ..
        } => plan
            .computed_source()
            .and_then(|computed| computed.scalar())
            .into_iter()
            .chain(*expression)
            .chain(*arg)
            .collect(),
        Node::SequenceExists { .. } => Vec::new(),
        Node::SequenceItemAt { index, .. } => vec![*index],
        Node::SequenceAggregate { arg, .. } => arg.iter().copied().collect(),
        _ => Vec::new(),
    }
}

/// The position-selector walk's parent, private and replacement inputs as
/// its per-kind match collected them.
fn legacy_selector_inputs(node: &Node) -> SelectorInputs {
    let mut parent = Vec::new();
    let mut private = Vec::new();
    let mut replacements = Vec::new();
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => parent.extend(args),
        Node::Raise { message } => parent.extend(message.iter().copied()),
        Node::RuntimeParameterDefault { default, .. } => parent.push(*default),
        Node::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => parent.extend([Some(*start), Some(*step), *restart].into_iter().flatten()),
        Node::If {
            condition,
            then,
            else_,
        } => parent.extend([*condition, *then, *else_]),
        Node::ValueMap { input, .. } => parent.push(*input),
        Node::Lookup { matches, .. } => parent.push(*matches),
        Node::DynamicSourceField { key, .. } => parent.push(*key),
        Node::XmlMixedContent {
            replacements: rules,
            ..
        } => replacements.extend(
            rules
                .iter()
                .enumerate()
                .map(|(index, replacement)| (index, replacement.expression)),
        ),
        Node::CollectionFind {
            predicate, value, ..
        } => private.extend([*predicate, *value]),
        Node::Aggregate {
            expression, arg, ..
        } => {
            private.extend(*expression);
            parent.extend(*arg);
        }
        Node::JoinAggregate {
            plan,
            expression,
            arg,
            ..
        } => {
            parent.extend(
                plan.computed_source()
                    .and_then(|computed| computed.scalar()),
            );
            private.extend(*expression);
            parent.extend(*arg);
        }
        Node::SequenceExists {
            sequence,
            predicate,
        } => {
            parent.extend(sequence.inputs());
            private.push(*predicate);
        }
        Node::SequenceItemAt { sequence, index } => {
            parent.extend(sequence.inputs());
            parent.push(*index);
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => {
            parent.extend(sequence.inputs());
            parent.extend(*arg);
            private.extend(*predicate);
            private.extend(*expression);
        }
        _ => {}
    }
    SelectorInputs {
        parent,
        private,
        replacements,
    }
}

/// `expression_depth`'s ordering inputs as they were matched by hand.
fn legacy_ordering_inputs(node: &Node) -> Option<Vec<NodeId>> {
    Some(match node {
        Node::Const { .. } | Node::SourceField { .. } | Node::Position { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        _ => return None,
    })
}

fn check(node: &Node, origin: &str) {
    assert_eq!(
        port_inputs(node),
        legacy_port_inputs(node),
        "{origin}: {node:?}"
    );
    assert_eq!(
        selector_inputs(node),
        legacy_selector_inputs(node),
        "{origin}: {node:?}"
    );
    assert_eq!(
        ordering_inputs(node),
        legacy_ordering_inputs(node),
        "{origin}: {node:?}"
    );
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

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn check_project(project: &mapping::Project, origin: &str) -> usize {
    let bodies = project
        .user_functions
        .values()
        .flat_map(|function| function.body.nodes.values());
    let mut nodes = 0;
    for node in project.graph.nodes.values().chain(bodies) {
        check(node, origin);
        nodes += 1;
    }
    nodes
}

#[test]
fn every_node_kind_reads_the_inputs_its_per_kind_match_read() {
    let mut kinds = [false; synthetic::KINDS];
    let mut sequence_kinds = [false; synthetic::SEQUENCE_KINDS];
    for node in synthetic::synthetic_nodes() {
        kinds[synthetic::kind(&node)] = true;
        if let Node::SequenceExists { sequence, .. } = &node {
            sequence_kinds[synthetic::sequence_kind(sequence)] = true;
        }
        check(&node, "synthetic");
    }
    assert!(kinds.iter().all(|covered| *covered), "{kinds:?}");
    assert!(sequence_kinds.iter().all(|covered| *covered));
}

#[test]
fn every_in_tree_design_reads_the_same_inputs() {
    let mut designs = Vec::new();
    collect(&workspace().join("crates"), "mfd", &mut designs);
    designs.sort();
    let mut imported = 0;
    let mut nodes = 0;
    for design in &designs {
        let Ok(import) = crate::import(design) else {
            continue;
        };
        imported += 1;
        nodes += check_project(&import.project, &design.display().to_string());
    }
    assert!(
        imported > 300,
        "imported {imported} of {} designs",
        designs.len()
    );
    let mut files = Vec::new();
    for root in ["conformance", "crates", "examples"] {
        collect(&workspace().join(root), "json", &mut files);
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
            nodes += check_project(&project, &file.display().to_string());
        }
    }
    assert!(nodes > 1500, "checked {nodes} nodes");
}
