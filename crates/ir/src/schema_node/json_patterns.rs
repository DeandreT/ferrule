//! Schema-wide JSON pattern compilation and matching budgets.

use crate::{
    JsonPatternConstraints, MAX_DISTINCT_JSON_PATTERNS, MAX_JSON_PATTERN_INSTRUCTIONS,
    MAX_JSON_PATTERN_SOURCE_BYTES, ScalarType, SchemaKind, SchemaNode,
};

impl SchemaNode {
    /// Checks that pattern constraints describe a string-capable scalar domain.
    pub fn json_patterns_are_valid(&self) -> bool {
        if self.json_patterns.is_none() {
            return true;
        }
        !self.json_any && self.accepts_scalar_type(ScalarType::String)
    }

    /// Checks one optional fixed lexical value against this node's pattern DNF.
    ///
    /// Whole-schema validation uses [`Self::json_pattern_budget_is_valid`] so
    /// fixed values across descendants share one work budget.
    pub fn json_pattern_fixed_value_is_valid(&self) -> bool {
        self.json_patterns_are_valid()
            && self.fixed.as_deref().is_none_or(|fixed| {
                self.json_patterns
                    .as_ref()
                    .is_none_or(|patterns| patterns.matches(fixed))
            })
    }

    /// Validates the aggregate pattern-program budget for one complete schema.
    ///
    /// Exact sources shared by multiple nodes count once. This is intentionally
    /// separate from [`Self::metadata_is_valid`] so callers can validate one
    /// root in linear time rather than rescanning descendants at every node.
    pub fn json_pattern_budget_is_valid(&self) -> bool {
        let mut programs = std::collections::BTreeMap::new();
        let mut source_bytes = 0_usize;
        let mut instructions = 0_usize;
        let mut fixed_work = json_pattern::DEFAULT_MATCH_WORK_LIMIT;
        self.accumulate_json_pattern_budget(
            &mut programs,
            &mut source_bytes,
            &mut instructions,
            &mut fixed_work,
        )
    }

