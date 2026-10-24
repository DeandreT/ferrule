//! The ordinary scalar input slots shared by headless and graphical editors.
use mapping::{Node, NodeId};

/// Change an ordinary scalar node's input in canvas order. Specialized
/// sequence, joined, dynamic and XML nodes retain their own authoring rules.
pub fn set_scalar_input(node: &mut Node, index: usize, source: NodeId) -> bool {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => {
            let Some(slot) = args.get_mut(index) else {
                return false;
            };
            *slot = source;
        }
        Node::Raise { message } if index == 0 => *message = Some(source),
        Node::RuntimeParameterDefault { default, .. } if index == 0 => *default = source,
        Node::If {
            condition,
            then,
            else_,
        } => match index {
            0 => *condition = source,
            1 => *then = source,
            2 => *else_ = source,
            _ => return false,
        },
        Node::ValueMap { input, .. } if index == 0 => *input = source,
        Node::Lookup { matches, .. } if index == 0 => *matches = source,
        Node::CollectionFind {
            predicate, value, ..
        } => match index {
            0 => *predicate = source,
            1 => *value = source,
            _ => return false,
        },
        Node::Aggregate {
            expression, arg, ..
        } => {
            if expression.is_some() && index == 0 {
                *expression = Some(source);
            } else if arg.is_some() && index == usize::from(expression.is_some()) {
                *arg = Some(source);
            } else {
                return false;
            }
        }
        _ => return false,
    }
    true
}

/// Candidates for hidden-input cleanup when no owner or visible node retains
/// them. Hosts still check source-pin availability. Constants and calls remain
/// visible nodes, even when disconnected.
pub fn is_hidden_input(node: &Node) -> bool {
    matches!(node, Node::Unconnected | Node::SourceField { .. })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_slots_follow_the_models_evaluated_input_order() {
        let mut nodes = vec![
            Node::Call {
                function: "concat".into(),
                args: vec![1, 2],
            },
            Node::UserFunctionCall {
                function: mapping::FunctionId::new(u64::MAX),
                args: vec![1, 2],
            },
            Node::Raise { message: Some(1) },
            Node::RuntimeParameterDefault {
                name: "value".into(),
                ty: ir::ScalarType::String,
                default: 1,
                preview: None,
            },
            Node::If {
                condition: 1,
                then: 2,
                else_: 3,
            },
            Node::ValueMap {
                input: 1,
                input_type: None,
                table: vec![],
                default: None,
            },
            Node::Lookup {
                collection: vec![],
                key: vec![],
                matches: 1,
                value: vec![],
            },
            Node::CollectionFind {
                collection: vec![],
                predicate: 1,
                value: 2,
            },
        ];
        for (expression, arg) in [(Some(1), Some(2)), (Some(1), None), (None, Some(2))] {
            nodes.push(Node::Aggregate {
                function: mapping::AggregateOp::Count,
                collection: vec![],
                value: vec![],
                expression,
                arg,
            });
        }
        for original in nodes {
            let inputs = original.evaluated_inputs();
            for (index, _) in inputs.iter().enumerate() {
                let mut edited = original.clone();
                let mut expected = inputs.clone();
                expected[index] = 99;
                assert!(set_scalar_input(&mut edited, index, 99));
                assert_eq!(edited.evaluated_inputs(), expected);
            }
            let mut unchanged = original.clone();
            assert!(!set_scalar_input(&mut unchanged, inputs.len(), 99));
            assert_eq!(format!("{original:?}"), format!("{unchanged:?}"));
        }
        let mut optional = Node::Raise { message: None };
        assert!(set_scalar_input(&mut optional, 0, 99));
        assert_eq!(optional.evaluated_inputs(), [99]);
    }
}
