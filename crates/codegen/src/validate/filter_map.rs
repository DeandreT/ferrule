//! Defensive admission for directly constructed common filter/map Programs.
//! This validates structure and permissions; it evaluates no expressions.
use std::collections::{BTreeMap, BTreeSet};

use ir::ScalarType;
use mapping::{FilterMapAdmissionKind as Kind, FilterMapStage, FunctionId, NodeId};

use super::{ProgramValidationError, graph_dependencies};
use crate::{
    Expression, FailureIteration, GeneratedSequence, Program, ScalarFunction, UserFunctionProgram,
};

pub(super) fn validate(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
    private_items: &BTreeSet<NodeId>,
) -> Result<(), ProgramValidationError> {
    let compositions = program.filter_map_v1_sequences();
    if compositions.is_empty() {
        return Ok(());
    }
    for rule in &program.failure_rules {
        if let FailureIteration::Generated(GeneratedSequence::FilterMapV1(composition)) =
            &rule.iteration
        {
            return Err(ProgramValidationError::FilterMapAdmission {
                item: composition.item,
                kind: Kind::UnsupportedConsumer {
                    site: "failure rule",
                },
            });
        }
    }
    let functions: BTreeMap<_, _> = program
        .user_functions
        .iter()
        .map(|function| (function.id, function))
        .collect();
    for composition in compositions {
        let error = |kind| ProgramValidationError::FilterMapAdmission {
            item: composition.item,
            kind,
        };
        for node in composition
            .source
            .from
            .iter()
            .copied()
            .chain([composition.source.to])
        {
            validate_parent(expressions, node, "source", |item| {
                (item == composition.source.item || item == composition.item)
                    .then_some(Kind::PrivateSourceArgument { node, item })
            })
            .map_err(error)?;
        }
        if composition.captures.len() > mapping::MAX_FILTER_MAP_CAPTURES {
            return Err(error(Kind::CaptureCount {
                found: composition.captures.len(),
                max: mapping::MAX_FILTER_MAP_CAPTURES,
            }));
        }
        for (capture, value) in composition.captures.iter().enumerate() {
            validate_parent(expressions, value.node, "capture", |item| {
                private_items
                    .contains(&item)
                    .then_some(Kind::PrivateCapture {
                        capture,
                        node: value.node,
                        item,
                    })
            })
            .map_err(error)?;
        }
        let parameters: Vec<_> = [ScalarType::Int, ScalarType::Int]
            .into_iter()
            .chain(composition.captures.iter().map(|capture| capture.ty))
            .collect();
        for (stage, function, output) in [
            (
                FilterMapStage::Predicate,
                composition.predicate,
                ScalarType::Bool,
            ),
            (
                FilterMapStage::Mapper,
                composition.mapper,
                composition.output_type,
            ),
        ] {
            let definition = functions
                .get(&function)
                .ok_or_else(|| error(Kind::MissingFunction { function }))?;
            let found: Vec<_> = definition
                .parameters
                .iter()
                .map(|parameter| parameter.ty)
                .collect();
            if found != parameters || definition.output_type != output {
                return Err(error(Kind::StageSignature {
                    stage,
                    function,
                    expected_parameters: parameters.clone(),
                    found_parameters: found,
                    expected_output: output,
                    found_output: definition.output_type,
                }));
            }
        }
        let mut memo = BTreeMap::new();
        validate_stage(
            composition.predicate,
            &functions,
            &mut Vec::new(),
            &mut memo,
        )
        .map_err(error)?;
        validate_stage(composition.mapper, &functions, &mut Vec::new(), &mut memo)
            .map_err(error)?;
    }
    Ok(())
}

fn validate_parent(
    expressions: &BTreeMap<NodeId, &Expression>,
    root: NodeId,
    role: &'static str,
    private: impl Fn(NodeId) -> Option<Kind>,
) -> Result<(), Kind> {
    let mut visited = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if let Some(error) = private(node) {
            return Err(error);
        }
        if !visited.insert(node) {
            continue;
        }
        let expression = expressions
            .get(&node)
            .ok_or(Kind::MissingGraphNode { role, node })?;
        let mut inputs = graph_dependencies::of(expression);
        if let Expression::SequenceExists { sequence, .. }
        | Expression::SequenceItemAt { sequence, .. }
        | Expression::SequenceAggregate { sequence, .. } = expression
        {
            // These identities are dependencies of the mapping descriptor even
            // when a reducer does not read an item value. Captures cannot hide
            // an owned generated context behind a scalar reducer result.
            inputs.extend(sequence.owned_items());
        }
        pending.extend(inputs.into_iter().rev());
    }
    Ok(())
}

