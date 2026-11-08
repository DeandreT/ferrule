//! Cross-field and recursive schema metadata validation.

use crate::{DatabaseForeignKeySide, SchemaKind, SchemaNode};

impl SchemaNode {
    /// Checks every cross-field schema metadata invariant.
    pub fn metadata_is_valid(&self) -> bool {
        self.alternatives_are_valid()
            && self.json_array_wrapper_is_valid()
            && self.json_tuple_frame_is_valid()
            && self.required_fields_are_valid()
            && self.xml_name_alternatives_are_valid()
            && self.recursive_ref_is_valid()
            && self.fixed_is_valid()
            && self.json_allowed_values_are_valid()
            && self.numeric_range_is_valid()
            && self.json_multiple_of_is_valid()
            && self.item_count_range_is_valid()
            && self.json_contains_are_valid()
            && self.json_dependent_schemas_are_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_pattern_property_names_are_valid()
            && self.json_property_names_are_valid()
            && self.json_unique_items_is_valid()
            && self.string_length_range_is_valid()
            && self.json_patterns_are_valid()
            && self.json_formats_are_valid()
            && self.value_generation_is_valid()
            && self.default_is_valid()
            && self.alternative_mode_is_valid()
            && self.xml_alternative_kind_is_valid()
            && self.xml_type_alternatives_are_valid()
            && self.xml_default_type_is_valid()
            && self.xml_optional_is_valid()
            && self.xml_attribute_required_is_valid()
            && self.xml_repeating_sequences_are_valid()
            && self.xml_repeating_choices_are_valid()
            && self.database_relation_is_valid()
            && self.nullable_is_valid()
            && self.container_nullable_is_valid()
            && self.json_object_item_nullable_is_valid()
            && self.json_any_is_valid()
            && self.json_undeclared_properties_are_valid()
            && self.xml_wildcard_namespace_is_valid()
            && self.xml_wildcard_process_contents_is_valid()
            && self.xml_wildcard_excluded_names_are_valid()
    }

    fn metadata_without_pattern_evaluation_is_valid(&self) -> bool {
        self.alternatives_are_valid()
            && self.json_array_wrapper_is_valid()
            && self.json_tuple_frame_is_valid()
            && self.required_fields_are_valid()
            && self.xml_name_alternatives_are_valid()
            && self.recursive_ref_is_valid()
            && self.fixed_is_valid()
            && self.json_allowed_values_are_valid()
            && self.numeric_range_is_valid()
            && self.json_multiple_of_is_valid()
            && self.item_count_range_is_valid()
            && self.json_contains_are_structurally_valid()
            && self.json_dependent_schemas_are_structurally_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && (self.json_pattern_property_names.is_none()
                || self.json_pattern_property_name_expectations().is_some())
            && self.json_property_names_are_structurally_valid()
            && self.json_unique_items_is_valid()
            && self.string_length_range_is_valid()
            && self.json_patterns_are_valid()
            && self.json_formats_are_valid()
            && self.value_generation_is_valid()
            && self.default_is_valid()
            && self.alternative_mode_is_valid()
            && self.xml_alternative_kind_is_valid()
            && self.xml_type_alternatives_are_valid()
            && self.xml_default_type_is_valid()
            && self.xml_optional_is_valid()
            && self.xml_attribute_required_is_valid()
            && self.xml_repeating_sequences_are_valid()
            && self.xml_repeating_choices_are_valid()
            && self.database_relation_is_valid()
            && self.nullable_is_valid()
            && self.container_nullable_is_valid()
            && self.json_object_item_nullable_is_valid()
            && self.json_any_is_valid()
            && self.json_undeclared_properties_are_valid()
            && self.xml_wildcard_namespace_is_valid()
            && self.xml_wildcard_process_contents_is_valid()
            && self.xml_wildcard_excluded_names_are_valid()
    }

    pub fn recursive_ref_is_valid(&self) -> bool {
        let Some(anchor) = &self.recursive_ref else {
            return true;
        };
        !anchor.is_empty()
            && !self.attribute
            && !self.text
            && matches!(
                &self.kind,
                SchemaKind::Group {
                    children,
                    alternatives,
                    required,
                    xml_restricted_alternatives,
                    dynamic,
                } if children.is_empty()
                    && alternatives.is_empty()
                    && required.is_empty()
                    && xml_restricted_alternatives.is_empty()
                    && dynamic.is_none()
            )
            && self.alternative_mode.is_exclusive()
            && self.xml_alternative_kind.is_xsi_type()
    }

    /// Checks that fixed-value metadata remains limited to one scalar type.
    pub fn fixed_is_valid(&self) -> bool {
        self.fixed.is_none()
            || (self.json_allowed_values.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Recursively checks all local metadata invariants, including private
    /// JSON predicate schema trees.
    pub fn metadata_tree_is_valid(&self) -> bool {
        self.metadata_is_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children.iter().all(SchemaNode::metadata_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::metadata_tree_is_valid)
                }
            }
    }

    pub(super) fn metadata_tree_without_pattern_evaluation_is_valid(&self) -> bool {
        self.metadata_without_pattern_evaluation_is_valid()
            && self.json_contains.as_ref().is_none_or(|constraints| {
                constraints.as_slice().iter().all(|constraint| {
                    constraint
                        .predicate()
                        .as_schema()
                        .is_none_or(SchemaNode::metadata_tree_without_pattern_evaluation_is_valid)
                })
            })
            && self
                .json_dependent_schemas
                .as_ref()
                .is_none_or(|constraints| {
                    constraints.as_slice().iter().all(|constraint| {
                        constraint.predicate().as_schema().is_none_or(
                            SchemaNode::metadata_tree_without_pattern_evaluation_is_valid,
                        )
                    })
                })
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::metadata_tree_without_pattern_evaluation_is_valid)
                        && dynamic.as_deref().is_none_or(
                            SchemaNode::metadata_tree_without_pattern_evaluation_is_valid,
                        )
                }
            }
    }

    /// Checks that generated-value metadata remains scalar-only and cannot
    /// conflict with repetition or a fixed literal.
    pub fn value_generation_is_valid(&self) -> bool {
        self.value_generation.is_none()
            || (!self.repeating
                && self.fixed.is_none()
                && self.default.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Checks that XML default metadata remains non-repeating, scalar, and
    /// mutually exclusive with fixed and generated values.
    pub fn default_is_valid(&self) -> bool {
        self.default.is_none()
            || (!self.repeating
                && self.fixed.is_none()
                && self.value_generation.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Checks that declared database relation metadata belongs to this nested table.
    pub fn database_relation_is_valid(&self) -> bool {
        let Some(relation) = &self.database_relation else {
            return true;
        };
        let Some((table, join_column)) = self.name.split_once('|') else {
            return false;
        };
        if table.is_empty()
            || join_column.is_empty()
            || join_column.contains('|')
            || relation.parent_column.is_empty()
            || relation.child_column.is_empty()
        {
            return false;
        }
        let join_matches_owner = match relation.foreign_key_side {
            DatabaseForeignKeySide::Parent => {
                join_column.eq_ignore_ascii_case(&relation.parent_column)
            }
            DatabaseForeignKeySide::Child => {
                join_column.eq_ignore_ascii_case(&relation.child_column)
            }
        };
        join_matches_owner
            && self.repeating
            && self.recursive_ref.is_none()
            && matches!(self.kind, SchemaKind::Group { .. })
    }
}
