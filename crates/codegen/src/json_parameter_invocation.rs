use crate::{DocumentProgram, Expression, TargetConstruction};
use mapping::{JsonParameterInvocationPlan, JsonParameterPlanError};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub enum JsonParameterBoundaryError {
    Plan(JsonParameterPlanError),
    Boundary(crate::DocumentVisitorBoundaryError),
    Unsupported(&'static str),
    Descriptor(serde_json::Error),
}

impl std::fmt::Display for JsonParameterBoundaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plan(error) => error.fmt(f),
            Self::Boundary(error) => error.fmt(f),
            Self::Unsupported(reason) => write!(f, "JSON parameter invocation: {reason}"),
            Self::Descriptor(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for JsonParameterBoundaryError {}

/// Bind a filename-only outer key to the exact inner document program.
/// Stateful inner programs require a shared invocation context and stay refused.
pub fn prepare_json_parameter_invocation(
    document: &DocumentProgram,
    plan: &JsonParameterInvocationPlan,
) -> Result<String, JsonParameterBoundaryError> {
    use JsonParameterBoundaryError as Error;
    plan.validate().map_err(Error::Plan)?;
    let program = document.program();
    crate::validate_document_filename_clock(program, None).map_err(Error::Boundary)?;
    if !program.extra_sources.is_empty()
        || !program.extra_targets.is_empty()
        || !program.user_functions.is_empty()
        || !program.failure_rules.is_empty()
        || program
            .expressions
            .iter()
            .any(|node| matches!(node.expression, Expression::InvocationCounter { .. }))
    {
        return Err(Error::Unsupported(
            "requires a stateless physical grouped body",
        ));
    }
    let keys = program
        .expressions
        .iter()
        .filter_map(|node| match &node.expression {
            Expression::RuntimeParameter {
                name,
                ty: ir::ScalarType::String,
            } if name == &plan.key_parameter => Some(node.id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [key] = keys.as_slice() else {
        return Err(Error::Unsupported("requires one exact String key binding"));
    };
    if program.expressions.iter().any(|node| {
        matches!(&node.expression,
        Expression::RuntimeParameter { name, .. } | Expression::RuntimeParameterDefault { name, .. }
        if name == &plan.parameter || name == &plan.key_parameter && node.id != *key)
    }) {
        return Err(Error::Unsupported(
            "parameter input or key binding has another use",
        ));
    }
    if matches!(document.primary_filename(), crate::PrimaryFilenamePlan::Parameter { name, .. } if name == &plan.key_parameter || name == &plan.parameter)
    {
        return Err(Error::Unsupported(
            "outer key cannot replace physical acquisition",
        ));
    }
    let mut users = BTreeMap::<_, Vec<_>>::new();
    for node in &program.expressions {
        for dependency in crate::validate::graph_dependencies::of(&node.expression) {
            users.entry(dependency).or_default().push(node.id);
        }
    }
    let mut dependent = BTreeSet::from([*key]);
    let mut pending = vec![*key];
    while let Some(node) = pending.pop() {
        for user in users.get(&node).into_iter().flatten() {
            if dependent.insert(*user) {
                pending.push(*user);
            }
        }
    }
    let filename = program
        .root
        .iteration
        .as_ref()
        .unwrap()
        .dynamic_document_iteration()
        .unwrap()
        .output_path();
    if !dependent.contains(&filename) {
        return Err(Error::Unsupported(
            "output filename does not use the outer key",
        ));
    }
    let mut pending = vec![(&program.root, true)];
    while let Some((scope, root)) = pending.pop() {
        let mut roots = scope
            .bindings
            .iter()
            .map(|binding| binding.expression)
            .collect::<Vec<_>>();
        match scope.construction {
            TargetConstruction::Group => {}
            TargetConstruction::Scalar { expression, .. } => roots.push(expression),
            _ => {
                return Err(Error::Unsupported(
                    "unsupported outer-parameter body construction",
                ));
            }
        }
        if let Some(iteration) = &scope.iteration {
            let mut controls = iteration.roots().collect::<Vec<_>>();
            if root {
                // Exempt exactly the output-path slot, not a filter/group key
                // which happens to refer to the same expression identity.
                let output = controls.iter().rposition(|id| *id == filename).unwrap();
                controls.remove(output);
            }
            roots.extend(controls);
            if let Some(sequence) = iteration.concatenated() {
                pending.extend(sequence.iter().map(|scope| (scope, false)));
            }
        }
        if roots.iter().any(|id| dependent.contains(id)) {
            return Err(Error::Unsupported(
                "outer key reaches the body or an input control",
            ));
        }
        pending.extend(scope.children.iter().map(|scope| (scope, false)));
    }
    serde_json::to_string(plan).map_err(Error::Descriptor)
}
