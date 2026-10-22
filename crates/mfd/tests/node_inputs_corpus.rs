//! `Node::dependencies` reads every input through the shared role-aware node
//! input visitor. This compares it with the hand-written list it replaced over
//! the graph and user-function bodies of every in-tree `.mfd` import.

use std::path::{Path, PathBuf};

use mapping::{Node, NodeId};

/// `Node::dependencies` as it was written by hand.
fn legacy_dependencies(node: &Node) -> Vec<NodeId> {
    match node {
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::SourceField { .. }
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::Raise { message } => message.iter().copied().collect(),
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(lookup) => vec![lookup.matches],
        Node::OwnedCollectionFind(find) => vec![find.predicate, find.value],
        Node::OwnedAggregate(aggregate) => aggregate.inputs(),
        Node::OwnedJoinAggregate(aggregate) => aggregate.inputs(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain(sequence.owned_items())
            .chain([*predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain(sequence.owned_items())
            .chain([*index])
            .collect(),
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(sequence.owned_items())
            .chain([*predicate, *expression, *arg].into_iter().flatten())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && path.file_name().is_none_or(|name| name != "target") {
            collect(&path, files);
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "mfd") {
            files.push(path);
        }
    }
}

#[test]
fn every_in_tree_mfd_import_reads_the_same_node_inputs() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    collect(&crates, &mut files);
    files.sort();
    let mut imported = 0;
    let mut nodes = 0;
    for file in &files {
        let Ok(import) = mfd::import(file) else {
            continue;
        };
        imported += 1;
        let project = import.project;
        let bodies = project
            .user_functions
            .values()
            .flat_map(|function| function.body.nodes.values());
        for node in project.graph.nodes.values().chain(bodies) {
            assert_eq!(
                node.dependencies(),
                legacy_dependencies(node),
                "{}: {node:?}",
                file.display()
            );
            nodes += 1;
        }
    }
    assert!(
        imported > 200,
        "imported {imported} of {} designs",
        files.len()
    );
    assert!(nodes > 1000, "checked {nodes} nodes");
}
