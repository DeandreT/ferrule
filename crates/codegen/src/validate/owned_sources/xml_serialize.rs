//! Expression permission for owned XML operators, orthogonal to source
//! controls, failure eligibility and construction policies. The owned XML
//! levels nest: each admits every operator of the level below it. Legacy XML
//! operators keep their own unchanged gate at every level.
use super::*;
use crate::OwnedXml;

impl OwnedCapabilities {
    /// Whether these capabilities refuse `expression` as an owned XML operator.
    pub(super) fn closes_xml(self, expression: &Expression) -> bool {
        match expression {
            Expression::OwnedXmlSerialize(_) => self.xml < OwnedXml::Serialize,
            Expression::OwnedXmlMixedContent(_) => self.xml < OwnedXml::MixedContent,
            _ => false,
        }
    }

    /// Whether `scope`'s mixed-content construction is admitted: only an
    /// ungrouped scope whose own owned selector supplies the current item.
    pub(super) fn admits_xml_construction(self, scope: &TargetScope) -> bool {
        self.xml >= OwnedXml::MixedConstruction
            && matches!(
                scope.construction,
                TargetConstruction::XmlMixedContent { .. }
            )
            && scope.iteration.as_ref().is_some_and(|iteration| {
                iteration.owned_source_iteration().is_some()
                    && iteration.grouping().is_none()
                    && iteration.post_group_filter().is_none()
            })
    }

    /// Whether an admitted owned XML operator contributes the receiving
    /// context's scalar demands of its replacement expressions.
    pub(super) fn permits_xml_inputs(self, expression: &Expression) -> bool {
        matches!(expression, Expression::OwnedXmlMixedContent(_)) && !self.closes_xml(expression)
    }
}
