//! Interprets a mapping graph against a source record to produce a target
//! record.

use std::collections::HashSet;

use ir::{Record, Value};
use mapping::{Graph, Node, NodeId, Project};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum EngineError {
    #[error("mapping graph has no node with id {0}")]
    MissingNode(NodeId),
    #[error("cycle detected while evaluating node {0}")]
    Cycle(NodeId),
    #[error("source record has no field `{0}`")]
    MissingSourceField(String),
    #[error(transparent)]
    Function(#[from] functions::FunctionError),
}

/// Runs every target field binding in `project` against `source`, producing
/// one target record.
pub fn run(project: &Project, source: &Record) -> Result<Record, EngineError> {
    let mut target = Record::new();
    for binding in &project.bindings {
        let mut in_progress = HashSet::new();
        let value = eval(&project.graph, binding.node, source, &mut in_progress)?;
        target.set(binding.target_field.clone(), value);
    }
    Ok(target)
}

fn eval(
    graph: &Graph,
    node_id: NodeId,
    source: &Record,
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
        Node::SourceField { field } => source
            .get(field)
            .cloned()
            .ok_or_else(|| EngineError::MissingSourceField(field.clone())),
        Node::Const { value } => Ok(value.clone()),
        Node::Call { function, args } => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval(graph, *arg, source, in_progress)?);
            }
            functions::call(function, &values).map_err(EngineError::from)
        }
    };

    in_progress.remove(&node_id);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapping::Binding;

    fn graph_from(nodes: Vec<(NodeId, Node)>) -> Graph {
        Graph {
            nodes: nodes.into_iter().collect(),
        }
    }

    #[test]
    fn evaluates_a_function_call_over_source_fields() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    field: "first".into(),
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
                    field: "last".into(),
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
            source: Default::default(),
            target: Default::default(),
            graph,
            bindings: vec![Binding {
                target_field: "full_name".into(),
                node: 3,
            }],
        };
        let mut source = Record::new();
        source.set("first", Value::String("Jane".into()));
        source.set("last", Value::String("Doe".into()));

        let target = run(&project, &source).unwrap();
        assert_eq!(
            target.get("full_name"),
            Some(&Value::String("Jane Doe".into()))
        );
    }

    #[test]
    fn missing_source_field_is_reported() {
        let graph = graph_from(vec![(
            0,
            Node::SourceField {
                field: "missing".into(),
            },
        )]);
        let project = Project {
            source: Default::default(),
            target: Default::default(),
            graph,
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 0,
            }],
        };
        let err = run(&project, &Record::new()).unwrap_err();
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
            source: Default::default(),
            target: Default::default(),
            graph,
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 0,
            }],
        };
        let err = run(&project, &Record::new()).unwrap_err();
        assert_eq!(err, EngineError::Cycle(0));
    }
}
