//! Group alternative membership, validation and XML type selection.

use crate::{
    GroupAlternative, GroupAlternativeConstraintValue, GroupAlternativeMode, SchemaKind,
    SchemaNode, XmlAlternativeKind,
};

impl SchemaNode {
    /// Attaches validated alternative membership to a group node.
    pub fn with_alternatives(mut self, alternatives: Vec<GroupAlternative>) -> Option<Self> {
        self.set_alternatives(alternatives).then_some(self)
    }

    /// Attaches validated inclusive alternative membership to a group node.
    pub fn with_inclusive_alternatives(
        mut self,
        alternatives: Vec<GroupAlternative>,
    ) -> Option<Self> {
        self.set_group_alternatives(
            alternatives,
            GroupAlternativeMode::Inclusive,
            XmlAlternativeKind::XsiType,
        )
        .then_some(self)
    }

    /// Replaces alternative membership when it is valid for this group.
    pub fn set_alternatives(&mut self, alternatives: Vec<GroupAlternative>) -> bool {
        self.set_group_alternatives(
            alternatives,
            GroupAlternativeMode::Exclusive,
            XmlAlternativeKind::XsiType,
        )
    }

    /// Attaches exclusive alternatives represented by concrete XML element
    /// names from one XSD substitution group.
    pub fn with_substitution_group_alternatives(
        mut self,
        alternatives: Vec<GroupAlternative>,
    ) -> Option<Self> {
        self.set_substitution_group_alternatives(alternatives)
            .then_some(self)
    }

    pub fn set_substitution_group_alternatives(
        &mut self,
        alternatives: Vec<GroupAlternative>,
    ) -> bool {
        self.set_group_alternatives(
            alternatives,
            GroupAlternativeMode::Exclusive,
            XmlAlternativeKind::SubstitutionGroup,
        )
    }

    fn set_group_alternatives(
        &mut self,
        alternatives: Vec<GroupAlternative>,
        mode: GroupAlternativeMode,
        xml_kind: XmlAlternativeKind,
    ) -> bool {
        let (previous, previous_restricted) = {
            let SchemaKind::Group {
                children,
                alternatives: target,
                required: _,
                xml_restricted_alternatives,
                dynamic,
            } = &mut self.kind
            else {
                return false;
            };
            if dynamic.is_some() || !valid_group_alternatives(children, &alternatives) {
                return false;
            }
            (
                std::mem::replace(target, alternatives),
                std::mem::take(xml_restricted_alternatives),
            )
        };
        let previous_mode = std::mem::replace(&mut self.alternative_mode, mode);
        let previous_xml_kind = std::mem::replace(&mut self.xml_alternative_kind, xml_kind);
        let previous_xml_type_alternatives =
            std::mem::replace(&mut self.xml_type_alternatives, false);
        let previous_xml_default_type = self.xml_default_type.take();
        if self.json_tuple_frame_is_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_pattern_property_names_are_valid()
            && self.json_property_names_are_valid()
        {
            true
        } else {
            let SchemaKind::Group {
                alternatives,
                xml_restricted_alternatives,
                ..
            } = &mut self.kind
            else {
                return false;
            };
            *alternatives = previous;
            *xml_restricted_alternatives = previous_restricted;
            self.alternative_mode = previous_mode;
            self.xml_alternative_kind = previous_xml_kind;
            self.xml_type_alternatives = previous_xml_type_alternatives;
            self.xml_default_type = previous_xml_default_type;
            false
        }
    }

    pub fn set_xml_restricted_alternatives(&mut self, restricted: Vec<String>) -> bool {
        let SchemaKind::Group {
            alternatives,
            xml_restricted_alternatives,
            ..
        } = &mut self.kind
        else {
            return false;
        };
        if !self.alternative_mode.is_exclusive()
            || !self.xml_alternative_kind.is_xsi_type()
            || restricted.iter().enumerate().any(|(index, name)| {
                restricted[..index].contains(name)
                    || !alternatives
                        .iter()
                        .any(|alternative| alternative.name == *name)
            })
        {
            return false;
        }
        *xml_restricted_alternatives = restricted;
        true
    }

    pub fn xml_restricted_alternatives(&self) -> &[String] {
        match &self.kind {
            SchemaKind::Group {
                xml_restricted_alternatives,
                ..
            } => xml_restricted_alternatives,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => &[],
        }
    }

