//! Ordered parameter selection. This does not acquire files or execute a body.

use crate::{JsonSourceLimits, JsonSourceValue, MapForceJsonString, Value};
use mapping::{JsonParameterInvocationPlan, JsonParameterIteration, JsonParameterPlanError};
use std::collections::HashSet;

pub const MAX_JSON_PARAMETER_INVOCATIONS: usize = 4096;
pub const MAX_JSON_PARAMETER_KEY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct JsonParameterInvocationLimits {
    pub invocations: usize,
    pub key_bytes: usize,
}

impl Default for JsonParameterInvocationLimits {
    fn default() -> Self {
        Self {
            invocations: MAX_JSON_PARAMETER_INVOCATIONS,
            key_bytes: MAX_JSON_PARAMETER_KEY_BYTES,
        }
    }
}

#[derive(Debug)]
pub enum JsonParameterInvocationError {
    Descriptor(serde_json::Error),
    DescriptorLimit,
    Plan(JsonParameterPlanError),
    InvalidLimits,
    Parse(crate::MapForceStringComponentError),
    InvocationLimit,
    KeyBytes,
}

impl std::fmt::Display for JsonParameterInvocationError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "JSON parameter invocation refused: {self:?}")
    }
}

impl std::error::Error for JsonParameterInvocationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Descriptor(error) => Some(error),
            Self::Plan(error) => Some(error),
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

pub fn parse_json_parameter_invocation_plan(
    descriptor: &str,
) -> Result<JsonParameterInvocationPlan, JsonParameterInvocationError> {
    if descriptor.len() > 1024 * 1024 {
        return Err(JsonParameterInvocationError::DescriptorLimit);
    }
    let plan: JsonParameterInvocationPlan =
        serde_json::from_str(descriptor).map_err(JsonParameterInvocationError::Descriptor)?;
    plan.validate()
        .map_err(JsonParameterInvocationError::Plan)?;
    Ok(plan)
}

#[derive(Debug)]
pub struct PreparedJsonParameterInvocations {
    source: MapForceJsonString,
    keys: Vec<String>,
    key_parameter: String,
}

#[derive(Debug, Clone, Copy)]
pub struct JsonParameterInvocation<'a> {
    key: &'a str,
    object: &'a JsonSourceValue,
    key_parameter: &'a str,
}

impl<'a> JsonParameterInvocation<'a> {
    pub fn key(self) -> &'a str {
        self.key
    }

    /// The complete original object, not a key-only or duplicate-stripped copy.
    pub fn parameter_object(self) -> &'a JsonSourceValue {
        self.object
    }

    pub fn key_binding(self) -> (&'a str, Value) {
        (self.key_parameter, Value::String(self.key.to_owned()))
    }
}

impl PreparedJsonParameterInvocations {
    /// Finish parsing and admission before the host can start physical I/O.
    pub fn prepare(
        plan: &JsonParameterInvocationPlan,
        input: &Value,
        json_limits: JsonSourceLimits,
        limits: JsonParameterInvocationLimits,
    ) -> Result<Self, JsonParameterInvocationError> {
        plan.validate()
            .map_err(JsonParameterInvocationError::Plan)?;
        if limits.invocations > MAX_JSON_PARAMETER_INVOCATIONS
            || limits.key_bytes > MAX_JSON_PARAMETER_KEY_BYTES
        {
            return Err(JsonParameterInvocationError::InvalidLimits);
        }
        let source = MapForceJsonString::parse(input, json_limits)
            .map_err(JsonParameterInvocationError::Parse)?;
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        let mut bytes = 0usize;
        for key in source.strings(&plan.key_member) {
            if plan.iteration == JsonParameterIteration::DistinctRootStringKeys && !seen.insert(key)
            {
                continue;
            }
            if keys.len() == limits.invocations {
                return Err(JsonParameterInvocationError::InvocationLimit);
            }
            bytes = bytes
                .checked_add(key.len())
                .filter(|bytes| *bytes <= limits.key_bytes)
                .ok_or(JsonParameterInvocationError::KeyBytes)?;
            keys.push(key.to_owned());
        }
        drop(seen);
        Ok(Self {
            source,
            keys,
            key_parameter: plan.key_parameter.clone(),
        })
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn invocations(&self) -> impl Iterator<Item = JsonParameterInvocation<'_>> {
        self.keys.iter().map(|key| JsonParameterInvocation {
            key,
            object: self.source.root(),
            key_parameter: &self.key_parameter,
        })
    }

