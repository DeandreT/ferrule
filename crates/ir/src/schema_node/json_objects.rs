//! JSON object properties, dynamic fields and presence constraints.

use crate::{
    JsonPatternPropertyNames, JsonPropertyDependencies, JsonPropertyNameConstraints,
    PropertyCountRange, SchemaKind, SchemaNode,
};

impl SchemaNode {
    pub fn with_property_count_range(mut self, range: PropertyCountRange) -> Option<Self> {
        self.property_count_range = Some(range);
        (self.json_tuple_frame_is_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_property_names_are_valid())
        .then_some(self)
    }

    pub fn with_json_property_dependencies(
        mut self,
        dependencies: JsonPropertyDependencies,
    ) -> Option<Self> {
        self.set_json_property_dependencies(Some(dependencies))
            .then_some(self)
    }

    pub fn set_json_property_dependencies(
        &mut self,
        dependencies: Option<JsonPropertyDependencies>,
    ) -> bool {
        let previous = std::mem::replace(&mut self.json_property_dependencies, dependencies);
        if self.json_tuple_frame_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_pattern_property_names_are_valid()
            && self.property_count_range_is_valid()
            && self.json_property_names_are_valid()
            && (self.json_pattern_property_names.is_none() || self.json_pattern_budget_is_valid())
        {
            true
        } else {
            self.json_property_dependencies = previous;
            false
        }
    }

    pub fn with_json_pattern_property_names(
        mut self,
        selectors: JsonPatternPropertyNames,
    ) -> Option<Self> {
        self.set_json_pattern_property_names(Some(selectors))
            .then_some(self)
    }

    pub fn set_json_pattern_property_names(
        &mut self,
        selectors: Option<JsonPatternPropertyNames>,
    ) -> bool {
        let previous = std::mem::replace(&mut self.json_pattern_property_names, selectors);
        if self.json_tuple_frame_is_valid()
            && self.json_pattern_property_names_are_valid()
            && self.json_pattern_budget_is_valid()
        {
            true
        } else {
            self.json_pattern_property_names = previous;
            false
        }
    }

    pub fn with_json_property_names(
        mut self,
        constraints: JsonPropertyNameConstraints,
    ) -> Option<Self> {
        self.json_property_names = Some(constraints);
        (self.json_tuple_frame_is_valid()
            && self.json_property_names_are_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_pattern_budget_is_valid())
        .then_some(self)
    }

    /// Checks that property-count metadata remains attached to an object and
    /// is compatible with its required fields and closed property capacity.
    pub fn property_count_range_is_valid(&self) -> bool {
        let Some(range) = self.property_count_range else {
            return true;
        };
        let SchemaKind::Group {
            children,
            alternatives,
            required,
            dynamic,
            ..
        } = &self.kind
        else {
            return false;
        };
        let fits = |required: &[String], declared: Option<&[String]>| {
            let required_count =
                effective_required_fields(required, self.json_property_dependencies.as_ref()).len();
            let required_fits = u64::try_from(required_count).map_or_else(
                |_| range.maximum().is_none(),
                |required_count| {
                    range
                        .maximum()
                        .is_none_or(|maximum| required_count <= maximum)
                },
            );
            if !required_fits {
                return false;
            }
            self.json_property_capacity(declared)
                .is_none_or(|capacity| {
                    u64::try_from(capacity).map_or(true, |capacity| range.minimum() <= capacity)
                })
        };
        let child_names = children
            .iter()
            .map(|child| child.name.clone())
            .collect::<Vec<_>>();
        if !fits(
            required,
            dynamic.is_none().then_some(child_names.as_slice()),
        ) {
            return false;
        }
        alternatives.iter().all(|alternative| {
            let mut required = required.clone();
            for field in &alternative.required {
                if !required.contains(field) {
                    required.push(field.clone());
                }
            }
            fits(&required, Some(&alternative.members))
        })
    }

