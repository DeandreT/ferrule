use std::collections::BTreeMap;
use std::fmt;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::NodeId;

use crate::{Expression, IterationOutput, Program, TargetScope};

/// A malformed backend-neutral program that an emitter must not publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramValidationError {
    DuplicateExpression {
        node: NodeId,
    },
    MissingDependency {
        node: NodeId,
        dependency: NodeId,
    },
    ExpressionCycle {
        cycle: Vec<NodeId>,
    },
    InvalidAggregateCollection {
        node: NodeId,
        collection: Vec<String>,
    },
    InvalidAggregateValuePath {
        node: NodeId,
        collection: Vec<String>,
        value: Vec<String>,
    },
    InvalidSourceIteration {
        target_path: Vec<String>,
        source_path: Vec<String>,
    },
    MissingBindingExpression {
        target_path: Vec<String>,
        target_field: String,
        expression: NodeId,
    },
    MissingFilterExpression {
        target_path: Vec<String>,
        expression: NodeId,
    },
    MissingSortExpression {
        target_path: Vec<String>,
        key: usize,
        expression: NodeId,
    },
    MissingWindowExpression {
        target_path: Vec<String>,
        window: usize,
        bound: usize,
        expression: NodeId,
    },
    InvalidIterationOutput {
        target_path: Vec<String>,
        output: IterationOutput,
    },
    InvalidDuplicateBinding {
        target_path: Vec<String>,
        target_field: String,
        first_binding: usize,
        duplicate_binding: usize,
    },
    DuplicateChildTarget {
        target_path: Vec<String>,
        target_field: String,
        first_child: usize,
        duplicate_child: usize,
    },
    BindingChildCollision {
        target_path: Vec<String>,
        target_field: String,
        binding: usize,
        child: usize,
    },
}

/// Validates invariants relied on by every source-code emitter.
///
/// Programs produced by [`crate::lower`] already satisfy these invariants.
/// This check protects the public programmatic API from emitting recursive or
/// backend-dependent source when callers construct a [`Program`] directly.
pub fn validate_program(program: &Program) -> Result<(), ProgramValidationError> {
    let expressions = collect_expressions(program)?;
    validate_dependencies(&expressions)?;
    validate_cycles(&expressions)?;
    validate_aggregate_paths(&program.source, &expressions)?;
    validate_scope(
        &program.root,
        &expressions,
        &program.source,
        &program.target,
        &mut Vec::new(),
    )
}

fn collect_expressions(
    program: &Program,
) -> Result<BTreeMap<NodeId, &Expression>, ProgramValidationError> {
    let mut expressions = BTreeMap::new();
    for node in &program.expressions {
        if expressions.insert(node.id, &node.expression).is_some() {
            return Err(ProgramValidationError::DuplicateExpression { node: node.id });
        }
    }
    Ok(expressions)
}

