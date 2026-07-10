use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ir::{SchemaKind, SchemaNode};
use mapping::{Graph, Node, NodeId, Project, Scope};

/// One actionable problem found before a mapping is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub location: String,
    pub message: String,
}

impl ValidationIssue {
    fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Checks graph integrity, source/target paths, scope references, builtin
/// names, and cycles without reading input data or evaluating expressions.
pub fn validate(project: &Project) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    validate_graph(project, &mut issues);
    validate_cycles(&project.graph, &mut issues);
    validate_scope(
        project,
        &project.root,
        Some(&project.target),
        &mut Vec::new(),
        &mut issues,
    );
    issues
}

fn validate_graph(project: &Project, issues: &mut Vec<ValidationIssue>) {
    for (&id, node) in &project.graph.nodes {
        let location = format!("graph node {id}");
        for (input, referenced) in node_inputs(node) {
            if !project.graph.nodes.contains_key(&referenced) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!("{input} references missing node {referenced}"),
                ));
            }
        }

        match node {
            Node::SourceField { path } => {
                if !source_path_matches(project, path, |node| {
                    matches!(node.kind, SchemaKind::Scalar { .. })
                }) {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!("source field `{}` matches no scalar", display_path(path)),
                    ));
                }
            }
            Node::Position { collection } if !collection.is_empty() => {
                validate_collection_path(project, &location, collection, "position", issues);
            }
            Node::Call { function, .. } if !functions::is_known(function) => {
                issues.push(ValidationIssue::new(
                    &location,
                    format!("unknown function `{function}`"),
                ));
            }
            Node::Lookup {
                collection,
                key,
                value,
                ..
            } => {
                validate_collection_path(project, &location, collection, "lookup", issues);
                validate_collection_value(
                    project,
                    &location,
                    collection,
                    key,
                    "lookup key",
                    issues,
                );
                validate_collection_value(
                    project,
                    &location,
                    collection,
                    value,
                    "lookup value",
                    issues,
                );
            }
            Node::Aggregate {
                collection,
                value,
                expression,
                ..
            } => {
                validate_collection_path(project, &location, collection, "aggregate", issues);
                if expression.is_none() && !value.is_empty() {
                    validate_collection_value(
                        project,
                        &location,
                        collection,
                        value,
                        "aggregate value",
                        issues,
                    );
                }
            }
            _ => {}
        }
    }
}

fn validate_collection_path(
    project: &Project,
    location: &str,
    collection: &[String],
    label: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if !source_path_matches(project, collection, |_| true) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "{label} collection `{}` matches no source path",
                display_path(collection)
            ),
        ));
    }
}

fn validate_collection_value(
    project: &Project,
    location: &str,
    collection: &[String],
    value: &[String],
    label: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if !source_path_matches(project, collection, |node| {
        follow_schema(node, value)
            .is_some_and(|leaf| matches!(leaf.kind, SchemaKind::Scalar { .. }))
    }) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "{label} `{}` is not a scalar under collection `{}`",
                display_path(value),
                display_path(collection)
            ),
        ));
    }
}

