//! `Node::dependencies` and the `.mfd` export's and GUI's per-node input lists
//! read inputs through the shared role-aware node input visitor. These checks
//! compare them with the hand-written lists they replaced over every node
//! kind, the graph and user-function bodies of every in-tree `.mfd` import,
//! and every in-tree Project file.

use std::path::{Path, PathBuf};

use mapping::{Node, NodeId, NodeInputRole, NodeInputSlot};

#[path = "node_inputs_corpus/synthetic.rs"]
mod synthetic;

use synthetic::{KINDS, SEQUENCE_KINDS, kind, sequence_kind, synthetic_nodes};

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

/// The `.mfd` export auto-number pass's input list as it was written by hand.
fn legacy_auto_number(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
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
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
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
            .chain(
                [Some(sequence.item()), *predicate, *expression, *arg]
                    .into_iter()
                    .flatten(),
            )
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
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
        | Node::XmlSerialize { .. } => Vec::new(),
    }
}

/// The `.mfd` export recursive pass's input list as it was written by hand.
fn legacy_recursive(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
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
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
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
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
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
            .chain(
                [Some(sequence.item()), *predicate, *expression, *arg]
                    .into_iter()
                    .flatten(),
            )
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// The `.mfd` export join pass's input list as it was written by hand.
fn legacy_join(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
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
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
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
        | Node::XmlSerialize { .. } => Vec::new(),
    }
}

/// The GUI graph references' input list as it was written by hand.
fn legacy_graph_references(node: &Node) -> Vec<NodeId> {
    match node {
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
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
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedXmlMixedContent(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// The GUI canvas's input list as it was written by hand.
fn legacy_canvas(node: &Node) -> Vec<NodeId> {
    match node {
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
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
        | Node::XmlSerialize { .. } => vec![],
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![*input],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedXmlMixedContent(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// The `.mfd` export dynamic-JSON pass's input list as it was written by hand.
fn legacy_dynamic_json(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
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
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
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
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
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
            .chain(
                [Some(sequence.item()), *predicate, *expression, *arg]
                    .into_iter()
                    .flatten(),
            )
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// The `.mfd` export position pass's input list as it was written by hand.
fn legacy_position(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
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
        // The reducer's predicate has its own generated-item context and is
        // connected explicitly when the filter/exists chain is emitted.
        // Its sequence arguments still execute in the enclosing scope.
        Node::SequenceExists { sequence, .. } => sequence.inputs(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate { sequence, arg, .. } => {
            sequence.inputs().into_iter().chain(*arg).collect()
        }
        Node::Aggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
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
        | Node::XmlSerialize { .. } => Vec::new(),
    }
}

fn check(node: &Node, origin: &str) {
    assert_eq!(
        node.dependencies(),
        legacy_dependencies(node),
        "{origin}: {node:?}"
    );
    let kept_items = node.inputs_where(|input| input.slot != NodeInputSlot::SequenceReservedItem);
    assert_eq!(kept_items, legacy_auto_number(node), "{origin}: {node:?}");
    assert_eq!(kept_items, legacy_recursive(node), "{origin}: {node:?}");
    assert_eq!(kept_items, legacy_dynamic_json(node), "{origin}: {node:?}");
    assert_eq!(
        node.inputs_where(|input| {
            !matches!(
                input.role,
                NodeInputRole::ItemIdentity | NodeInputRole::GeneratedItem
            )
        }),
        legacy_position(node),
        "{origin}: {node:?}"
    );
    let evaluated = node.evaluated_inputs();
    assert_eq!(evaluated, legacy_join(node), "{origin}: {node:?}");
    assert_eq!(
        evaluated,
        legacy_graph_references(node),
        "{origin}: {node:?}"
    );
    assert_eq!(evaluated, legacy_canvas(node), "{origin}: {node:?}");
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

#[test]
fn every_node_kind_reads_the_same_node_inputs() {
    let nodes = synthetic_nodes();
    let mut kinds = [false; KINDS];
    let mut sequence_kinds = [false; SEQUENCE_KINDS];
    for node in &nodes {
        kinds[kind(node)] = true;
        if let Node::SequenceExists { sequence, .. } = node {
            sequence_kinds[sequence_kind(sequence)] = true;
        }
        check(node, "synthetic");
    }
    assert!(kinds.iter().all(|covered| *covered), "{kinds:?}");
    assert!(sequence_kinds.iter().all(|covered| *covered));
}

#[test]
fn every_in_tree_mfd_import_reads_the_same_node_inputs() {
    let mut files = Vec::new();
    collect(&workspace().join("crates"), "mfd", &mut files);
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
            check(node, &file.display().to_string());
            nodes += 1;
        }
    }
    assert!(
        imported > 300,
        "imported {imported} of {} designs",
        files.len()
    );
    assert!(nodes > 1000, "checked {nodes} nodes");
}

#[test]
fn every_in_tree_project_reads_the_same_node_inputs() {
    let mut files = Vec::new();
    for root in ["conformance", "crates", "examples"] {
        collect(&workspace().join(root), "json", &mut files);
    }
    files.sort();
    let mut nodes = 0;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !text.contains("\"graph\"") || !text.contains("\"root\"") {
            continue;
        }
        let Ok(project) = serde_json::from_str::<mapping::Project>(&text) else {
            continue;
        };
        let bodies = project
            .user_functions
            .values()
            .flat_map(|function| function.body.nodes.values());
        for node in project.graph.nodes.values().chain(bodies) {
            check(node, &file.display().to_string());
            nodes += 1;
        }
    }
    assert!(nodes > 500, "checked {nodes} nodes");
}
