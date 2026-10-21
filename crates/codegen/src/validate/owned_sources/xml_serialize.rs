//! Expression permission for owned XML operators, orthogonal to source
//! controls, failure eligibility and construction policies.
use super::{
    AdjacencyTreePolicy, BasicOwnedProfile, DynamicFieldPolicy, FailurePolicy, PathHierarchyPolicy,
    SearchPolicy, SequenceItemAtPolicy, validate_basic_owned_with_xml,
};
use crate::{Expression, validate::ProgramValidationError};

/// The owned XML companions nest: each one admits every operator of the
/// companion it composes. Every other profile refuses an owned XML operator;
/// legacy XML operators keep their own unchanged gate in every profile.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OwnedXmlPolicy {
    Closed,
    /// Owned XML serialization.
    Serialize,
    /// Serialization plus owned mixed-content atomization.
    MixedContent,
}

impl OwnedXmlPolicy {
    /// Whether this policy refuses `expression` as an owned XML operator.
    pub(super) fn closes(self, expression: &Expression) -> bool {
        match expression {
            Expression::OwnedXmlSerialize(_) => self < Self::Serialize,
            Expression::OwnedXmlMixedContent(_) => self < Self::MixedContent,
            _ => false,
        }
    }

    /// Whether an admitted owned XML operator contributes the receiving
    /// context's scalar demands of its replacement expressions.
    pub(super) fn permits_inputs(self, expression: &Expression) -> bool {
        matches!(expression, Expression::OwnedXmlMixedContent(_)) && !self.closes(expression)
    }
}

/// Explicitly admits owned XML serialization in Basic target contexts, composing
/// the owned dynamic source-field companion: owned dynamic reads, owned searches,
/// owned IntoBlocks grouping, recursive filtering of owned selectors, ItemAt over
/// scalar producers and owned recursive walks, owned path-hierarchy and
/// adjacency-tree constructions, EndingWith source controls, primary-row failures
/// and owned failure-source walks.
pub fn validate_basic_owned_xml_serialize_emission(
    program: &crate::Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_xml_with(program, OwnedXmlPolicy::Serialize)
}

/// The dynamic source-field composition plus one owned XML companion level.
pub(super) fn validate_basic_owned_xml_with(
    program: &crate::Program,
    owned_xml: OwnedXmlPolicy,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_xml(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndIntoBlocksAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        PathHierarchyPolicy::Owned,
        AdjacencyTreePolicy::OwnedWithRecursiveFilter,
        SearchPolicy::Basic,
        DynamicFieldPolicy::Owned,
        owned_xml,
    )
}

/// Explicitly admits owned XML mixed-content atomization in Basic target
/// contexts, composing the owned XML serialization companion. Its parent
/// replacements are Basic receiving-context demands; occurrence replacements
/// add only an owned occurrence of the group's own endpoint.
pub fn validate_basic_owned_xml_mixed_content_emission(
    program: &crate::Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_xml_with(program, OwnedXmlPolicy::MixedContent)
}
