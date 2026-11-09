//! XML namespace, wildcard and occurrence metadata.

use crate::{
    ScalarType, SchemaKind, SchemaNode, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD,
    XML_LOCAL_NAME_FIELD, XML_NAMESPACE_URI_FIELD, XML_TEXT_FIELD, XmlAlternativeKind,
    XmlNamespace, XmlRepeatingChoice, XmlRepeatingSequence, XmlWildcardNamespaceConstraint,
};

impl SchemaNode {
    /// Marks this XML name as explicitly unqualified.
    pub fn xml_unqualified(mut self) -> Self {
        self.xml_namespace = Some(XmlNamespace::Unqualified);
        self.xml_name_alternatives.clear();
        self
    }

    /// Marks this XML name as belonging to a non-empty namespace URI.
    pub fn xml_qualified(mut self, uri: impl Into<String>) -> Option<Self> {
        self.xml_namespace = Some(XmlNamespace::qualified(uri)?);
        self.xml_name_alternatives.clear();
        Some(self)
    }

    /// Adds exact namespace identities for XML elements that share this
    /// mapping-port local name and complete typed shape.
    pub fn with_xml_name_alternatives(mut self, alternatives: Vec<XmlNamespace>) -> Option<Self> {
        let previous = std::mem::replace(&mut self.xml_name_alternatives, alternatives);
        if self.xml_name_alternatives_are_valid() {
            Some(self)
        } else {
            self.xml_name_alternatives = previous;
            None
        }
    }

    pub fn xml_name_alternatives_are_valid(&self) -> bool {
        if self.xml_name_alternatives.is_empty() {
            return true;
        }
        let Some(primary) = &self.xml_namespace else {
            return false;
        };
        !self.attribute
            && !self.text
            && self.recursive_ref.is_none()
            && self.xml_alternative_kind != XmlAlternativeKind::SubstitutionGroup
            && self
                .xml_name_alternatives
                .iter()
                .enumerate()
                .all(|(index, namespace)| {
                    namespace != primary && !self.xml_name_alternatives[..index].contains(namespace)
                })
    }

    /// Returns whether one runtime XML namespace is an exact accepted name
    /// identity for this local element.
    pub fn xml_namespace_matches(&self, namespace: Option<&str>) -> bool {
        self.xml_namespace
            .iter()
            .chain(&self.xml_name_alternatives)
            .any(|candidate| candidate.matches(namespace))
    }

    pub fn with_xml_wildcard_namespace(
        mut self,
        constraint: XmlWildcardNamespaceConstraint,
    ) -> Option<Self> {
        self.xml_wildcard_namespace = Some(constraint);
        self.xml_wildcard_namespace_is_valid().then_some(self)
    }

    pub fn xml_wildcard_namespace_is_valid(&self) -> bool {
        self.xml_wildcard_namespace.is_none()
            || (matches!(
                self.name.as_str(),
                XML_ELEMENTS_FIELD | XML_ATTRIBUTES_FIELD
            ) && self.recursive_ref.is_none()
                && !self.attribute
                && !self.text
                && matches!(
                    &self.kind,
                    SchemaKind::Group { children, .. }
                        if [XML_LOCAL_NAME_FIELD, XML_NAMESPACE_URI_FIELD]
                            .into_iter()
                            .all(|name| children.iter().any(|child| {
                                child.name == name
                                    && !child.repeating
                                    && !child.attribute
                                    && !child.text
                                    && matches!(
                                        child.kind,
                                        SchemaKind::Scalar {
                                            ty: ScalarType::String
                                        }
                                    )
                            }))
                            && (self.name != XML_ATTRIBUTES_FIELD
                                || children.iter().any(|child| {
                                    child.name == XML_TEXT_FIELD
                                        && child.text
                                        && !child.repeating
                                        && !child.attribute
                                        && matches!(
                                            child.kind,
                                            SchemaKind::Scalar {
                                                ty: ScalarType::String
                                            }
                                        )
                                }))
                ))
    }

    pub fn xml_wildcard_process_contents_is_valid(&self) -> bool {
        self.xml_wildcard_process_contents.is_skip()
            || (self.xml_wildcard_namespace.is_some() && self.xml_wildcard_namespace_is_valid())
    }

    /// Checks that optional occurrence metadata belongs to a named singular
    /// XML element, including a singular recursive group reference.
    pub fn xml_optional_is_valid(&self) -> bool {
        !self.xml_optional
            || (!self.name.is_empty()
                && !matches!(
                    self.name.as_str(),
                    XML_TEXT_FIELD | XML_ELEMENTS_FIELD | XML_ATTRIBUTES_FIELD
                )
                && !self.name.starts_with('\u{1f}')
                && !self.attribute
                && !self.text
                && !self.repeating
                && matches!(
                    self.kind,
                    SchemaKind::Scalar { .. } | SchemaKind::Group { .. }
                ))
    }

