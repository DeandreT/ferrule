//! Static Basic source controls and grouping. Each control capability admits
//! one scope shape over an ordinary or owned source selector with a `Group` or
//! `Scalar` target; a scope's control is permitted when any admitted shape
//! matches it. Only an admitted grouped scope may mint `GroupMembers` ancestry.

use super::*;

fn static_target(scope: &TargetScope) -> bool {
    matches!(
        scope.construction,
        TargetConstruction::Group | TargetConstruction::Scalar { .. }
    )
}

fn static_input(iteration: &crate::IterationPlan) -> bool {
    matches!(
        iteration.input(),
        IterationSource::Source(_) | IterationSource::OwnedSource(_)
    )
}

fn ungrouped(iteration: &crate::IterationPlan) -> bool {
    iteration.grouping().is_none() && iteration.post_group_filter().is_none()
}

/// Repeated output, or `First` of a group target.
fn windowed_output(scope: &TargetScope, iteration: &crate::IterationPlan) -> bool {
    iteration.output() == IterationOutput::Repeated
        || iteration.output() == IterationOutput::First
            && matches!(scope.construction, TargetConstruction::Group)
}

/// A filtered repeated scope without sort, grouping or windows.
fn filter_shape(scope: &TargetScope) -> bool {
    static_target(scope)
        && scope.iteration.as_ref().is_some_and(|iteration| {
            iteration.filter().is_some()
                && static_input(iteration)
                && iteration.sort().is_none()
                && ungrouped(iteration)
                && iteration.windows().is_empty()
                && iteration.output() == IterationOutput::Repeated
        })
}

/// A sorted repeated scope without grouping or windows.
fn sort_shape(scope: &TargetScope) -> bool {
    static_target(scope)
        && scope.iteration.as_ref().is_some_and(|iteration| {
            iteration.sort().is_some()
                && static_input(iteration)
                && ungrouped(iteration)
                && iteration.windows().is_empty()
                && iteration.output() == IterationOutput::Repeated
        })
}

/// An ungrouped scope with windows or an implicit `First`.
fn window_shape(scope: &TargetScope) -> bool {
    static_target(scope)
        && scope.iteration.as_ref().is_some_and(|iteration| {
            (!iteration.windows().is_empty() || iteration.output() == IterationOutput::First)
                && static_input(iteration)
                && ungrouped(iteration)
                && windowed_output(scope, iteration)
        })
}

fn has_windows(scope: &TargetScope) -> bool {
    scope.iteration.as_ref().is_some_and(|iteration| {
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First
    })
}

impl OwnedCapabilities {
    /// A grouped scope in an admitted mode. This is an exact mode allowlist,
    /// not a generic grouping permission.
    pub(super) fn permits_group(self, scope: &TargetScope) -> bool {
        self.grouping.any()
            && static_target(scope)
            && scope.iteration.as_ref().is_some_and(|iteration| {
                self.grouping.permits(iteration.grouping())
                    && static_input(iteration)
                    && windowed_output(scope, iteration)
            })
    }

    pub(super) fn permits_filter(self, scope: &TargetScope) -> bool {
        let filtered = scope
            .iteration
            .as_ref()
            .is_some_and(|iteration| iteration.filter().is_some());
        self.controls.filter && filter_shape(scope)
            || self.controls.sort && sort_shape(scope)
            || self.controls.windows && window_shape(scope) && filtered
            || self.permits_group(scope) && filtered
    }

    pub(super) fn permits_sort(self, scope: &TargetScope) -> bool {
        let sorted = scope
            .iteration
            .as_ref()
            .is_some_and(|iteration| iteration.sort().is_some());
        self.controls.sort && sort_shape(scope)
            || self.controls.windows && window_shape(scope) && sorted
            || self.permits_group(scope) && sorted
    }

    pub(super) fn permits_windows(self, scope: &TargetScope) -> bool {
        self.controls.windows && window_shape(scope)
            || self.permits_group(scope) && has_windows(scope)
    }
}