    /// Preserve the host's first failure and previously completed effects.
    pub fn visit<E>(
        &self,
        mut invoke: impl FnMut(JsonParameterInvocation<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        for invocation in self.invocations() {
            invoke(invocation)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapping::{JsonSourceProperty, JsonSourceShape};

    fn plan(iteration: JsonParameterIteration) -> JsonParameterInvocationPlan {
        JsonParameterInvocationPlan {
            parameter: "Variables".into(),
            key_member: "key".into(),
            iteration,
            key_parameter: "__invocation_key".into(),
            projection: JsonSourceShape::Object {
                properties: vec![JsonSourceProperty {
                    name: "key".into(),
                    shape: JsonSourceShape::String,
                }],
            },
        }
    }

    fn prepare(iteration: JsonParameterIteration, text: &str) -> PreparedJsonParameterInvocations {
        PreparedJsonParameterInvocations::prepare(
            &plan(iteration),
            &Value::String(text.into()),
            Default::default(),
            Default::default(),
        )
        .unwrap()
    }

    #[test]
    fn occurrences_and_distinct_keys_keep_the_complete_original_object() {
        let text = r#"{"key":"A","key":17,"key":"A","key":"a","key":"","extra":{"n":1e1000}}"#;
        for (iteration, expected) in [
            (
                JsonParameterIteration::RootStringOccurrences,
                vec!["A", "A", "a", ""],
            ),
            (
                JsonParameterIteration::DistinctRootStringKeys,
                vec!["A", "a", ""],
            ),
        ] {
            let prepared = prepare(iteration, text);
            let items: Vec<_> = prepared.invocations().collect();
            assert_eq!(
                items.iter().map(|item| item.key()).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(prepared.len(), expected.len());
            for item in &items {
                assert!(std::ptr::eq(
                    item.parameter_object(),
                    items[0].parameter_object()
                ));
                assert_eq!(item.parameter_object().members_named("key").count(), 5);
                assert_eq!(item.parameter_object().members_named("extra").count(), 1);
                assert_eq!(
                    item.key_binding(),
                    ("__invocation_key", Value::String(item.key().into()))
                );
            }
        }
    }

    #[test]
    fn absent_and_wrong_typed_drivers_never_reach_the_host() {
        for input in [
            "null",
            "[]",
            "42",
            "true",
            r#""text""#,
            "{}",
            r#"{"key":null}"#,
            r#"{"key":1,"key":false,"key":[],"key":{}}"#,
        ] {
            let prepared = prepare(JsonParameterIteration::DistinctRootStringKeys, input);
            assert!(prepared.is_empty(), "{input}");
            prepared
                .visit::<()>(|_| panic!("absent driver reached host"))
                .unwrap();
        }
        let prepared = prepare(
            JsonParameterIteration::RootStringOccurrences,
            r#"{"key":null,"key":""}"#,
        );
        assert_eq!(prepared.invocations().next().unwrap().key(), "");
        assert!(
            PreparedJsonParameterInvocations::prepare(
                &plan(JsonParameterIteration::RootStringOccurrences),
                &Value::String(r#"{"key":"A","broken": }"#.into()),
                Default::default(),
                Default::default()
            )
            .is_err()
        );
    }

    #[test]
    fn limits_are_eager_and_count_utf8_keys_after_the_selected_policy() {
        let input = Value::String(r#"{"key":"é","key":"é"}"#.into());
        let limits = JsonParameterInvocationLimits {
            invocations: 1,
            key_bytes: 2,
        };
        let run = |iteration, limits| {
            PreparedJsonParameterInvocations::prepare(
                &plan(iteration),
                &input,
                Default::default(),
                limits,
            )
        };
        use JsonParameterIteration::{DistinctRootStringKeys, RootStringOccurrences};
        assert_eq!(run(DistinctRootStringKeys, limits).unwrap().len(), 1);
        assert!(matches!(
            run(RootStringOccurrences, limits),
            Err(JsonParameterInvocationError::InvocationLimit)
        ));
        assert!(matches!(
            run(
                DistinctRootStringKeys,
                JsonParameterInvocationLimits {
                    key_bytes: 1,
                    ..limits
                }
            ),
            Err(JsonParameterInvocationError::KeyBytes)
        ));
        let invalid = JsonParameterInvocationLimits {
            invocations: MAX_JSON_PARAMETER_INVOCATIONS + 1,
            ..limits
        };
        assert!(matches!(
            run(DistinctRootStringKeys, invalid),
            Err(JsonParameterInvocationError::InvalidLimits)
        ));
    }

    #[test]
    fn callback_failure_keeps_prior_effects_and_stops_later_invocations() {
        let prepared = prepare(
            JsonParameterIteration::DistinctRootStringKeys,
            r#"{"key":"A","key":"B","key":"C"}"#,
        );
        let mut visited = Vec::new();
        let mut accepted = Vec::new();
        let result = prepared.visit(|item| {
            visited.push(item.key().to_owned());
            if item.key() == "B" {
                return Err("second invocation failed");
            }
            accepted.push(item.key().to_owned());
            Ok(())
        });
        assert_eq!(result, Err("second invocation failed"));
        assert_eq!(visited, ["A", "B"]);
        assert_eq!(accepted, ["A"]);
        assert_eq!(prepared.len(), 3);
    }

    #[test]
    fn descriptors_are_strict_and_require_the_original_string_type() {
        let mut plan = plan(JsonParameterIteration::DistinctRootStringKeys);
        let text = serde_json::to_string(&plan).unwrap();
        assert_eq!(parse_json_parameter_invocation_plan(&text).unwrap(), plan);
        assert!(
            parse_json_parameter_invocation_plan(&text.replacen('{', "{\"unknown\":true,", 1))
                .is_err()
        );
        let JsonSourceShape::Object { properties } = &mut plan.projection else {
            unreachable!()
        };
        properties[0].shape = JsonSourceShape::Number;
        assert_eq!(
            plan.validate(),
            Err(JsonParameterPlanError::StringRootMember)
        );
        assert!(matches!(
            PreparedJsonParameterInvocations::prepare(
                &plan,
                &Value::String("invalid".into()),
                Default::default(),
                Default::default()
            ),
            Err(JsonParameterInvocationError::Plan(_))
        ));
    }
}