    /// Recursively checks JSON object property-count placement and
    /// feasibility for one schema tree.
    pub fn property_count_range_tree_is_valid(&self) -> bool {
        self.property_count_range_is_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::property_count_range_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::property_count_range_tree_is_valid)
                }
            }
    }

    /// Checks that JSON property dependencies belong to an object and that
    /// every unconditionally implied property remains possible.
    pub fn json_property_dependencies_are_valid(&self) -> bool {
        let Some(dependencies) = &self.json_property_dependencies else {
            return true;
        };
        let SchemaKind::Group {
            children,
            alternatives,
            required,
            dynamic,
            ..
        } = &self.kind
        else {
            return false;
        };
        let ordinary_allowed = dynamic.is_none().then(|| {
            children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<std::collections::BTreeSet<_>>()
        });
        let ordinary_required = effective_required_fields(required, Some(dependencies));
        if ordinary_allowed.as_ref().is_some_and(|allowed| {
            ordinary_required
                .iter()
                .any(|required| !allowed.contains(required.as_str()))
        }) {
            return false;
        }
        alternatives.iter().all(|alternative| {
            let mut initial = required.clone();
            for field in &alternative.required {
                if !initial.contains(field) {
                    initial.push(field.clone());
                }
            }
            let effective = effective_required_fields(&initial, Some(dependencies));
            effective
                .iter()
                .all(|required| alternative.members.contains(required))
        })
    }

    /// Recursively checks JSON property-dependency placement and feasibility.
    pub fn json_property_dependencies_tree_is_valid(&self) -> bool {
        self.json_property_dependencies_are_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_property_dependencies_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_property_dependencies_tree_is_valid)
                }
            }
    }

    /// Checks that dynamic JSON property-name selectors belong to one open
    /// object, admit every runtime-named property referenced by ordinary
    /// required or dependency metadata, and agree with every statically
    /// declared child whose name also matches a selector.
    pub fn json_pattern_property_names_are_valid(&self) -> bool {
        let Some(selectors) = &self.json_pattern_property_names else {
            return true;
        };
        let Some(expectations) = self.json_pattern_property_name_expectations() else {
            return false;
        };
        selectors.matches_expected(
            expectations
                .iter()
                .map(|(name, expected)| (name.as_str(), *expected)),
        )
    }

    pub(super) fn json_pattern_property_name_expectations(
        &self,
    ) -> Option<std::collections::BTreeMap<String, bool>> {
        self.json_pattern_property_names.as_ref()?;
        let SchemaKind::Group {
            children,
            alternatives,
            required,
            dynamic,
            ..
        } = &self.kind
        else {
            return None;
        };
        let dynamic = dynamic.as_deref()?;
        if !alternatives.is_empty() {
            return None;
        }
        let mut expectations = std::collections::BTreeMap::new();
        for child in children {
            // An authored undeclared property exists only because a selector
            // admits it under the dynamic value schema.
            if child.json_undeclared_property {
                expectations.insert(child.name.clone(), true);
            } else if !same_schema_ignoring_name(child, dynamic) {
                expectations.insert(child.name.clone(), false);
            }
        }
        let mut require_selected = |name: &str| {
            if !children.iter().any(|child| child.name == name) {
                expectations.insert(name.to_string(), true);
            }
        };
        for required in required {
            require_selected(required);
        }
        if let Some(dependencies) = &self.json_property_dependencies {
            for (trigger, requirements) in dependencies.rules() {
                require_selected(trigger);
                for required in requirements {
                    require_selected(required);
                }
            }
        }
        Some(expectations)
    }

    /// Recursively checks dynamic JSON pattern-property selector placement
    /// and all referenced runtime property names.
    pub fn json_pattern_property_names_tree_is_valid(&self) -> bool {
        self.json_pattern_property_names_are_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_pattern_property_names_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_pattern_property_names_tree_is_valid)
                }
            }
    }

    /// Checks that JSON property-name constraints belong to an object and
    /// admit every property that is unconditionally required.
    pub fn json_property_names_are_valid(&self) -> bool {
        let Some(constraints) = &self.json_property_names else {
            return true;
        };
        let SchemaKind::Group {
            alternatives,
            required,
            ..
        } = &self.kind
        else {
            return false;
        };
        if !constraints.is_canonical() {
            return false;
        }
        let required =
            effective_required_fields(required, self.json_property_dependencies.as_ref());
        if required
            .iter()
            .any(|required| !constraints.accepts(required))
        {
            return false;
        }
        alternatives.iter().all(|alternative| {
            let mut initial = required.clone();
            for field in &alternative.required {
                if !initial.contains(field) {
                    initial.push(field.clone());
                }
            }
            let effective =
                effective_required_fields(&initial, self.json_property_dependencies.as_ref());
            effective
                .iter()
                .all(|required| constraints.accepts(required))
        })
    }

    pub(super) fn json_property_names_are_structurally_valid(&self) -> bool {
        let Some(constraints) = &self.json_property_names else {
            return true;
        };
        let SchemaKind::Group {
            alternatives,
            required,
            ..
        } = &self.kind
        else {
            return false;
        };
        if !constraints.is_structurally_canonical() {
            return false;
        }
        let required =
            effective_required_fields(required, self.json_property_dependencies.as_ref());
        if required
            .iter()
            .any(|required| !constraints.accepts_without_patterns(required))
        {
            return false;
        }
        alternatives.iter().all(|alternative| {
            let mut initial = required.clone();
            for field in &alternative.required {
                if !initial.contains(field) {
                    initial.push(field.clone());
                }
            }
            let effective =
                effective_required_fields(&initial, self.json_property_dependencies.as_ref());
            effective
                .iter()
                .all(|required| constraints.accepts_without_patterns(required))
        })
    }

    pub(super) fn json_property_name_pattern_inputs(
        &self,
    ) -> Option<std::collections::BTreeSet<String>> {
        let constraints = self.json_property_names.as_ref()?;
        if constraints.patterns().is_none() && constraints.excluded_patterns().is_none() {
            return None;
        }
        let SchemaKind::Group {
            alternatives,
            required,
            ..
        } = &self.kind
        else {
            return None;
        };
        let required =
            effective_required_fields(required, self.json_property_dependencies.as_ref());
        let mut inputs = required
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        for alternative in alternatives {
            let mut initial = required.clone();
            for field in &alternative.required {
                if !initial.contains(field) {
                    initial.push(field.clone());
                }
            }
            inputs.extend(effective_required_fields(
                &initial,
                self.json_property_dependencies.as_ref(),
            ));
        }
        Some(inputs)
    }

    /// Recursively checks JSON property-name placement and feasibility.
    pub fn json_property_names_tree_is_valid(&self) -> bool {
        self.json_property_names_are_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_property_names_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_property_names_tree_is_valid)
                }
            }
    }

    fn json_property_capacity(&self, declared: Option<&[String]>) -> Option<usize> {
        let Some(constraints) = &self.json_property_names else {
            return declared.map(<[String]>::len);
        };
        match constraints.finite_capacity() {
            Some(0) => Some(0),
            Some(_) => constraints.allowed().map(|allowed| {
                declared.map_or_else(
                    || allowed.as_slice().len(),
                    |declared| {
                        declared
                            .iter()
                            .filter(|name| allowed.contains(name))
                            .count()
                    },
                )
            }),
            None => declared.map(|declared| {
                declared
                    .iter()
                    .filter(|name| constraints.accepts(name))
                    .count()
            }),
        }
    }

    /// Declares a homogeneous computed-field value schema for this group.
    /// Object alternatives and open fields are intentionally exclusive: an
    /// open object cannot be matched to one closed alternative exactly.
    pub fn with_dynamic_fields(mut self, value: SchemaNode) -> Option<Self> {
        self.set_dynamic_fields(Some(value)).then_some(self)
    }

    pub fn set_dynamic_fields(&mut self, value: Option<SchemaNode>) -> bool {
        let previous = {
            let SchemaKind::Group {
                children,
                alternatives,
                required,
                dynamic,
                ..
            } = &mut self.kind
            else {
                return false;
            };
            if value.is_some() && !alternatives.is_empty() {
                return false;
            }
            if value.is_none()
                && required
                    .iter()
                    .any(|name| !children.iter().any(|child| child.name == *name))
            {
                return false;
            }
            std::mem::replace(dynamic, value.map(Box::new))
        };
        if self.json_tuple_frame_is_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_pattern_property_names_are_valid()
            && self.json_property_names_are_valid()
            && (self.json_pattern_property_names.is_none() || self.json_pattern_budget_is_valid())
        {
            true
        } else {
            let SchemaKind::Group { dynamic, .. } = &mut self.kind else {
                return false;
            };
            *dynamic = previous;
            false
        }
    }

    pub fn dynamic_fields(&self) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { dynamic, .. } => dynamic.as_deref(),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
        }
    }

    /// Attaches exact JSON object-property presence requirements.
    pub fn with_required_fields(mut self, required: Vec<String>) -> Option<Self> {
        self.set_required_fields(required).then_some(self)
    }

    pub fn set_required_fields(&mut self, required: Vec<String>) -> bool {
        let previous = {
            let SchemaKind::Group {
                children,
                required: target,
                dynamic,
                ..
            } = &mut self.kind
            else {
                return false;
            };
            if !valid_required_fields(children, dynamic.as_deref(), &required) {
                return false;
            }
            std::mem::replace(target, required)
        };
        if self.json_tuple_frame_is_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_pattern_property_names_are_valid()
            && self.json_property_names_are_valid()
            && (self.json_pattern_property_names.is_none() || self.json_pattern_budget_is_valid())
        {
            true
        } else {
            let SchemaKind::Group {
                required: target, ..
            } = &mut self.kind
            else {
                return false;
            };
            *target = previous;
            false
        }
    }

    pub fn required_fields(&self) -> &[String] {
        match &self.kind {
            SchemaKind::Group { required, .. } => required,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => &[],
        }
    }

    /// Checks ordinary JSON object-property presence metadata.
    pub fn required_fields_are_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group {
                children,
                required,
                dynamic,
                ..
            } => valid_required_fields(children, dynamic.as_deref(), required),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
        }
    }

    pub fn json_pattern_property_names(&self) -> Option<&JsonPatternPropertyNames> {
        self.json_pattern_property_names.as_ref()
    }
}

fn same_schema_ignoring_name(left: &SchemaNode, right: &SchemaNode) -> bool {
    if left.name == right.name {
        return left == right;
    }
    let mut left = left.clone();
    let mut right = right.clone();
    left.name.clear();
    right.name.clear();
    left == right
}

fn valid_required_fields(
    children: &[SchemaNode],
    dynamic: Option<&SchemaNode>,
    required: &[String],
) -> bool {
    required.iter().enumerate().all(|(index, name)| {
        !name.is_empty()
            && !required[..index].contains(name)
            && (dynamic.is_some() || children.iter().any(|child| child.name == *name))
    })
}

fn effective_required_fields(
    required: &[String],
    dependencies: Option<&JsonPropertyDependencies>,
) -> Vec<String> {
    let mut effective = required
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let Some(dependencies) = dependencies else {
        return effective.into_iter().collect();
    };
    loop {
        let previous_len = effective.len();
        for (trigger, requirements) in dependencies.rules() {
            if effective.contains(trigger) {
                effective.extend(requirements.iter().cloned());
            }
        }
        if effective.len() == previous_len {
            return effective.into_iter().collect();
        }
    }
}