    /// Checks metadata that may have entered through direct deserialization.
    pub fn alternatives_are_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group {
                children,
                alternatives,
                required: _,
                xml_restricted_alternatives,
                dynamic,
            } => {
                (alternatives.is_empty() || dynamic.is_none())
                    && (alternatives.is_empty() || valid_group_alternatives(children, alternatives))
                    && xml_restricted_alternatives
                        .iter()
                        .enumerate()
                        .all(|(index, name)| {
                            !xml_restricted_alternatives[..index].contains(name)
                                && alternatives
                                    .iter()
                                    .any(|alternative| alternative.name == *name)
                        })
                    && (xml_restricted_alternatives.is_empty()
                        || self.alternative_mode.is_exclusive()
                            && self.xml_alternative_kind.is_xsi_type())
            }
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
        }
    }

    /// Checks that inclusive semantics cannot exist without group
    /// alternatives or leak onto scalar nodes.
    pub fn alternative_mode_is_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group { alternatives, .. } => {
                !alternatives.is_empty() || self.alternative_mode.is_exclusive()
            }
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => {
                self.alternative_mode.is_exclusive()
            }
        }
    }

    /// Checks that element-name alternatives stay exclusive and group-scoped.
    pub fn xml_alternative_kind_is_valid(&self) -> bool {
        match self.xml_alternative_kind {
            XmlAlternativeKind::XsiType => true,
            XmlAlternativeKind::SubstitutionGroup => {
                self.alternative_mode.is_exclusive()
                    && self.recursive_ref.is_none()
                    && !self.attribute
                    && !self.text
                    && matches!(
                        &self.kind,
                        SchemaKind::Group { alternatives, .. } if !alternatives.is_empty()
                    )
            }
        }
    }

    /// XSD-derived type projections use member-set refinement rather than
    /// JSON Schema's exclusive `oneOf` interpretation.
    pub fn xml_type_alternatives_are_valid(&self) -> bool {
        if !self.xml_type_alternatives {
            return true;
        }
        self.alternative_mode == GroupAlternativeMode::Exclusive
            && self.xml_alternative_kind == XmlAlternativeKind::XsiType
            && matches!(
                &self.kind,
                SchemaKind::Group { alternatives, .. }
                    if !alternatives.is_empty()
                        && alternatives.iter().all(|alternative| {
                            !alternative.name.is_empty()
                                && alternative.required.is_empty()
                                && alternative.constraints.is_empty()
                        })
            )
    }

    /// A declared default is a concrete retained base, never a guessed member
    /// of an abstract or structurally unrelated alternative set.
    pub fn xml_default_type_is_valid(&self) -> bool {
        let Some(identity) = &self.xml_default_type else {
            return true;
        };
        if !self.alternatives_are_valid()
            || !self.xml_type_alternatives_are_valid()
            || !self.xml_type_alternatives
            || self.attribute
            || self.text
            || self.recursive_ref.is_some()
        {
            return false;
        }
        let SchemaKind::Group {
            alternatives,
            xml_restricted_alternatives,
            ..
        } = &self.kind
        else {
            return false;
        };
        let Some(base) = alternatives
            .iter()
            .find(|alternative| alternative.name == *identity)
        else {
            return false;
        };
        !xml_restricted_alternatives.contains(identity)
            && alternatives.iter().all(|alternative| {
                if xml_restricted_alternatives.contains(&alternative.name) {
                    alternative
                        .members
                        .iter()
                        .all(|member| base.members.contains(member))
                } else {
                    base.members
                        .iter()
                        .all(|member| alternative.members.contains(member))
                }
            })
    }

    pub fn alternative_mode(&self) -> GroupAlternativeMode {
        self.alternative_mode
    }

    pub fn alternatives(&self) -> &[GroupAlternative] {
        match &self.kind {
            SchemaKind::Group { alternatives, .. } => alternatives,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => &[],
        }
    }
}

fn valid_group_alternatives(children: &[SchemaNode], alternatives: &[GroupAlternative]) -> bool {
    !alternatives.is_empty()
        && children.iter().enumerate().all(|(index, child)| {
            !children[..index]
                .iter()
                .any(|previous| previous.name == child.name)
        })
        && alternatives.iter().enumerate().all(|(index, alternative)| {
            !alternative.name.is_empty()
                && !alternatives[..index]
                    .iter()
                    .any(|previous| previous.name == alternative.name)
                && alternative
                    .members
                    .iter()
                    .enumerate()
                    .all(|(member_index, member)| {
                        !alternative.members[..member_index].contains(member)
                            && children.iter().any(|child| child.name == *member)
                    })
                && alternative
                    .required
                    .iter()
                    .enumerate()
                    .all(|(required_index, required)| {
                        !alternative.required[..required_index].contains(required)
                            && alternative.members.contains(required)
                    })
                && alternative.constraints.iter().enumerate().all(
                    |(constraint_index, constraint)| {
                        !alternative.constraints[..constraint_index]
                            .iter()
                            .any(|previous| previous.member == constraint.member)
                            && alternative.members.contains(&constraint.member)
                            && children.iter().any(|child| {
                                child.name == constraint.member
                                    && !child.repeating
                                    && match child.kind {
                                        SchemaKind::Scalar { ty } => {
                                            constraint.value.is_valid_for(ty, child.nullable)
                                        }
                                        SchemaKind::ScalarUnion { types } => {
                                            constraint
                                                .value
                                                .scalar_type()
                                                .is_some_and(|ty| types.contains(ty))
                                                || matches!(
                                                    constraint.value,
                                                    GroupAlternativeConstraintValue::JsonNull
                                                ) && child.nullable
                                        }
                                        SchemaKind::Group { .. } => false,
                                    }
                            })
                    },
                )
        })
}
