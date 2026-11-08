//! JSON scalar domains, array assertions and nullability validation.

use crate::{JsonAllowedValue, NumericRange, ScalarType, SchemaKind, SchemaNode};

impl SchemaNode {
    /// Checks that JSON allowed-value metadata is compatible with this exact
    /// scalar domain and is the sole authority for JSON null membership.
    pub fn json_allowed_values_are_valid(&self) -> bool {
        let Some(values) = &self.json_allowed_values else {
            return true;
        };
        if self.json_any
            || self.fixed.is_some()
            || !self.is_scalar()
            || self.nullable != values.contains_json_null()
        {
            return false;
        }
        values.values().iter().all(|value| match value {
            JsonAllowedValue::String(_) => self.accepts_scalar_type(ScalarType::String),
            JsonAllowedValue::Int(_) => {
                self.accepts_scalar_type(ScalarType::Int)
                    || self.accepts_scalar_type(ScalarType::Float)
            }
            JsonAllowedValue::Float(_) => self.accepts_scalar_type(ScalarType::Float),
            JsonAllowedValue::Bool(_) => self.accepts_scalar_type(ScalarType::Bool),
            JsonAllowedValue::JsonNull => self.nullable,
        })
    }

    /// Recursively checks JSON allowed-value metadata for one schema tree.
    pub fn json_allowed_values_tree_is_valid(&self) -> bool {
        self.json_allowed_values_are_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_allowed_values_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_allowed_values_tree_is_valid)
                }
            }
    }

    /// Checks that numeric-range metadata matches one concrete numeric scalar,
    /// the Int member of an exact String-or-Int union, or the Float member of
    /// an unconstrained String-or-Float union. Union intervals have no fixed
    /// value; a concrete scalar's optional fixed value lies inside its interval.
    pub fn numeric_range_is_valid(&self) -> bool {
        let Some(range) = self.numeric_range else {
            return true;
        };
        match (range, &self.kind) {
            (
                NumericRange::Integer(range),
                SchemaKind::Scalar {
                    ty: ScalarType::Int,
                },
            ) => self.fixed.as_deref().is_none_or(|fixed| {
                fixed
                    .parse::<i64>()
                    .is_ok_and(|value| range.contains(value))
            }),
            (
                NumericRange::Number(range),
                SchemaKind::Scalar {
                    ty: ScalarType::Float,
                },
            ) => self.fixed.as_deref().is_none_or(|fixed| {
                fixed
                    .parse::<f64>()
                    .is_ok_and(|value| range.contains(value))
            }),
            (NumericRange::Integer(_), SchemaKind::ScalarUnion { .. })
                if self.is_string_int_union() =>
            {
                self.fixed.is_none() && !self.json_any
            }
            (NumericRange::Number(_), SchemaKind::ScalarUnion { .. })
                if self.is_string_float_union() =>
            {
                self.fixed.is_none()
                    && !self.json_any
                    && self.json_allowed_values.is_none()
                    && self.json_multiple_of.is_none()
                    && self.string_length_range.is_none()
                    && self.json_patterns.is_none()
            }
            _ => false,
        }
    }

    /// Checks that JSON `multipleOf` metadata is attached to a numeric-capable
    /// scalar and that an optional fixed lexical value is exactly divisible.
    pub fn json_multiple_of_is_valid(&self) -> bool {
        let Some(constraints) = &self.json_multiple_of else {
            return true;
        };
        if self.json_any
            || !(self.accepts_scalar_type(ScalarType::Int)
                || self.accepts_scalar_type(ScalarType::Float))
        {
            return false;
        }
        self.fixed.as_deref().is_none_or(|fixed| match self.kind {
            SchemaKind::Scalar {
                ty: ScalarType::Int,
            } => fixed
                .parse::<i64>()
                .is_ok_and(|value| constraints.matches_i64(value)),
            SchemaKind::Scalar {
                ty: ScalarType::Float,
            } => fixed
                .parse::<f64>()
                .is_ok_and(|value| constraints.matches_f64(value)),
            _ => false,
        })
    }

    /// Checks JSON `multipleOf` metadata throughout one complete schema tree.
    pub fn json_multiple_of_tree_is_valid(&self) -> bool {
        self.json_multiple_of_is_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_multiple_of_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_multiple_of_tree_is_valid)
                }
            }
    }

    /// Checks that item-count metadata remains attached to an array wrapper.
    pub fn item_count_range_is_valid(&self) -> bool {
        self.item_count_range.is_none() || self.repeating
    }

    /// Checks that every `contains` assertion belongs to an array wrapper and
    /// retains canonical, structurally valid predicate metadata.
    pub fn json_contains_are_valid(&self) -> bool {
        let Some(constraints) = &self.json_contains else {
            return true;
        };
        self.repeating
            && constraints.is_canonical()
            && constraints.as_slice().iter().all(|constraint| {
                constraint.predicate().as_schema().is_none_or(|schema| {
                    schema.metadata_tree_is_valid() && schema.private_unique_items_are_exact()
                })
            })
    }

    pub(super) fn json_contains_are_structurally_valid(&self) -> bool {
        self.json_contains.as_ref().is_none_or(|constraints| {
            self.repeating
                && constraints.collection_is_canonical()
                && constraints.as_slice().iter().all(|constraint| {
                    constraint
                        .predicate()
                        .as_schema()
                        .is_none_or(SchemaNode::private_unique_items_are_exact)
                })
        })
    }

    /// Recursively checks JSON `contains` placement and predicate schemas.
    pub fn json_contains_tree_is_valid(&self) -> bool {
        self.json_contains_are_valid()
            && self.json_contains.as_ref().is_none_or(|constraints| {
                constraints.as_slice().iter().all(|constraint| {
                    constraint
                        .predicate()
                        .as_schema()
                        .is_none_or(SchemaNode::json_contains_tree_is_valid)
                })
            })
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children.iter().all(SchemaNode::json_contains_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_contains_tree_is_valid)
                }
            }
    }

    /// Checks that every dependent-schema assertion belongs to an object and
    /// retains canonical, structurally valid predicate metadata.
    pub fn json_dependent_schemas_are_valid(&self) -> bool {
        let Some(constraints) = &self.json_dependent_schemas else {
            return true;
        };
        matches!(self.kind, SchemaKind::Group { .. })
            && constraints.is_canonical()
            && constraints.as_slice().iter().all(|constraint| {
                constraint.predicate().as_schema().is_none_or(|schema| {
                    schema.metadata_tree_is_valid() && schema.private_unique_items_are_exact()
                })
            })
    }

    pub(super) fn json_dependent_schemas_are_structurally_valid(&self) -> bool {
        self.json_dependent_schemas
            .as_ref()
            .is_none_or(|constraints| {
                matches!(self.kind, SchemaKind::Group { .. })
                    && constraints.is_canonical()
                    && constraints.as_slice().iter().all(|constraint| {
                        constraint
                            .predicate()
                            .as_schema()
                            .is_none_or(SchemaNode::private_unique_items_are_exact)
                    })
            })
    }

    fn private_unique_items_are_exact(&self) -> bool {
        if self.json_unique_items && self.item_shape_admits_lossy_json_number() {
            return false;
        }
        let predicates_are_exact = self.json_contains.as_ref().is_none_or(|constraints| {
            constraints.as_slice().iter().all(|constraint| {
                constraint
                    .predicate()
                    .as_schema()
                    .is_none_or(SchemaNode::private_unique_items_are_exact)
            })
        }) && self.json_dependent_schemas.as_ref().is_none_or(
            |constraints| {
                constraints.as_slice().iter().all(|constraint| {
                    constraint
                        .predicate()
                        .as_schema()
                        .is_none_or(SchemaNode::private_unique_items_are_exact)
                })
            },
        );
        predicates_are_exact
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::private_unique_items_are_exact)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::private_unique_items_are_exact)
                }
            }
    }

    fn item_shape_admits_lossy_json_number(&self) -> bool {
        if self.json_any {
            return true;
        }
        match &self.kind {
            SchemaKind::Scalar { ty } => *ty == ScalarType::Float,
            SchemaKind::ScalarUnion { types } => types.contains(ScalarType::Float),
            SchemaKind::Group {
                children, dynamic, ..
            } => {
                children
                    .iter()
                    .any(SchemaNode::item_shape_admits_lossy_json_number)
                    || dynamic
                        .as_deref()
                        .is_some_and(SchemaNode::item_shape_admits_lossy_json_number)
            }
        }
    }

    /// Recursively checks dependent-schema placement and predicate schemas.
    pub fn json_dependent_schemas_tree_is_valid(&self) -> bool {
        self.json_dependent_schemas_are_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_dependent_schemas_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_dependent_schemas_tree_is_valid)
                }
            }
    }

    /// Checks that JSON `uniqueItems` metadata remains attached to an array
    /// wrapper rather than its item shape.
    pub fn json_unique_items_is_valid(&self) -> bool {
        !self.json_unique_items || self.repeating
    }

    /// Recursively checks JSON `uniqueItems` placement for one schema tree.
    pub fn json_unique_items_tree_is_valid(&self) -> bool {
        self.json_unique_items_is_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_unique_items_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_unique_items_tree_is_valid)
                }
            }
    }

    /// Checks that string-length metadata matches a string-capable scalar
    /// domain and that an optional fixed lexical value lies inside the interval.
    pub fn string_length_range_is_valid(&self) -> bool {
        let Some(range) = self.string_length_range else {
            return true;
        };
        !self.json_any
            && self.accepts_scalar_type(ScalarType::String)
            && self
                .fixed
                .as_deref()
                .is_none_or(|fixed| range.contains_str(fixed))
    }

    /// Checks that JSON format annotations describe a string-capable scalar
    /// value or each scalar item of a repeating node.
    pub fn json_formats_are_valid(&self) -> bool {
        self.json_formats.is_empty()
            || (!self.json_any && self.accepts_scalar_type(ScalarType::String))
    }

    /// Checks that explicit JSON nullability remains scalar-only.
    pub fn nullable_is_valid(&self) -> bool {
        (!self.nullable || self.is_scalar()) && self.json_allowed_values_are_valid()
    }

    /// Checks that JSON container nullability belongs to an object or array.
    pub fn container_nullable_is_valid(&self) -> bool {
        !self.container_nullable || self.repeating || matches!(self.kind, SchemaKind::Group { .. })
    }

    pub fn json_object_item_nullable_is_valid(&self) -> bool {
        self.json_object_item_nullable.is_none()
            || (self.repeating
                && matches!(self.kind, SchemaKind::Group { .. })
                && !self.json_array_wrapper
                && !self.json_any)
    }

    pub fn json_object_item_nullability_tree_is_valid(&self) -> bool {
        self.walk_json_schema_nodes(Self::json_object_item_nullable_is_valid)
    }

    pub fn contains_json_object_item_nullability(&self) -> bool {
        !self.walk_json_schema_nodes(|node| node.json_object_item_nullable.is_none())
    }

    /// Nullability at a single JSON object occurrence, independent of its array.
    pub fn json_single_container_nullable(&self) -> bool {
        if self.repeating
            && matches!(self.kind, SchemaKind::Group { .. })
            && !self.json_array_wrapper
        {
            self.json_object_item_nullable
                .unwrap_or(self.container_nullable)
        } else {
            self.container_nullable
        }
    }

    /// The complete inner array schema of a validated distinct array frame.
    pub fn json_array_wrapper_item(&self) -> Option<&SchemaNode> {
        if !self.json_array_wrapper || !self.json_array_wrapper_is_valid() {
            return None;
        }
        let SchemaKind::Group { children, .. } = &self.kind else {
            return None;
        };
        children.first()
    }

    pub fn json_array_wrappers_tree_is_valid(&self) -> bool {
        self.walk_json_schema_nodes(|node| node.json_array_wrapper_is_valid())
    }

    /// Physical non-JSON boundaries must not reinterpret these group frames.
    pub fn contains_json_array_wrapper(&self) -> bool {
        !self.walk_json_schema_nodes(|node| !node.json_array_wrapper)
    }

    pub(crate) fn walk_json_schema_nodes(
        &self,
        mut accept: impl FnMut(&SchemaNode) -> bool,
    ) -> bool {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            if !accept(node) {
                return false;
            }
            if let SchemaKind::Group {
                children, dynamic, ..
            } = &node.kind
            {
                pending.extend(children.iter());
                if let Some(dynamic) = dynamic {
                    pending.push(dynamic);
                }
            }
            if let Some(contains) = &node.json_contains {
                pending.extend(
                    contains
                        .as_slice()
                        .iter()
                        .filter_map(|term| term.predicate().as_schema()),
                );
            }
            if let Some(dependent) = &node.json_dependent_schemas {
                pending.extend(
                    dependent
                        .as_slice()
                        .iter()
                        .filter_map(|term| term.predicate().as_schema()),
                );
            }
        }
        true
    }

    /// Checks that arbitrary JSON values use the graph's canonical string domain.
    pub fn json_any_is_valid(&self) -> bool {
        !self.json_any
            || (!self.attribute
                && !self.text
                && !self.nullable
                && self.fixed.is_none()
                && self.json_allowed_values.is_none()
                && self.numeric_range.is_none()
                && self.json_multiple_of.is_none()
                && self.string_length_range.is_none()
                && self.json_patterns.is_none()
                && self.json_formats.is_empty()
                && self.default.is_none()
                && self.value_generation.is_none()
                && matches!(
                    self.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                ))
    }
}
