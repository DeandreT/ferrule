//! Expression permission for owned XML serialization, orthogonal to source
//! controls, failure eligibility and construction policies.
use super::{
    AdjacencyTreePolicy, BasicOwnedProfile, DynamicFieldPolicy, FailurePolicy, PathHierarchyPolicy,
    SearchPolicy, SequenceItemAtPolicy, validate_basic_owned_with_xml,
};
use crate::{Expression, validate::ProgramValidationError};

/// Only the explicit owned XML serialization companion opts in. Every other
/// profile refuses an owned serializer as an XML operator; legacy serializers
/// keep their own unchanged gate in every profile.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum XmlSerializePolicy {
    Closed,
    Owned,
}

impl XmlSerializePolicy {
    /// Whether this policy refuses `expression` as an owned XML operator.
    pub(super) fn closes(self, expression: &Expression) -> bool {
        self == Self::Closed && matches!(expression, Expression::OwnedXmlSerialize(_))
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
    validate_basic_owned_with_xml(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndIntoBlocksAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        PathHierarchyPolicy::Owned,
        AdjacencyTreePolicy::OwnedWithRecursiveFilter,
        SearchPolicy::Basic,
        DynamicFieldPolicy::Owned,
        XmlSerializePolicy::Owned,
    )
}
