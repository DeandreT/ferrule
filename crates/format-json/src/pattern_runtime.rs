use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};
use json_pattern::{DEFAULT_MATCH_WORK_LIMIT, PortableJsonPattern};

use crate::JsonFormatError;

pub(super) struct PatternRuntime {
    programs: BTreeMap<String, PortableJsonPattern>,
    remaining_work: u64,
}

impl PatternRuntime {
    pub(super) fn new(schema: &SchemaNode) -> Result<Self, JsonFormatError> {
        if !schema.json_pattern_budget_is_valid() {
            return Err(JsonFormatError::InvalidPatternMetadata {
                reason:
                    "schema-wide pattern metadata, program, or fixed-value work budget is invalid"
                        .to_string(),
            });
        }
        let mut programs = BTreeMap::new();
        collect_programs(schema, &mut programs)?;
        Ok(Self {
            programs,
            remaining_work: DEFAULT_MATCH_WORK_LIMIT,
        })
    }

    pub(super) fn validate_json(
        &mut self,
        schema: &SchemaNode,
        value: &serde_json::Value,
    ) -> Result<(), JsonFormatError> {
        let (Some(constraints), serde_json::Value::String(value)) = (&schema.json_patterns, value)
        else {
            return Ok(());
        };
        for alternative in constraints.any_of() {
            let mut matched = true;
            for source in alternative {
                let Some(program) = self.programs.get(source) else {
                    return Err(JsonFormatError::InvalidPatternMetadata {
                        reason: format!("compiled program for `{source}` is missing"),
                    });
                };
                match program.is_match_with_budget(value, &mut self.remaining_work) {
                    Ok(true) => {}
                    Ok(false) => {
                        matched = false;
                        break;
                    }
                    Err(_) => {
                        return Err(JsonFormatError::PatternWorkLimit {
                            name: schema.name.clone(),
                        });
                    }
                }
            }
            if matched {
                return Ok(());
            }
        }
        Err(JsonFormatError::PatternMismatch {
            name: schema.name.clone(),
        })
    }

    pub(super) fn validate_property_name(
        &mut self,
        schema: &SchemaNode,
        property: &str,
    ) -> Result<(), JsonFormatError> {
        let Some(constraints) = &schema.json_property_names else {
            return Ok(());
        };
        if matches!(constraints, ir::JsonPropertyNameConstraints::Never)
            || constraints
                .allowed()
                .is_some_and(|allowed| !allowed.contains(property))
            || constraints
                .length()
                .is_some_and(|length| !length.contains_str(property))
        {
            return Err(JsonFormatError::InvalidPropertyName {
                object: schema.name.clone(),
                property: property.to_string(),
            });
        }
        let Some(patterns) = constraints.patterns() else {
            return Ok(());
        };
        for alternative in patterns.any_of() {
            let mut matched = true;
            for source in alternative {
                let Some(program) = self.programs.get(source) else {
                    return Err(JsonFormatError::InvalidPatternMetadata {
                        reason: format!("compiled program for `{source}` is missing"),
                    });
                };
                match program.is_match_with_budget(property, &mut self.remaining_work) {
                    Ok(true) => {}
                    Ok(false) => {
                        matched = false;
                        break;
                    }
                    Err(_) => {
                        return Err(JsonFormatError::PatternWorkLimit {
                            name: schema.name.clone(),
                        });
                    }
                }
            }
            if matched {
                return Ok(());
            }
        }
        Err(JsonFormatError::InvalidPropertyName {
            object: schema.name.clone(),
            property: property.to_string(),
        })
    }
}

fn collect_programs(
    schema: &SchemaNode,
    programs: &mut BTreeMap<String, PortableJsonPattern>,
) -> Result<(), JsonFormatError> {
    if !schema.json_patterns_are_valid() {
        return Err(JsonFormatError::InvalidPatternMetadata {
            reason: format!(
                "pattern constraints on `{}` require a string-capable scalar domain",
                schema.name
            ),
        });
    }
    if let Some(constraints) = &schema.json_patterns {
        for alternative in constraints.any_of() {
            for source in alternative {
                if programs.contains_key(source) {
                    continue;
                }
                let program = PortableJsonPattern::compile(source).map_err(|error| {
                    JsonFormatError::InvalidPatternMetadata {
                        reason: error.to_string(),
                    }
                })?;
                programs.insert(source.clone(), program);
            }
        }
    }
    if let Some(constraints) = &schema.json_property_names
        && let Some(patterns) = constraints.patterns()
    {
        for source in patterns.any_of().iter().flatten() {
            if programs.contains_key(source) {
                continue;
            }
            let program = PortableJsonPattern::compile(source).map_err(|error| {
                JsonFormatError::InvalidPatternMetadata {
                    reason: error.to_string(),
                }
            })?;
            programs.insert(source.clone(), program);
        }
    }
    if let Some(constraints) = &schema.json_contains {
        for constraint in constraints.as_slice() {
            if let Some(predicate) = constraint.predicate().as_schema() {
                collect_programs(predicate, programs)?;
            }
        }
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Ok(());
    };
    for child in children {
        collect_programs(child, programs)?;
    }
    if let Some(dynamic) = schema.dynamic_fields() {
        collect_programs(dynamic, programs)?;
    }
    Ok(())
}
