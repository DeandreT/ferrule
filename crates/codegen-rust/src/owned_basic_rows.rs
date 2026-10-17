//! Scope-local demand deliberately excludes global failure expressions. Failure
//! roots are still retained globally for emission, never for target dispatch.

use super::*;

pub(super) fn target_expression_usage(
    program: &Program,
    scopes: &[(String, &TargetScope, Vec<String>, Vec<String>)],
) -> BTreeSet<NodeId> {
    let mut pending = Vec::new();
    for (_, scope, _, _) in scopes {
        pending.extend(scope.bindings.iter().map(|binding| binding.expression));
        if let TargetConstruction::Scalar { expression, .. } = &scope.construction {
            pending.push(*expression);
        }
        if let Some(iteration) = &scope.iteration {
            pending.extend(iteration.roots());
        }
    }
    let expressions = program
        .expressions
        .iter()
        .map(|node| (node.id, &node.expression))
        .collect::<BTreeMap<_, _>>();
    let mut retained = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if retained.insert(node)
            && let Some(expression) = expressions.get(&node)
        {
            pending.extend(expression_inputs(expression));
        }
    }
    retained
}