fn node_inputs(node: &Node) -> Vec<(String, NodeId)> {
    match node {
        Node::SourceField { .. } | Node::Position { .. } | Node::Const { .. } => Vec::new(),
        Node::Call { args, .. } => args
            .iter()
            .enumerate()
            .map(|(index, &id)| (format!("argument {index}"), id))
            .collect(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![
            ("condition".into(), *condition),
            ("then branch".into(), *then),
            ("else branch".into(), *else_),
        ],
        Node::ValueMap { input, .. } => vec![("input".into(), *input)],
        Node::Lookup { matches, .. } => vec![("matches".into(), *matches)],
        Node::Aggregate {
            expression, arg, ..
        } => expression
            .iter()
            .map(|&id| ("value expression".to_string(), id))
            .chain(arg.iter().map(|&id| ("argument".to_string(), id)))
            .collect(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Active,
    Done,
}

fn validate_cycles(graph: &Graph, issues: &mut Vec<ValidationIssue>) {
    fn visit(
        id: NodeId,
        graph: &Graph,
        visits: &mut BTreeMap<NodeId, Visit>,
        reported: &mut BTreeSet<NodeId>,
        issues: &mut Vec<ValidationIssue>,
    ) {
        visits.insert(id, Visit::Active);
        if let Some(node) = graph.nodes.get(&id) {
            for (_, referenced) in node_inputs(node) {
                match visits.get(&referenced) {
                    Some(Visit::Active) if reported.insert(referenced) => {
                        issues.push(ValidationIssue::new(
                            format!("graph node {id}"),
                            format!("cycle reaches node {referenced}"),
                        ));
                    }
                    Some(_) => {}
                    None if graph.nodes.contains_key(&referenced) => {
                        visit(referenced, graph, visits, reported, issues);
                    }
                    None => {}
                }
            }
        }
        visits.insert(id, Visit::Done);
    }

    let mut visits = BTreeMap::new();
    let mut reported = BTreeSet::new();
    for &id in graph.nodes.keys() {
        if !visits.contains_key(&id) {
            visit(id, graph, &mut visits, &mut reported, issues);
        }
    }
}

fn validate_scope(
    project: &Project,
    scope: &Scope,
    target: Option<&SchemaNode>,
    path: &mut Vec<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let location = if path.is_empty() {
        "root scope".to_string()
    } else {
        format!("scope `{}`", path.join("/"))
    };

    if let Some(source) = &scope.source
        && !source_path_matches(project, source, |_| true)
    {
        issues.push(ValidationIssue::new(
            &location,
            format!("source path `{}` does not exist", display_path(source)),
        ));
    }
    for (label, node) in [
        ("filter", scope.filter),
        ("group-by key", scope.group_by),
        ("sort key", scope.sort_by),
        ("take count", scope.take),
    ] {
        if let Some(node) = node
            && !project.graph.nodes.contains_key(&node)
        {
            issues.push(ValidationIssue::new(
                &location,
                format!("{label} references missing node {node}"),
            ));
        }
    }
    if scope.source.is_none() && scope.filter.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "filter has no iterated source",
        ));
    }
    if scope.source.is_none() && scope.group_by.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group-by key has no iterated source",
        ));
    }
    if scope.source.is_none() && scope.sort_by.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "sort key has no iterated source",
        ));
    }
    if scope.source.is_none() && scope.take.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "take count has no iterated source",
        ));
    }

    let mut bound_fields = BTreeSet::new();
    for binding in &scope.bindings {
        if !bound_fields.insert(&binding.target_field) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "target field `{}` is bound more than once",
                    binding.target_field
                ),
            ));
        }
        if !project.graph.nodes.contains_key(&binding.node) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "binding for `{}` references missing node {}",
                    binding.target_field, binding.node
                ),
            ));
        }
        if let Some(target) = target {
            match target.child(&binding.target_field) {
                Some(field) if matches!(field.kind, SchemaKind::Scalar { .. }) => {}
                Some(_) => issues.push(ValidationIssue::new(
                    &location,
                    format!("binding target `{}` is not a scalar", binding.target_field),
                )),
                None => issues.push(ValidationIssue::new(
                    &location,
                    format!("binding target `{}` does not exist", binding.target_field),
                )),
            }
        }
    }

    let mut child_fields = BTreeSet::new();
    for child in &scope.children {
        if !child_fields.insert(&child.target_field) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "target child scope `{}` occurs more than once",
                    child.target_field
                ),
            ));
        }
        path.push(child.target_field.clone());
        let child_target = target.and_then(|target| target.child(&child.target_field));
        match child_target {
            Some(node) if matches!(node.kind, SchemaKind::Group { .. }) => {}
            Some(_) => issues.push(ValidationIssue::new(
                format!("scope `{}`", path.join("/")),
                "target scope is not a group",
            )),
            None => issues.push(ValidationIssue::new(
                format!("scope `{}`", path.join("/")),
                "target scope does not exist",
            )),
        }
        validate_scope(project, child, child_target, path, issues);
        path.pop();
    }
}

