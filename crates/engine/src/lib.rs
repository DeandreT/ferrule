//! Interprets a mapping graph against a source instance to produce a target
//! instance.

use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{Graph, Node, NodeId, Project, Scope};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum EngineError {
    #[error("mapping graph has no node with id {0}")]
    MissingNode(NodeId),
    #[error("cycle detected while evaluating node {0}")]
    Cycle(NodeId),
    #[error("no source field found at path `{0}`")]
    MissingSourceField(String),
    #[error(transparent)]
    Function(#[from] functions::FunctionError),
}

/// Runs `project`'s scope tree against `source`, producing one target
/// instance.
pub fn run(project: &Project, source: &Instance) -> Result<Instance, EngineError> {
    eval_scope(&project.graph, &project.root, &[source])
}

fn eval_scope(
    graph: &Graph,
    scope: &Scope,
    context: &[&Instance],
) -> Result<Instance, EngineError> {
    let extensions: Vec<Vec<&Instance>> = match &scope.source {
        None => vec![vec![*context.last().expect("context is never empty")]],
        Some(path) => walk(context.last().expect("context is never empty"), path, &[]),
    };

    let mut produced = Vec::with_capacity(extensions.len());
    for extension in &extensions {
        let mut next_context = context.to_vec();
        next_context.extend(extension.iter().copied());

        let mut fields = Vec::with_capacity(scope.bindings.len() + scope.children.len());
        for binding in &scope.bindings {
            let mut in_progress = HashSet::new();
            let value = eval_expr(graph, binding.node, &next_context, &mut in_progress)?;
            fields.push((binding.target_field.clone(), Instance::Scalar(value)));
        }
        for child in &scope.children {
            let child_instance = eval_scope(graph, child, &next_context)?;
            fields.push((child.target_field.clone(), child_instance));
        }
        produced.push(Instance::Group(fields));
    }

    if scope.source.is_some() {
        Ok(Instance::Repeated(produced))
    } else {
        Ok(produced
            .into_iter()
            .next()
            .expect("a scope without `source` always yields exactly one item"))
    }
}

/// Walks `path` from `base`, branching (and pushing one context frame) each
/// time it crosses a repeating element -- whether mid-path or, if `path` is
/// exhausted and the final value is itself repeating (e.g. `path` is empty
/// and `base` is a CSV file's rows), at the very end. Returns one extension
/// (the new frames to push, innermost last) per produced item.
fn walk<'a>(base: &'a Instance, path: &[String], acc: &[&'a Instance]) -> Vec<Vec<&'a Instance>> {
    match path.split_first() {
        None => match base {
            Instance::Repeated(items) => items
                .iter()
                .map(|item| {
                    let mut next = acc.to_vec();
                    next.push(item);
                    next
                })
                .collect(),
            _ => {
                let mut next = acc.to_vec();
                next.push(base);
                vec![next]
            }
        },
        Some((segment, rest)) => match base.field(segment) {
            None => Vec::new(),
            Some(Instance::Repeated(items)) => items
                .iter()
                .flat_map(|item| {
                    let mut next_acc = acc.to_vec();
                    next_acc.push(item);
                    if rest.is_empty() {
                        vec![next_acc]
                    } else {
                        walk(item, rest, &next_acc)
                    }
                })
                .collect(),
            Some(other) => walk(other, rest, acc),
        },
    }
}

fn eval_expr(
    graph: &Graph,
    node_id: NodeId,
    context: &[&Instance],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    if !in_progress.insert(node_id) {
        return Err(EngineError::Cycle(node_id));
    }

    let node = graph
        .nodes
        .get(&node_id)
        .ok_or(EngineError::MissingNode(node_id))?;

    let result = match node {
        Node::SourceField { path } => resolve_scalar(context, path)
            .ok_or_else(|| EngineError::MissingSourceField(path.join("/"))),
        Node::Const { value } => Ok(value.clone()),
        Node::Call { function, args } => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_expr(graph, *arg, context, in_progress)?);
            }
            functions::call(function, &values).map_err(EngineError::from)
        }
    };

    in_progress.remove(&node_id);
    result
}