    /// Sets optional XML occurrence metadata without leaving an invalid role.
    pub fn set_xml_optional(&mut self, optional: bool) -> bool {
        let previous = std::mem::replace(&mut self.xml_optional, optional);
        if self.xml_optional_is_valid() && self.json_tuple_frame_is_valid() {
            true
        } else {
            self.xml_optional = previous;
            false
        }
    }

    /// Checks that required-use metadata belongs to one named scalar attribute.
    pub fn xml_attribute_required_is_valid(&self) -> bool {
        !self.xml_attribute_required
            || (!self.name.is_empty()
                && !matches!(
                    self.name.as_str(),
                    XML_TEXT_FIELD | XML_ELEMENTS_FIELD | XML_ATTRIBUTES_FIELD
                )
                && !self.name.starts_with('\u{1f}')
                && self.attribute
                && !self.text
                && !self.repeating
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Sets XML attribute requiredness without leaving an invalid role.
    pub fn set_xml_attribute_required(&mut self, required: bool) -> bool {
        let previous = std::mem::replace(&mut self.xml_attribute_required, required);
        if self.xml_attribute_required_is_valid() {
            true
        } else {
            self.xml_attribute_required = previous;
            false
        }
    }

    pub fn xml_repeating_sequences_are_valid(&self) -> bool {
        let SchemaKind::Group { children, .. } = &self.kind else {
            return self.xml_repeating_sequences.is_empty();
        };
        let mut used = std::collections::BTreeSet::new();
        self.xml_repeating_sequences.iter().all(|sequence| {
            let positions = sequence
                .members
                .iter()
                .map(|member| {
                    let mut matches = children.iter().enumerate().filter(|(_, child)| {
                        child.name == member.name
                            && child.repeating
                            && !child.attribute
                            && !child.text
                    });
                    let position = matches.next().map(|(position, _)| position)?;
                    matches.next().is_none().then_some(position)
                })
                .collect::<Option<Vec<_>>>();
            sequence.members.len() > 1
                && sequence
                    .members
                    .iter()
                    .all(|member| !member.name.is_empty() && used.insert(member.name.as_str()))
                && positions.is_some_and(|positions| {
                    positions.windows(2).all(|pair| pair[1] == pair[0] + 1)
                })
        })
    }

    pub fn set_xml_repeating_sequences(&mut self, sequences: Vec<XmlRepeatingSequence>) -> bool {
        let previous = std::mem::replace(&mut self.xml_repeating_sequences, sequences);
        if self.xml_repeating_sequences_are_valid() && self.json_tuple_frame_is_valid() {
            true
        } else {
            self.xml_repeating_sequences = previous;
            false
        }
    }

    pub fn xml_repeating_choices_are_valid(&self) -> bool {
        let SchemaKind::Group { children, .. } = &self.kind else {
            return self.xml_repeating_choices.is_empty();
        };
        let sequence_members = self
            .xml_repeating_sequences
            .iter()
            .flat_map(|sequence| sequence.members.iter().map(|member| member.name.as_str()))
            .collect::<std::collections::BTreeSet<_>>();
        let mut used = std::collections::BTreeSet::new();
        self.xml_repeating_choices.iter().all(|choice| {
            let positions = choice
                .members
                .iter()
                .map(|member| {
                    let mut matches = children.iter().enumerate().filter(|(_, child)| {
                        child.name == *member
                            && child.repeating == choice.repeating
                            && !child.attribute
                            && !child.text
                    });
                    let position = matches.next().map(|(position, _)| position)?;
                    matches.next().is_none().then_some(position)
                })
                .collect::<Option<Vec<_>>>();
            choice.members.len() > 1
                && choice.members.iter().all(|member| {
                    !member.is_empty()
                        && !sequence_members.contains(member.as_str())
                        && used.insert(member.as_str())
                })
                && positions.is_some_and(|positions| {
                    positions.windows(2).all(|pair| pair[1] == pair[0] + 1)
                })
        })
    }

    pub fn set_xml_repeating_choices(&mut self, choices: Vec<XmlRepeatingChoice>) -> bool {
        let previous = std::mem::replace(&mut self.xml_repeating_choices, choices);
        if self.xml_repeating_choices_are_valid() && self.json_tuple_frame_is_valid() {
            true
        } else {
            self.xml_repeating_choices = previous;
            false
        }
    }
}
