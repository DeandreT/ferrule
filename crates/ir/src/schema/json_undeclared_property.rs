//! Authored properties of open JSON objects that the object's schema admits
//! without declaring them in `properties`.
//!
//! A mapping may name one such property exactly, for example `code` beside a
//! declared `Code`. The member is recorded as an explicit child marked with
//! [`SchemaNode::json_undeclared_property`] instead of being matched by case
//! folding or added to the authored schema. Its schema is a copy of the
//! object's dynamic value schema and its presence remains owned by the
//! object's `required` list.

use std::fmt;

use crate::{SchemaKind, SchemaNode};

/// Why an object cannot carry one authored undeclared property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonUndeclaredPropertyError {
    /// The node is not a JSON object.
    NotAnObject,
    /// The name is empty or a reserved private port name.
    InvalidName,
    /// The object already declares a property with exactly this name.
    Declared,
    /// The object admits no runtime-named property.
    Closed,
    /// Object alternatives cannot be combined with runtime-named properties.
    Alternatives,
    /// The object's `patternProperties` selectors do not match the name.
    NotSelected,
    /// The object's `propertyNames` assertions reject the name.
    NameRejected,
}

impl fmt::Display for JsonUndeclaredPropertyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAnObject => "the owner is not a JSON object",
            Self::InvalidName => "the property name is empty or reserved",
            Self::Declared => "the object already declares this property",
            Self::Closed => "the object does not admit additional properties",
            Self::Alternatives => "the object uses closed object alternatives",
            Self::NotSelected => "no `patternProperties` selector matches the property name",
            Self::NameRejected => "the object's `propertyNames` assertions reject the name",
        })
    }
}

impl std::error::Error for JsonUndeclaredPropertyError {}

impl SchemaNode {
    /// Records `name` as an authored property of this open object that its
    /// JSON Schema admits through the dynamic value schema without declaring
    /// it in `properties`.
    ///
    /// The name is kept exactly and is never matched against declared
    /// properties by case. The member copies the dynamic value schema, so it
    /// carries a value type only when the schema constrains the admitted
    /// properties. The object's `required` list is unchanged and continues to
    /// own the member's presence.
    pub fn add_json_undeclared_property(
        &mut self,
        name: &str,
    ) -> Result<(), JsonUndeclaredPropertyError> {
        if !is_member_name(name) {
            return Err(JsonUndeclaredPropertyError::InvalidName);
        }
        let SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } = &mut self.kind
        else {
            return Err(JsonUndeclaredPropertyError::NotAnObject);
        };
        if children.iter().any(|child| child.name == name) {
            return Err(JsonUndeclaredPropertyError::Declared);
        }
        if !alternatives.is_empty() {
            return Err(JsonUndeclaredPropertyError::Alternatives);
        }
        let Some(dynamic) = dynamic.as_deref() else {
            return Err(JsonUndeclaredPropertyError::Closed);
        };
        if self
            .json_pattern_property_names
            .as_ref()
            .is_some_and(|selectors| !selectors.matches(name))
        {
            return Err(JsonUndeclaredPropertyError::NotSelected);
        }
        if self
            .json_property_names
            .as_ref()
            .is_some_and(|constraints| !constraints.accepts(name))
        {
            return Err(JsonUndeclaredPropertyError::NameRejected);
        }
        let mut member = dynamic.clone();
        member.name = name.to_owned();
        member.json_undeclared_property = true;
        children.push(member);
        Ok(())
    }

    /// Checks that every authored undeclared property of this object is an
    /// exact copy of its dynamic value schema under a valid name.
    ///
    /// Pattern selection of the names is checked with the other
    /// `patternProperties` expectations.
    pub(crate) fn json_undeclared_properties_are_valid(&self) -> bool {
        let SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } = &self.kind
        else {
            return true;
        };
        if dynamic
            .as_deref()
            .is_some_and(|dynamic| dynamic.json_undeclared_property)
        {
            return false;
        }
        let mut members = children
            .iter()
            .filter(|child| child.json_undeclared_property)
            .peekable();
        if members.peek().is_none() {
            return true;
        }
        let Some(dynamic) = dynamic.as_deref() else {
            return false;
        };
        alternatives.is_empty()
            && members.all(|member| is_member_name(&member.name) && copies(member, dynamic))
    }
}

fn is_member_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('\u{1f}')
}

fn copies(member: &SchemaNode, dynamic: &SchemaNode) -> bool {
    let mut member = member.clone();
    member.name.clone_from(&dynamic.name);
    member.json_undeclared_property = false;
    member == *dynamic
}

#[cfg(test)]
mod tests {
    use super::JsonUndeclaredPropertyError as Error;
    use crate::{
        EffectivePresence, GroupAlternative, JsonPatternPropertyNames, JsonPropertyNameConstraints,
        ScalarType, SchemaKind, SchemaNode, StringLengthRange,
    };

    fn any(name: &str) -> SchemaNode {
        SchemaNode::scalar(name, ScalarType::String)
            .json_any()
            .unwrap()
    }