/// Resolves `path` against the innermost context item, falling back to
/// enclosing items if not found there (nearest enclosing wins).
fn resolve_scalar(context: &[&Instance], path: &[String]) -> Option<Value> {
    for item in context.iter().rev() {
        let mut current = *item;
        let mut found = true;
        for segment in path {
            match current.field(segment) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found && let Some(value) = current.as_scalar() {
            return Some(value.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::SchemaNode;
    use mapping::Binding;

    fn graph_from(nodes: Vec<(NodeId, Node)>) -> Graph {
        Graph {
            nodes: nodes.into_iter().collect(),
        }
    }

    fn dummy_schema() -> SchemaNode {
        SchemaNode::group("root", vec![])
    }

    #[test]
    fn evaluates_a_function_call_over_source_fields() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    path: vec!["first".into()],
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String(" ".into()),
                },
            ),
            (
                2,
                Node::SourceField {
                    path: vec!["last".into()],
                },
            ),
            (
                3,
                Node::Call {
                    function: "concat".into(),
                    args: vec![0, 1, 2],
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                bindings: vec![Binding {
                    target_field: "full_name".into(),
                    node: 3,
                }],
                children: vec![],
            },
        };
        let source = Instance::Group(vec![
            (
                "first".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            ("last".into(), Instance::Scalar(Value::String("Doe".into()))),
        ]);

        let target = run(&project, &source).unwrap();
        assert_eq!(
            target.field("full_name").and_then(Instance::as_scalar),
            Some(&Value::String("Jane Doe".into()))
        );
    }

    #[test]
    fn missing_source_field_is_reported() {
        let graph = graph_from(vec![(
            0,
            Node::SourceField {
                path: vec!["missing".into()],
            },
        )]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 0,
                }],
                children: vec![],
            },
        };
        let err = run(&project, &Instance::Group(vec![])).unwrap_err();
        assert_eq!(err, EngineError::MissingSourceField("missing".to_string()));
    }

    #[test]
    fn self_referential_node_is_a_cycle() {
        let graph = graph_from(vec![(
            0,
            Node::Call {
                function: "concat".into(),
                args: vec![0],
            },
        )]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 0,
                }],
                children: vec![],
            },
        };
        let err = run(&project, &Instance::Group(vec![])).unwrap_err();
        assert_eq!(err, EngineError::Cycle(0));
    }

    /// The "hard part" this milestone is about: a nested repeating source
    /// (Order -> Item) flattened into a single repeating target level, with
    /// an Order-level field ("cust") broadcast into every produced row --
    /// this is the shape of a real-world nested join.
    #[test]
    fn nested_repetition_flattens_with_broadcast_from_enclosing_scope() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    path: vec!["cust".into()],
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["item_id".into()],
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: Some(vec!["orders".into(), "items".into()]),
                bindings: vec![
                    Binding {
                        target_field: "cust".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "item_id".into(),
                        node: 1,
                    },
                ],
                children: vec![],
            },
        };

        let item = |id: &str| {
            Instance::Group(vec![(
                "item_id".into(),
                Instance::Scalar(Value::String(id.into())),
            )])
        };
        let order = |cust: &str, items: Vec<Instance>| {
            Instance::Group(vec![
                ("cust".into(), Instance::Scalar(Value::String(cust.into()))),
                ("items".into(), Instance::Repeated(items)),
            ])
        };
        let source = Instance::Group(vec![(
            "orders".into(),
            Instance::Repeated(vec![
                order("Jane", vec![item("A"), item("B")]),
                order("John", vec![item("C")]),
            ]),
        )]);

        let target = run(&project, &source).unwrap();
        let rows = target.as_repeated().unwrap();
        assert_eq!(rows.len(), 3);

        let row = |i: usize| &rows[i];
        let cust = |i: usize| row(i).field("cust").and_then(Instance::as_scalar).cloned();
        let item_id = |i: usize| {
            row(i)
                .field("item_id")
                .and_then(Instance::as_scalar)
                .cloned()
        };

        assert_eq!(cust(0), Some(Value::String("Jane".into())));
        assert_eq!(item_id(0), Some(Value::String("A".into())));
        assert_eq!(cust(1), Some(Value::String("Jane".into())));
        assert_eq!(item_id(1), Some(Value::String("B".into())));
        assert_eq!(cust(2), Some(Value::String("John".into())));
        assert_eq!(item_id(2), Some(Value::String("C".into())));
    }
}
