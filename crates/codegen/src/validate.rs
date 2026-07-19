use std::collections::BTreeMap;
use std::fmt;

use ir::ScalarType;
use mapping::NodeId;

use crate::{Expression, Program, TargetScope};

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
    MissingBindingExpression {
        target_path: Vec<String>,
        target_field: String,
        expression: NodeId,
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
    validate_scope(&program.root, &expressions, &mut Vec::new())
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
        Expression::SourceField { .. } | Expression::Const { .. } => Vec::new(),
        Expression::Call { args, .. } => args.clone(),
        Expression::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
    }
}

fn validate_scope(
    scope: &TargetScope,
    expressions: &BTreeMap<NodeId, &Expression>,
    target_path: &mut Vec<String>,
) -> Result<(), ProgramValidationError> {
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
        let result = validate_scope(child, expressions, target_path);
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
            Self::MissingBindingExpression {
                target_path,
                target_field,
                expression,
            } => write!(
                formatter,
                "target scope {} field {target_field:?} references missing expression {expression}",
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
    use crate::{Binding, ExpressionNode, ScalarFunction, SourceIteration};

    fn program() -> Program {
        Program {
            source: SchemaNode::group("Source", Vec::new()),
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
        program.root.iteration = Some(SourceIteration::new(Vec::new()));
        assert_eq!(validate_program(&program), Ok(()));

        program.root.iteration = Some(SourceIteration::new(vec!["Rows".into()]));
        assert_eq!(validate_program(&program), Ok(()));
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