fn validate_dependencies(
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        for dependency in dependencies(expression) {
            if !expressions.contains_key(&dependency) {
                return Err(ProgramValidationError::MissingDependency { node, dependency });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Active(usize),
    Complete,
}

fn validate_cycles(
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    let mut visits = BTreeMap::new();
    let mut stack = Vec::new();
    for node in expressions.keys().copied() {
        visit_expression(node, expressions, &mut visits, &mut stack)?;
    }
    Ok(())
}

fn visit_expression(
    node: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    visits: &mut BTreeMap<NodeId, Visit>,
    stack: &mut Vec<NodeId>,
) -> Result<(), ProgramValidationError> {
    match visits.get(&node) {
        Some(Visit::Complete) => return Ok(()),
        Some(Visit::Active(start)) => {
            let mut cycle = stack[*start..].to_vec();
            cycle.push(node);
            return Err(ProgramValidationError::ExpressionCycle { cycle });
        }
        None => {}
    }

    visits.insert(node, Visit::Active(stack.len()));
    stack.push(node);
    if let Some(expression) = expressions.get(&node) {
        for dependency in dependencies(expression) {
            visit_expression(dependency, expressions, visits, stack)?;
        }
    }
    stack.pop();
    visits.insert(node, Visit::Complete);
    Ok(())
}

fn dependencies(expression: &Expression) -> Vec<NodeId> {
    match expression {
        Expression::SourceField { .. } | Expression::Position { .. } | Expression::Const { .. } => {
            Vec::new()
        }
        Expression::Call { args, .. } => args.clone(),
        Expression::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Expression::Aggregate { value, arg, .. } => {
            value.expression().into_iter().chain(*arg).collect()
        }
    }
}

fn validate_aggregate_paths(
    source: &SchemaNode,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let Expression::Aggregate {
            collection, value, ..
        } = expression
        else {
            continue;
        };
        if !schema_path_matches(source, collection, |_| true) {
            return Err(ProgramValidationError::InvalidAggregateCollection {
                node,
                collection: collection.clone(),
            });
        }
        let crate::AggregateValue::Path(value) = value else {
            continue;
        };
        if !value.is_empty()
            && !schema_path_matches(source, collection, |collection| {
                follow_schema_from(source, collection, value)
                    .is_some_and(|leaf| matches!(leaf.kind, SchemaKind::Scalar { .. }))
            })
        {
            return Err(ProgramValidationError::InvalidAggregateValuePath {
                node,
                collection: collection.clone(),
                value: value.clone(),
            });
        }
    }
    Ok(())
}

/// Expression paths are relative to the active source frame, so a valid path
/// can begin at any group in the source schema rather than only at its root.
fn schema_path_matches(
    root: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    fn visit(
        root: &SchemaNode,
        current: &SchemaNode,
        path: &[String],
        predicate: impl Fn(&SchemaNode) -> bool + Copy,
    ) -> bool {
        if follow_schema_from(root, current, path).is_some_and(predicate) {
            return true;
        }
        match &current.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .any(|child| visit(root, child, path, predicate)),
            SchemaKind::Scalar { .. } => false,
        }
    }

    visit(root, root, path, predicate)
}

fn follow_schema_from<'a>(
    root: &'a SchemaNode,
    current: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = current;
    for segment in path {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_schema_group(root, anchor)?;
        }
        current = current.child(segment)?;
    }
    Some(current)
}

fn find_concrete_schema_group<'a>(current: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if current.recursive_ref.is_none()
        && current.name == anchor
        && matches!(current.kind, SchemaKind::Group { .. })
    {
        return Some(current);
    }
    let SchemaKind::Group { children, .. } = &current.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_schema_group(child, anchor))
}

fn validate_scope(
    scope: &TargetScope,
    expressions: &BTreeMap<NodeId, &Expression>,
    source: &SchemaNode,
    target: &SchemaNode,
    target_path: &mut Vec<String>,
) -> Result<(), ProgramValidationError> {
    if let Some(iteration) = &scope.iteration {
        if !schema_path_matches(source, iteration.source_iteration().path(), |_| true) {
            return Err(ProgramValidationError::InvalidSourceIteration {
                target_path: target_path.clone(),
                source_path: iteration.source_iteration().path().to_vec(),
            });
        }
        if let Some(expression) = iteration.filter()
            && !expressions.contains_key(&expression)
        {
            return Err(ProgramValidationError::MissingFilterExpression {
                target_path: target_path.clone(),
                expression,
            });
        }
        if let Some(sort) = iteration.sort() {
            for (key, sort_key) in sort.keys().enumerate() {
                if !expressions.contains_key(&sort_key.expression) {
                    return Err(ProgramValidationError::MissingSortExpression {
                        target_path: target_path.clone(),
                        key,
                        expression: sort_key.expression,
                    });
                }
            }
        }
        for (window, sequence_window) in iteration.windows().iter().copied().enumerate() {
            for (bound, expression) in sequence_window.nodes().enumerate() {
                if !expressions.contains_key(&expression) {
                    return Err(ProgramValidationError::MissingWindowExpression {
                        target_path: target_path.clone(),
                        window,
                        bound,
                        expression,
                    });
                }
            }
        }
        let target_is_nonrepeating_group = follow_schema_from(target, target, target_path)
            .is_some_and(|target| {
                !target.repeating && matches!(target.kind, SchemaKind::Group { .. })
            });
        let invalid_output = match iteration.output() {
            IterationOutput::Repeated => false,
            IterationOutput::First => scope.repeating || !target_is_nonrepeating_group,
            IterationOutput::MappedSequence => {
                scope.repeating || target_path.is_empty() || !target_is_nonrepeating_group
            }
        };
        if invalid_output {
            return Err(ProgramValidationError::InvalidIterationOutput {
                target_path: target_path.clone(),
                output: iteration.output(),
            });
        }
    }

    let mut bindings = BTreeMap::<&str, (usize, bool, ScalarType)>::new();
    for (binding_index, binding) in scope.bindings.iter().enumerate() {
        if !expressions.contains_key(&binding.expression) {
            return Err(ProgramValidationError::MissingBindingExpression {
                target_path: target_path.clone(),
                target_field: binding.target_field.clone(),
                expression: binding.expression,
            });
        }
        if let Some(&(first_binding, repeating, target_type)) =
            bindings.get(binding.target_field.as_str())
        {
            if !repeating || !binding.repeating || target_type != binding.target_type {
                return Err(ProgramValidationError::InvalidDuplicateBinding {
                    target_path: target_path.clone(),
                    target_field: binding.target_field.clone(),
                    first_binding,
                    duplicate_binding: binding_index,
                });
            }
        } else {
            bindings.insert(
                binding.target_field.as_str(),
                (binding_index, binding.repeating, binding.target_type),
            );
        }
    }

    let mut children = BTreeMap::<&str, usize>::new();
    for (child_index, child) in scope.children.iter().enumerate() {
        if let Some(&first_child) = children.get(child.target_field.as_str()) {
            return Err(ProgramValidationError::DuplicateChildTarget {
                target_path: target_path.clone(),
                target_field: child.target_field.clone(),
                first_child,
                duplicate_child: child_index,
            });
        }
        if let Some(&(binding, _, _)) = bindings.get(child.target_field.as_str()) {
            return Err(ProgramValidationError::BindingChildCollision {
                target_path: target_path.clone(),
                target_field: child.target_field.clone(),
                binding,
                child: child_index,
            });
        }
        children.insert(child.target_field.as_str(), child_index);
    }

    for child in &scope.children {
        target_path.push(child.target_field.clone());
        let result = validate_scope(child, expressions, source, target, target_path);
        target_path.pop();
        result?;
    }
    Ok(())
}