    fn open_object() -> SchemaNode {
        SchemaNode::group("root", vec![SchemaNode::scalar("Code", ScalarType::Int)])
            .with_dynamic_fields(any("*"))
            .unwrap()
            .with_required_fields(vec!["code".into()])
            .unwrap()
    }

    fn decode(schema: &SchemaNode) -> Result<SchemaNode, serde_json::Error> {
        serde_json::from_value(serde_json::to_value(schema)?)
    }

    #[test]
    fn member_copies_the_dynamic_schema_under_its_exact_name() {
        let mut object = open_object();
        object.add_json_undeclared_property("code").unwrap();
        let member = object.child("code").unwrap();
        assert!(member.json_undeclared_property);
        assert!(member.json_any);
        assert_eq!(
            object
                .child("Code")
                .map(|code| code.json_undeclared_property),
            Some(false)
        );
        assert_eq!(
            object.json_property_presence("code"),
            Some(EffectivePresence::Required)
        );
        assert!(object.metadata_tree_is_valid());
        assert_eq!(decode(&object).unwrap(), object);
        assert_eq!(
            object.add_json_undeclared_property("code"),
            Err(Error::Declared)
        );
    }

    #[test]
    fn typed_dynamic_schema_supplies_the_member_type() {
        let mut object = SchemaNode::group("root", Vec::new())
            .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::Int))
            .unwrap();
        object.add_json_undeclared_property("code").unwrap();
        assert!(matches!(
            object.child("code").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
        assert!(object.metadata_tree_is_valid());
    }

    #[test]
    fn objects_that_do_not_admit_the_name_refuse_it() {
        let mut closed =
            SchemaNode::group("root", vec![SchemaNode::scalar("Code", ScalarType::Int)]);
        assert_eq!(
            closed.add_json_undeclared_property("code"),
            Err(Error::Closed)
        );
        assert_eq!(
            closed.add_json_undeclared_property("Code"),
            Err(Error::Declared)
        );
        let mut scalar = SchemaNode::scalar("value", ScalarType::Int);
        assert_eq!(
            scalar.add_json_undeclared_property("code"),
            Err(Error::NotAnObject)
        );
        let mut object = open_object();
        for name in ["", "\u{1f}private"] {
            assert_eq!(
                object.add_json_undeclared_property(name),
                Err(Error::InvalidName)
            );
        }
        let mut selected = SchemaNode::group("root", Vec::new())
            .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::Int))
            .unwrap()
            .with_json_pattern_property_names(JsonPatternPropertyNames::new(["^x-"]).unwrap())
            .unwrap();
        assert_eq!(
            selected.add_json_undeclared_property("code"),
            Err(Error::NotSelected)
        );
        selected.add_json_undeclared_property("x-code").unwrap();
        assert!(selected.metadata_tree_is_valid());
        let mut short = open_object()
            .with_required_fields(Vec::new())
            .unwrap()
            .with_json_property_names(
                JsonPropertyNameConstraints::schema(
                    None,
                    StringLengthRange::new(0, Some(3)),
                    None,
                    Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            short.add_json_undeclared_property("code"),
            Err(Error::NameRejected)
        );
    }

    #[test]
    fn saved_members_must_remain_exact_copies_of_an_open_object() {
        let mut object = open_object();
        object.add_json_undeclared_property("code").unwrap();
        let SchemaKind::Group { children, .. } = &mut object.kind else {
            unreachable!()
        };
        children[1].kind = SchemaKind::Scalar {
            ty: ScalarType::Int,
        };
        children[1].json_any = false;
        assert!(!object.metadata_tree_is_valid());
        assert!(decode(&object).is_err());

        let mut closed = SchemaNode::group("root", vec![any("code")]);
        let SchemaKind::Group { children, .. } = &mut closed.kind else {
            unreachable!()
        };
        children[0].json_undeclared_property = true;
        assert!(!closed.metadata_tree_is_valid());
        assert!(decode(&closed).is_err());

        let mut root = any("root");
        root.json_undeclared_property = true;
        assert!(decode(&root).is_err());

        let mut alternatives = SchemaNode::group("root", vec![any("code")]);
        assert!(alternatives.set_alternatives(vec![GroupAlternative {
            name: "only".into(),
            members: vec!["code".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }]));
        assert_eq!(
            alternatives.add_json_undeclared_property("other"),
            Err(Error::Alternatives)
        );
    }

    #[test]
    fn selectors_must_admit_saved_members() {
        let mut selected = SchemaNode::group("root", Vec::new())
            .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::Int))
            .unwrap()
            .with_json_pattern_property_names(JsonPatternPropertyNames::new(["^x-"]).unwrap())
            .unwrap();
        selected.add_json_undeclared_property("x-code").unwrap();
        let SchemaKind::Group { children, .. } = &mut selected.kind else {
            unreachable!()
        };
        children[0].name = "code".into();
        assert!(!selected.metadata_tree_is_valid());
        assert!(decode(&selected).is_err());
    }
}