    fn accumulate_json_pattern_budget<'a>(
        &'a self,
        programs: &mut std::collections::BTreeMap<&'a str, json_pattern::PortableJsonPattern>,
        source_bytes: &mut usize,
        instructions: &mut usize,
        fixed_work: &mut u64,
    ) -> bool {
        if !self.json_patterns_are_valid()
            || !self.json_property_names_are_structurally_valid()
            || (self.json_pattern_property_names.is_some()
                && self.json_pattern_property_name_expectations().is_none())
            || !self.json_contains_are_structurally_valid()
            || !self.json_dependent_schemas_are_structurally_valid()
        {
            return false;
        }
        if let Some(selectors) = &self.json_pattern_property_names
            && !accumulate_pattern_sources(
                selectors.patterns(),
                programs,
                source_bytes,
                instructions,
            )
        {
            return false;
        }
        if let Some(selectors) = &self.json_pattern_property_names {
            let Some(expectations) = self.json_pattern_property_name_expectations() else {
                return false;
            };
            for (name, expected) in expectations {
                if patterns_match_with_programs(selectors.patterns(), &name, programs, fixed_work)
                    != Some(expected)
                {
                    return false;
                }
            }
        }
        if let Some(constraints) = &self.json_property_names {
            let included_patterns = constraints.patterns();
            let excluded_patterns = constraints.excluded_patterns();
            for patterns in [included_patterns, excluded_patterns].into_iter().flatten() {
                if !accumulate_pattern_sources(patterns, programs, source_bytes, instructions) {
                    return false;
                }
            }
            if included_patterns.is_some() || excluded_patterns.is_some() {
                if let Some(allowed) = constraints.allowed() {
                    for name in allowed.as_slice() {
                        if included_patterns.is_some_and(|patterns| {
                            patterns_match_with_programs(patterns, name, programs, fixed_work)
                                != Some(true)
                        }) || excluded_patterns.is_some_and(|patterns| {
                            patterns_match_with_programs(patterns, name, programs, fixed_work)
                                != Some(false)
                        }) {
                            return false;
                        }
                    }
                }
                let Some(inputs) = self.json_property_name_pattern_inputs() else {
                    return false;
                };
                for input in inputs {
                    if included_patterns.is_some_and(|patterns| {
                        patterns_match_with_programs(patterns, &input, programs, fixed_work)
                            != Some(true)
                    }) || excluded_patterns.is_some_and(|patterns| {
                        patterns_match_with_programs(patterns, &input, programs, fixed_work)
                            != Some(false)
                    }) {
                        return false;
                    }
                }
            }
        }
        if let Some(patterns) = &self.json_patterns {
            if !accumulate_pattern_sources(patterns, programs, source_bytes, instructions) {
                return false;
            }
            if let Some(fixed) = self.fixed.as_deref()
                && patterns_match_with_programs(patterns, fixed, programs, fixed_work) != Some(true)
            {
                return false;
            }
        }

        let contains_are_valid = self.json_contains.as_ref().is_none_or(|constraints| {
            constraints.as_slice().iter().all(|constraint| {
                constraint.predicate().as_schema().is_none_or(|schema| {
                    schema.accumulate_json_pattern_budget(
                        programs,
                        source_bytes,
                        instructions,
                        fixed_work,
                    )
                })
            })
        });
        let dependent_schemas_are_valid =
            self.json_dependent_schemas
                .as_ref()
                .is_none_or(|constraints| {
                    constraints.as_slice().iter().all(|constraint| {
                        constraint.predicate().as_schema().is_none_or(|schema| {
                            schema.accumulate_json_pattern_budget(
                                programs,
                                source_bytes,
                                instructions,
                                fixed_work,
                            )
                        })
                    })
                });
        contains_are_valid
            && dependent_schemas_are_valid
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children.iter().all(|child| {
                        child.accumulate_json_pattern_budget(
                            programs,
                            source_bytes,
                            instructions,
                            fixed_work,
                        )
                    }) && dynamic.as_deref().is_none_or(|child| {
                        child.accumulate_json_pattern_budget(
                            programs,
                            source_bytes,
                            instructions,
                            fixed_work,
                        )
                    })
                }
            }
    }
}

fn accumulate_pattern_sources<'a>(
    patterns: &'a JsonPatternConstraints,
    programs: &mut std::collections::BTreeMap<&'a str, json_pattern::PortableJsonPattern>,
    source_bytes: &mut usize,
    instructions: &mut usize,
) -> bool {
    for source in patterns.any_of().iter().flatten().map(String::as_str) {
        if programs.contains_key(source) {
            continue;
        }
        if programs.len() == MAX_DISTINCT_JSON_PATTERNS {
            return false;
        }
        let Some(next_source_bytes) = source_bytes.checked_add(source.len()) else {
            return false;
        };
        if next_source_bytes > MAX_JSON_PATTERN_SOURCE_BYTES {
            return false;
        }
        let Ok(compiled) = json_pattern::PortableJsonPattern::compile(source) else {
            return false;
        };
        let Some(next_instructions) = instructions.checked_add(compiled.instruction_count()) else {
            return false;
        };
        if next_instructions > MAX_JSON_PATTERN_INSTRUCTIONS {
            return false;
        }
        programs.insert(source, compiled);
        *source_bytes = next_source_bytes;
        *instructions = next_instructions;
    }
    true
}

fn patterns_match_with_programs(
    patterns: &JsonPatternConstraints,
    value: &str,
    programs: &std::collections::BTreeMap<&str, json_pattern::PortableJsonPattern>,
    remaining_work: &mut u64,
) -> Option<bool> {
    for alternative in patterns.any_of() {
        let mut matched = true;
        for source in alternative {
            let program = programs.get(source.as_str())?;
            match program.is_match_with_budget(value, remaining_work) {
                Ok(true) => {}
                Ok(false) => {
                    matched = false;
                    break;
                }
                Err(_) => return None,
            }
        }
        if matched {
            return Some(true);
        }
    }
    Some(false)
}