impl fmt::Display for ProgramValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateExpression { node } => {
                write!(
                    formatter,
                    "compiled mapping contains duplicate expression {node}"
                )
            }
            Self::MissingDependency { node, dependency } => write!(
                formatter,
                "compiled mapping expression {node} references missing expression {dependency}"
            ),
            Self::ExpressionCycle { cycle } => write!(
                formatter,
                "compiled mapping expressions contain a cycle: {}",
                display_cycle(cycle)
            ),
            Self::InvalidAggregateCollection { node, collection } => write!(
                formatter,
                "compiled mapping aggregate expression {node} collection {} matches no source path",
                display_path(collection)
            ),
            Self::InvalidAggregateValuePath {
                node,
                collection,
                value,
            } => write!(
                formatter,
                "compiled mapping aggregate expression {node} value {} is not a scalar under collection {}",
                display_path(value),
                display_path(collection)
            ),
            Self::InvalidSourceIteration {
                target_path,
                source_path,
            } => write!(
                formatter,
                "target scope {} source iteration {} matches no source path",
                display_path(target_path),
                display_path(source_path)
            ),
            Self::MissingBindingExpression {
                target_path,
                target_field,
                expression,
            } => write!(
                formatter,
                "target scope {} field {target_field:?} references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingFilterExpression {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} filter references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingSortExpression {
                target_path,
                key,
                expression,
            } => write!(
                formatter,
                "target scope {} sort key {} references missing expression {expression}",
                display_path(target_path),
                key + 1
            ),
            Self::MissingWindowExpression {
                target_path,
                window,
                bound,
                expression,
            } => write!(
                formatter,
                "target scope {} sequence window {} bound {} references missing expression {expression}",
                display_path(target_path),
                window + 1,
                bound + 1
            ),
            Self::InvalidIterationOutput {
                target_path,
                output,
            } => write!(
                formatter,
                "target scope {} cannot use {output:?} iteration output with its target cardinality or location",
                display_path(target_path)
            ),
            Self::InvalidDuplicateBinding {
                target_path,
                target_field,
                first_binding,
                duplicate_binding,
            } => write!(
                formatter,
                "target scope {} bindings {first_binding} and {duplicate_binding} conflict for field {target_field:?}",
                display_path(target_path)
            ),
            Self::DuplicateChildTarget {
                target_path,
                target_field,
                first_child,
                duplicate_child,
            } => write!(
                formatter,
                "target scope {} children {first_child} and {duplicate_child} both construct field {target_field:?}",
                display_path(target_path)
            ),
            Self::BindingChildCollision {
                target_path,
                target_field,
                binding,
                child,
            } => write!(
                formatter,
                "target scope {} binding {binding} and child {child} both construct field {target_field:?}",
                display_path(target_path)
            ),
        }
    }
}

