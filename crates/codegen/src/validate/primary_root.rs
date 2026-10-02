use std::collections::{BTreeMap, BTreeSet};

use mapping::NodeId;

use crate::{Expression, FailureIteration, Program, TargetConstruction, TargetScope};

use super::{ProgramValidationError, graph_dependencies};

/// This proof also protects the public neutral Program API. Root readers do not
/// acquire permission merely because another consumer uses the same expression
/// in a valid binding, or because an invalid branch happens to be unselected.
pub(super) fn validate(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    let primitives = expressions
        .iter()
        .filter_map(|(&node, expression)| {
            matches!(
                expression,
                Expression::SourceRootXmlTypeEquals { .. } | Expression::SourceRootField { .. }
            )
            .then_some(node)
        })
        .collect::<BTreeSet<_>>();
    if primitives.is_empty() {
        return Ok(());
    }
    let first = *primitives.first().expect("nonempty primitive inventory");
    if program.root.repeating
        || program.root.iteration.is_some()
        || !program.root.target_field.is_empty()
        || !program.root.children.is_empty()
        || !matches!(program.root.construction, TargetConstruction::Group)
        || program
            .extra_sources
            .iter()
            .any(|source| source.dynamic.is_some())
    {
        return Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: first });
    }
    let allowed = closure(
        program
            .root
            .bindings
            .iter()
            .map(|binding| binding.expression),
        expressions,
    );
    let mut forbidden_roots = Vec::new();
    for target in &program.extra_targets {
        scope_roots(&target.root, &mut forbidden_roots);
    }
    for rule in &program.failure_rules {
        if let FailureIteration::Generated(sequence) = &rule.iteration {
            forbidden_roots.extend(sequence.roots());
        }
        forbidden_roots.extend(rule.selection.predicate());
        forbidden_roots.extend(rule.message);
    }
    let forbidden = closure(forbidden_roots, expressions);
    for &node in &primitives {
        if !allowed.contains(&node) || forbidden.contains(&node) {
            return Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node });
        }
    }
    // Every ancestor of a root reader must remain a pure scalar expression.
    // Private item/tuple/function/reducer contexts are deliberately unproved.
    let mut parents: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for (&node, expression) in expressions {
        for dependency in graph_dependencies::of(expression) {
            parents.entry(dependency).or_default().push(node);
        }
    }
    let mut pending = primitives.iter().copied().collect::<Vec<_>>();
    let mut affected = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if affected.insert(node) {
            pending.extend(parents.get(&node).into_iter().flatten().copied());
        }
    }
    for node in affected {
        if !allowed.contains(&node)
            || !matches!(
                expressions[&node],
                Expression::SourceRootXmlTypeEquals { .. }
                    | Expression::SourceRootField { .. }
                    | Expression::Call { .. }
                    | Expression::If { .. }
                    | Expression::ValueMap { .. }
            )
        {
            return Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node });
        }
    }
    validate_source(program, expressions, &primitives)
}

fn closure(
    roots: impl IntoIterator<Item = NodeId>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> BTreeSet<NodeId> {
    let mut pending = roots.into_iter().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if seen.insert(node)
            && let Some(expression) = expressions.get(&node)
        {
            pending.extend(graph_dependencies::of(expression));
        }
    }
    seen
}

fn scope_roots(scope: &TargetScope, roots: &mut Vec<NodeId>) {
    roots.extend(scope.bindings.iter().map(|binding| binding.expression));
    match &scope.construction {
        TargetConstruction::Scalar { expression, .. } => roots.push(*expression),
        TargetConstruction::DynamicGroup {
            bindings, children, ..
        } => {
            for binding in bindings {
                roots.extend([binding.key, binding.value]);
            }
            for child in children {
                roots.push(child.key);
                scope_roots(&child.scope, roots);
            }
        }
        TargetConstruction::RecursiveFilter { predicate, .. } => roots.push(*predicate),
        TargetConstruction::AdjacencyTree { root, .. } => roots.extend(root),
        TargetConstruction::Group
        | TargetConstruction::CopyCurrentSource
        | TargetConstruction::XmlMixedContent { .. }
        | TargetConstruction::PathHierarchy { .. } => {}
    }
    if let Some(iteration) = &scope.iteration {
        roots.extend(iteration.roots());
        if let Some(sequence) = iteration.concatenated() {
            for segment in sequence.iter() {
                scope_roots(segment, roots);
            }
        }
    }
    for child in &scope.children {
        scope_roots(child, roots);
    }
}

fn validate_source(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
    primitives: &BTreeSet<NodeId>,
) -> Result<(), ProgramValidationError> {
    for &node in primitives {
        if !ir::primary_root_schema_is_supported(&program.source) {
            return Err(ProgramValidationError::InvalidPrimaryRootSchema { node });
        }
        match expressions[&node] {
            Expression::SourceRootXmlTypeEquals {
                canonical_expanded_type,
            } => {
                if !ir::primary_root_schema_has_type(&program.source, canonical_expanded_type) {
                    return Err(ProgramValidationError::InvalidPrimaryRootType {
                        node,
                        identity: canonical_expanded_type.clone(),
                    });
                }
            }
            Expression::SourceRootField { path, .. } => {
                let borrowed = path.iter().map(String::as_str).collect::<Vec<_>>();
                if !ir::primary_root_schema_has_scalar(&program.source, &borrowed) {
                    return Err(ProgramValidationError::InvalidPrimaryRootField {
                        node,
                        path: path.clone(),
                    });
                }
            }
            _ => unreachable!("primitive inventory includes only primary-root readers"),
        }
    }
    Ok(())
}