fn source_path_matches(
    project: &Project,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && follow_schema(&extra.schema, rest).is_some_and(predicate)
    {
        return true;
    }

    any_schema_path(&project.source, path, predicate)
        || project
            .extra_sources
            .iter()
            .any(|source| any_schema_path(&source.schema, path, predicate))
}

/// SourceField paths are relative to the current scope frame, so a valid
/// path may start at any group in the source tree rather than only its root.
fn any_schema_path(
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if follow_schema(schema, path).is_some_and(predicate) {
        return true;
    }
    match &schema.kind {
        SchemaKind::Group { children } => children
            .iter()
            .any(|child| any_schema_path(child, path, predicate)),
        SchemaKind::Scalar { .. } => false,
    }
}

fn follow_schema<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut current = schema;
    for segment in path {
        current = current.child(segment)?;
    }
    Some(current)
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<current>".to_string()
    } else {
        path.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};
    use mapping::{Binding, NamedSource};

    fn valid_project() -> Project {
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["name".into()],
            },
        );
        Project {
            source: SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::String)]),
            target: SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::String)]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                source: Some(Vec::new()),
                bindings: vec![Binding {
                    target_field: "name".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        }
    }

    #[test]
    fn accepts_a_valid_project_and_relative_source_paths() {
        let mut project = valid_project();
        project.extra_sources.push(NamedSource {
            name: "reference".into(),
            path: "reference.json".into(),
            schema: SchemaNode::group(
                "records",
                vec![SchemaNode::scalar("code", ScalarType::String)],
            ),
            options: Default::default(),
        });
        project.graph.nodes.insert(
            1,
            Node::SourceField {
                path: vec!["reference".into(), "code".into()],
            },
        );

        assert!(validate(&project).is_empty());
    }

    #[test]
    fn reports_dangling_references_paths_unknown_functions_and_cycles() {
        let mut project = valid_project();
        project.graph.nodes.insert(
            1,
            Node::Call {
                function: "mystery".into(),
                args: vec![99],
            },
        );
        project.graph.nodes.insert(
            2,
            Node::Call {
                function: "concat".into(),
                args: vec![2],
            },
        );
        project.graph.nodes.insert(
            3,
            Node::SourceField {
                path: vec!["missing".into()],
            },
        );
        project.graph.nodes.insert(
            4,
            Node::Const {
                value: Value::String("unused".into()),
            },
        );
        project.root.source = None;
        project.root.filter = Some(88);
        project.root.group_by = Some(89);
        project.root.sort_by = Some(90);
        project.root.take = Some(91);
        project.root.bindings.push(Binding {
            target_field: "missing".into(),
            node: 77,
        });
        project.root.children.push(Scope {
            target_field: "absent".into(),
            ..Scope::default()
        });

        let rendered: Vec<String> = validate(&project)
            .into_iter()
            .map(|issue| issue.to_string())
            .collect();
        for expected in [
            "unknown function `mystery`",
            "argument 0 references missing node 99",
            "cycle reaches node 2",
            "source field `missing` matches no scalar",
            "filter references missing node 88",
            "group-by key references missing node 89",
            "sort key references missing node 90",
            "take count references missing node 91",
            "filter has no iterated source",
            "sort key has no iterated source",
            "take count has no iterated source",
            "binding target `missing` does not exist",
            "binding for `missing` references missing node 77",
            "target scope does not exist",
        ] {
            assert!(
                rendered.iter().any(|issue| issue.contains(expected)),
                "missing `{expected}` in {rendered:#?}"
            );
        }
    }
}