impl std::error::Error for ProgramValidationError {}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".into()
    } else {
        format!("`{}`", path.join("/"))
    }
}

fn display_cycle(cycle: &[NodeId]) -> String {
    cycle
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode, Value};

    use super::*;
    use crate::{
        AggregateFunction, AggregateValue, Binding, ExpressionNode, IterationPlan, ScalarFunction,
        SequenceWindow, SortFilterOrder, SortKey, SortPlan, SourceIteration,
    };

    fn program() -> Program {
        Program {
            source: SchemaNode::group(
                "Source",
                vec![SchemaNode::group("Rows", Vec::new()).repeating()],
            ),
            target: SchemaNode::group("Target", Vec::new()),
            expressions: vec![
                ExpressionNode {
                    id: 1,
                    expression: Expression::Const {
                        value: Value::Int(1),
                    },
                },
                ExpressionNode {
                    id: 2,
                    expression: Expression::Call {
                        function: ScalarFunction::Add,
                        args: vec![1, 1],
                    },
                },
            ],
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    expression: 2,
                    target_type: ScalarType::Int,
                    repeating: false,
                }],
                children: Vec::new(),
            },
        }
    }

    #[test]
    fn accepts_valid_repeating_duplicate_bindings() {
        let mut program = program();
        program.root.bindings = vec![
            Binding {
                target_field: "Values".into(),
                expression: 1,
                target_type: ScalarType::Int,
                repeating: true,
            },
            Binding {
                target_field: "Values".into(),
                expression: 2,
                target_type: ScalarType::Int,
                repeating: true,
            },
        ];

        assert_eq!(validate_program(&program), Ok(()));
    }

    #[test]
    fn accepts_empty_and_named_source_iterations() {
        let mut program = program();
        program.root.iteration = Some(IterationPlan::source(Vec::new()));
        assert_eq!(validate_program(&program), Ok(()));

        program.root.iteration = Some(IterationPlan::source(vec!["Rows".into()]));
        assert_eq!(validate_program(&program), Ok(()));

        program.root.iteration = Some(IterationPlan::source(vec!["Missing".into()]));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::InvalidSourceIteration {
                target_path: Vec::new(),
                source_path: vec!["Missing".into()],
            })
        );
    }

    #[test]
    fn accepts_framed_fields_positions_and_source_filters() {
        let mut program = program();
        program.expressions.extend([
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: Some(Vec::new()),
                    path: vec!["Value".into()],
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Position {
                    collection: vec!["Rows".into()],
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Position {
                    collection: Vec::new(),
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::Const {
                    value: Value::Bool(true),
                },
            },
        ]);
        program.root.iteration = Some(IterationPlan::new(
            SourceIteration::new(vec!["Rows".into()]),
            Some(6),
            None,
            Vec::new(),
            IterationOutput::Repeated,
        ));

        assert_eq!(validate_program(&program), Ok(()));
    }

    #[test]
    fn rejects_invalid_filters_at_the_exact_target_path() {
        let child = |filter| TargetScope {
            target_field: "Child".into(),
            repeating: true,
            iteration: Some(IterationPlan::new(
                SourceIteration::new(vec!["Rows".into()]),
                filter,
                None,
                Vec::new(),
                IterationOutput::Repeated,
            )),
            bindings: Vec::new(),
            children: Vec::new(),
        };

        let mut missing = program();
        missing.root.children.push(child(Some(99)));
        assert_eq!(
            validate_program(&missing),
            Err(ProgramValidationError::MissingFilterExpression {
                target_path: vec!["Child".into()],
                expression: 99,
            })
        );
    }

    #[test]
    fn validates_sort_window_and_iteration_output_controls() {
        let child = |iteration, repeating| TargetScope {
            target_field: "Child".into(),
            repeating,
            iteration: Some(iteration),
            bindings: Vec::new(),
            children: Vec::new(),
        };
        let sort = |then| {
            SortPlan::new(
                SortKey {
                    expression: 1,
                    descending: false,
                },
                then,
                SortFilterOrder::SortThenFilter,
            )
        };

        let mut missing_sort = program();
        missing_sort.root.children.push(child(
            IterationPlan::new(
                SourceIteration::new(vec!["Rows".into()]),
                None,
                Some(sort(vec![SortKey {
                    expression: 99,
                    descending: true,
                }])),
                Vec::new(),
                IterationOutput::Repeated,
            ),
            true,
        ));
        assert_eq!(
            validate_program(&missing_sort),
            Err(ProgramValidationError::MissingSortExpression {
                target_path: vec!["Child".into()],
                key: 1,
                expression: 99,
            })
        );

        let mut missing_window = program();
        missing_window.root.children.push(child(
            IterationPlan::new(
                SourceIteration::new(vec!["Rows".into()]),
                None,
                None,
                vec![SequenceWindow::FromTo { first: 1, last: 99 }],
                IterationOutput::Repeated,
            ),
            true,
        ));
        assert_eq!(
            validate_program(&missing_window),
            Err(ProgramValidationError::MissingWindowExpression {
                target_path: vec!["Child".into()],
                window: 0,
                bound: 1,
                expression: 99,
            })
        );

        let mut invalid_first = program();
        invalid_first.root.children.push(child(
            IterationPlan::new(
                SourceIteration::new(vec!["Rows".into()]),
                None,
                None,
                Vec::new(),
                IterationOutput::First,
            ),
            true,
        ));
        assert_eq!(
            validate_program(&invalid_first),
            Err(ProgramValidationError::InvalidIterationOutput {
                target_path: vec!["Child".into()],
                output: IterationOutput::First,
            })
        );

        let mut mapped_root = program();
        mapped_root.root.iteration = Some(IterationPlan::new(
            SourceIteration::new(Vec::new()),
            None,
            None,
            Vec::new(),
            IterationOutput::MappedSequence,
        ));
        assert_eq!(
            validate_program(&mapped_root),
            Err(ProgramValidationError::InvalidIterationOutput {
                target_path: Vec::new(),
                output: IterationOutput::MappedSequence,
            })
        );

        let mut scalar_first = program();
        scalar_first.target = SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Child", ScalarType::String)],
        );
        scalar_first.root.children.push(child(
            IterationPlan::new(
                SourceIteration::new(vec!["Rows".into()]),
                None,
                None,
                Vec::new(),
                IterationOutput::First,
            ),
            false,
        ));
        assert_eq!(
            validate_program(&scalar_first),
            Err(ProgramValidationError::InvalidIterationOutput {
                target_path: vec!["Child".into()],
                output: IterationOutput::First,
            })
        );

        let mut group_first = scalar_first;
        group_first.target =
            SchemaNode::group("Target", vec![SchemaNode::group("Child", Vec::new())]);
        assert_eq!(validate_program(&group_first), Ok(()));
    }

    #[test]
    fn rejects_duplicate_and_missing_expressions() {
        let mut duplicate = program();
        duplicate.expressions.push(duplicate.expressions[0].clone());
        assert_eq!(
            validate_program(&duplicate),
            Err(ProgramValidationError::DuplicateExpression { node: 1 })
        );

        let mut missing = program();
        missing.expressions[1].expression = Expression::Call {
            function: ScalarFunction::Add,
            args: vec![1, 99],
        };
        assert_eq!(
            validate_program(&missing),
            Err(ProgramValidationError::MissingDependency {
                node: 2,
                dependency: 99,
            })
        );
    }

    #[test]
    fn validates_aggregate_projection_and_argument_dependencies() {
        let aggregate = |value, arg| Expression::Aggregate {
            function: AggregateFunction::Sum,
            collection: vec!["Rows".into()],
            value,
            arg,
        };

        let mut missing_projection = program();
        missing_projection.expressions[1].expression =
            aggregate(AggregateValue::Expression(99), Some(98));
        assert_eq!(
            validate_program(&missing_projection),
            Err(ProgramValidationError::MissingDependency {
                node: 2,
                dependency: 99,
            })
        );

        let mut missing_argument = program();
        missing_argument.expressions[1].expression =
            aggregate(AggregateValue::Expression(1), Some(99));
        assert_eq!(
            validate_program(&missing_argument),
            Err(ProgramValidationError::MissingDependency {
                node: 2,
                dependency: 99,
            })
        );

        let mut cycle = program();
        cycle.expressions[1].expression = aggregate(AggregateValue::Expression(2), Some(1));
        assert_eq!(
            validate_program(&cycle),
            Err(ProgramValidationError::ExpressionCycle { cycle: vec![2, 2] })
        );
    }

    #[test]
    fn validates_aggregate_collection_and_direct_value_paths() {
        let mut program = program();
        program.source = SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Amount", ScalarType::Int),
                        SchemaNode::group("Nested", Vec::new()),
                    ],
                )
                .repeating(),
            ],
        );
        let aggregate = |collection: &[&str], value: &[&str]| Expression::Aggregate {
            function: AggregateFunction::Sum,
            collection: collection.iter().map(|segment| (*segment).into()).collect(),
            value: AggregateValue::Path(value.iter().map(|segment| (*segment).into()).collect()),
            arg: None,
        };

        program.expressions[1].expression = aggregate(&["Rows"], &["Amount"]);
        assert_eq!(validate_program(&program), Ok(()));

        program.expressions[1].expression = aggregate(&["Missing"], &["Amount"]);
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::InvalidAggregateCollection {
                node: 2,
                collection: vec!["Missing".into()],
            })
        );

        program.expressions[1].expression = aggregate(&["Rows"], &["Missing"]);
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::InvalidAggregateValuePath {
                node: 2,
                collection: vec!["Rows".into()],
                value: vec!["Missing".into()],
            })
        );

        program.expressions[1].expression = aggregate(&["Rows"], &["Nested"]);
        assert!(matches!(
            validate_program(&program),
            Err(ProgramValidationError::InvalidAggregateValuePath { node: 2, .. })
        ));

        // Empty value paths are valid for count and sum because a scalar
        // collection item is used directly and a structural item becomes Null.
        program.expressions[1].expression = aggregate(&["Rows"], &[]);
        assert_eq!(validate_program(&program), Ok(()));
    }

    #[test]
    fn rejects_self_and_multi_expression_cycles() {
        let mut self_cycle = program();
        self_cycle.expressions[1].expression = Expression::Call {
            function: ScalarFunction::Add,
            args: vec![2, 1],
        };
        assert_eq!(
            validate_program(&self_cycle),
            Err(ProgramValidationError::ExpressionCycle { cycle: vec![2, 2] })
        );

        let mut multi_cycle = program();
        multi_cycle.expressions[0].expression = Expression::If {
            condition: 2,
            then: 2,
            else_: 2,
        };
        assert_eq!(
            validate_program(&multi_cycle),
            Err(ProgramValidationError::ExpressionCycle {
                cycle: vec![1, 2, 1],
            })
        );
    }

    #[test]
    fn rejects_invalid_target_scope_states() {
        let mut missing = program();
        missing.root.bindings[0].expression = 99;
        assert!(matches!(
            validate_program(&missing),
            Err(ProgramValidationError::MissingBindingExpression { expression: 99, .. })
        ));

        let mut duplicate_binding = program();
        duplicate_binding.root.bindings.push(Binding {
            target_field: "Value".into(),
            expression: 1,
            target_type: ScalarType::Int,
            repeating: false,
        });
        assert!(matches!(
            validate_program(&duplicate_binding),
            Err(ProgramValidationError::InvalidDuplicateBinding {
                first_binding: 0,
                duplicate_binding: 1,
                ..
            })
        ));

        let child = TargetScope {
            target_field: "Child".into(),
            repeating: false,
            iteration: None,
            bindings: Vec::new(),
            children: Vec::new(),
        };
        let mut duplicate_child = program();
        duplicate_child.root.children = vec![child.clone(), child.clone()];
        assert!(matches!(
            validate_program(&duplicate_child),
            Err(ProgramValidationError::DuplicateChildTarget {
                first_child: 0,
                duplicate_child: 1,
                ..
            })
        ));

        let mut collision = program();
        collision.root.bindings[0].target_field = "Child".into();
        collision.root.children.push(child);
        assert!(matches!(
            validate_program(&collision),
            Err(ProgramValidationError::BindingChildCollision {
                binding: 0,
                child: 0,
                ..
            })
        ));
    }
}
