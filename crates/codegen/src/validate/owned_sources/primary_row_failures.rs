//! The primary-row permission is independent of target scope controls. This is
//! generation-time classification only; the runtime must prove the actual base.

use super::*;

/// Validate window-profile targets and the explicitly bounded primary-row
/// failure companion. Existing profile validators retain their closed rules.
pub fn validate_basic_owned_row_failure_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_profiles(program, BasicOwnedProfile::WindowsAndSortAndFilter, true)
}

/// Classify a potentially owned rule for the primary-row runtime boundary.
/// Callers must still validate the complete Program before emitting code.
pub fn is_basic_owned_primary_row_failure(program: &Program, rule: &crate::FailureRule) -> bool {
    let FailureIteration::Source(source) = &rule.iteration else {
        return false;
    };
    let [collection] = source.path() else {
        return false;
    };
    if collection.is_empty()
        || !matches!(program.source.kind, SchemaKind::Group { .. })
        || program.source.child(collection).is_none()
        || basic_dynamic_path(program, source.path())
    {
        return false;
    }
    let mut pending = rule
        .selection
        .predicate()
        .into_iter()
        .chain(rule.message)
        .collect::<Vec<_>>();
    let mut seen = std::collections::BTreeSet::new();
    let mut owned = false;
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            return false;
        };
        match &node.expression {
            Expression::OwnedSourceField(field) => {
                owned = true;
                if matches!(&field.source.owner, mapping::SourceOwner::Named { .. })
                    && !field.frame.as_ref().is_none_or(Vec::is_empty)
                {
                    return false;
                }
                if dynamic_owner(
                    program,
                    &request(&node.expression).expect("owned field request"),
                )
                .is_some()
                {
                    return false;
                }
            }
            Expression::OwnedPosition(position) => {
                owned = true;
                if !matches!(position.owner, mapping::SourceOwner::Primary) {
                    return false;
                }
            }
            Expression::SourceField { path, frame } => {
                if basic_dynamic_path(program, path)
                    || frame
                        .as_ref()
                        .is_some_and(|frame| basic_dynamic_path(program, frame))
                {
                    return false;
                }
            }
            Expression::Position { .. }
            | Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::Raise { .. }
            | Expression::ValueMap { .. } => {}
            _ => return false,
        }
        pending.extend(graph_dependencies::of(&node.expression));
    }
    owned
}