fn validate_stage(
    id: FunctionId,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    stack: &mut Vec<FunctionId>,
    memo: &mut BTreeMap<FunctionId, Vec<FunctionId>>,
) -> Result<Vec<FunctionId>, Kind> {
    if stack.contains(&id) {
        return Err(Kind::FunctionCycle {
            path: stack.iter().copied().chain([id]).collect(),
        });
    }
    if let Some(longest) = memo.get(&id) {
        if stack.len() + longest.len() > mapping::MAX_FILTER_MAP_FUNCTION_DEPTH {
            return Err(Kind::FunctionDepth {
                path: stack
                    .iter()
                    .copied()
                    .chain(longest.iter().copied())
                    .collect(),
                limit: mapping::MAX_FILTER_MAP_FUNCTION_DEPTH,
            });
        }
        return Ok(longest.clone());
    }
    if stack.len() >= mapping::MAX_FILTER_MAP_FUNCTION_DEPTH {
        return Err(Kind::FunctionDepth {
            path: stack.iter().copied().chain([id]).collect(),
            limit: mapping::MAX_FILTER_MAP_FUNCTION_DEPTH,
        });
    }
    let definition = functions
        .get(&id)
        .ok_or(Kind::MissingFunction { function: id })?;
    if definition.parameters.len() > mapping::MAX_FILTER_MAP_FUNCTION_PARAMETERS {
        return Err(Kind::FunctionParameterCount {
            function: id,
            found: definition.parameters.len(),
            max: mapping::MAX_FILTER_MAP_FUNCTION_PARAMETERS,
        });
    }
    // Shared UDF validation has already checked ordered parameter identities,
    // dependencies, outputs, arity and graph/call cycles. Stage admission adds
    // only the restricted transitive scalar capability and its finite limits.
    stack.push(id);
    let mut longest = vec![id];
    let expressions: BTreeMap<_, _> = definition
        .expressions
        .iter()
        .map(|node| (node.id, &node.expression))
        .collect();
    for (&node, expression) in &expressions {
        match expression {
            Expression::Const { .. }
            | Expression::FunctionParameter { .. }
            | Expression::If { .. }
            | Expression::Raise { .. } => {}
            Expression::Call { function, args } => {
                if !matches!(
                    function,
                    ScalarFunction::Add
                        | ScalarFunction::Multiply
                        | ScalarFunction::Divide
                        | ScalarFunction::Equal
                        | ScalarFunction::NotEqual
                        | ScalarFunction::LessThan
                        | ScalarFunction::GreaterThan
                        | ScalarFunction::Concat
                ) {
                    return Err(Kind::UnsupportedBuiltin {
                        function: id,
                        node,
                        builtin: function.as_str().into(),
                    });
                }
                if args.len() != 2 {
                    return Err(Kind::BuiltinArity {
                        function: id,
                        node,
                        found: args.len(),
                        expected: 2,
                    });
                }
            }
            Expression::UserFunctionCall {
                function: callee, ..
            } => {
                let child = validate_stage(*callee, functions, stack, memo)?;
                if child.len() + 1 > longest.len() {
                    longest = [vec![id], child].concat();
                }
            }
            other => {
                return Err(Kind::UnsupportedStageNode {
                    function: id,
                    node,
                    kind: unsupported_kind(other),
                });
            }
        }
    }
    stack.pop();
    memo.insert(id, longest.clone());
    Ok(longest)
}

fn unsupported_kind(expression: &Expression) -> &'static str {
    match expression {
        Expression::RuntimeValue { .. } => "runtime_value",
        Expression::RuntimeParameter { .. } => "runtime_parameter",
        Expression::RuntimeParameterDefault { .. } => "runtime_parameter_default",
        Expression::SourceField { .. } => "source_field",
        Expression::Position { .. } => "position",
        Expression::SequenceExists { .. } => "sequence_exists",
        Expression::SequenceItemAt { .. } => "sequence_item_at",
        Expression::SequenceAggregate { .. } => "sequence_aggregate",
        Expression::ValueMap { .. } => "value_map",
        Expression::DelimitedTextField { .. } => "delimited_text_field",
        _ => "non_scalar_stage_expression",
    }
}
