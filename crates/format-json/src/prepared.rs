use ir::{Instance, SchemaNode};

use crate::runtime_constraints::{self, PatternPrograms};
use crate::{JsonFormatError, RecursiveSchemas};

/// A schema whose pattern programs are compiled once and shared by every
/// document read or written with it.
///
/// The prepared state is immutable, so one value can serve repeated and
/// concurrent calls. Each call starts its own pattern work budget, exactly like
/// [`crate::from_str`] and [`crate::to_value`].
pub struct PreparedSchema {
    schema: SchemaNode,
    patterns: Option<PatternPrograms>,
}

impl PreparedSchema {
    /// Prepares `schema`. Preparation never fails: a schema the reader or
    /// writer would reject keeps no prepared state, and each call then takes
    /// the unprepared path, so its error and its precedence over document
    /// errors stay those of [`crate::from_str`] and [`crate::to_value`].
    pub fn new(schema: SchemaNode) -> Self {
        let patterns = RecursiveSchemas::new(&schema)
            .and_then(|_| PatternPrograms::new(&schema))
            .ok();
        Self { schema, patterns }
    }

    pub fn schema(&self) -> &SchemaNode {
        &self.schema
    }

    /// Equivalent to [`crate::from_str`] with this schema.
    pub fn from_str(&self, text: &str) -> Result<Instance, JsonFormatError> {
        let Some(programs) = &self.patterns else {
            return crate::from_str(text, &self.schema);
        };
        let text = crate::strip_utf8_bom(text);
        runtime_constraints::validate_raw_json_unique_items(&self.schema, text)?;
        let value: serde_json::Value = serde_json::from_str(text)?;
        // The anchor view borrows the schema, so it is rebuilt per call; its
        // validation already succeeded during preparation.
        let recursive = RecursiveSchemas::new(&self.schema)?;
        crate::from_value_with(&value, &self.schema, &recursive, &mut programs.runtime())
    }

    /// Equivalent to [`crate::to_value`] with this schema.
    pub fn to_value(&self, instance: &Instance) -> Result<serde_json::Value, JsonFormatError> {
        let Some(programs) = &self.patterns else {
            return crate::to_value(&self.schema, instance);
        };
        let recursive = RecursiveSchemas::new(&self.schema)?;
        crate::to_value_with(&self.schema, instance, &recursive, &mut programs.runtime())
    }
}

#[cfg(test)]
mod tests {
    use ir::{JsonPatternConstraints, ScalarType, Value};

    use super::*;

    fn patterned(pattern: &str) -> SchemaNode {
        let Some(schema) = JsonPatternConstraints::new([[pattern]])
            .ok()
            .and_then(|patterns| {
                SchemaNode::scalar("Value", ScalarType::String).with_json_patterns(patterns)
            })
        else {
            panic!("test pattern is valid for its string field");
        };
        schema.repeating()
    }

    fn assert_matches(schema: SchemaNode, documents: &[&str], instances: &[Instance]) {
        let prepared = PreparedSchema::new(schema.clone());
        for document in documents {
            for _ in 0..2 {
                assert_eq!(
                    prepared
                        .from_str(document)
                        .map_err(|error| error.to_string()),
                    crate::from_str(document, &schema).map_err(|error| error.to_string()),
                    "{document:.40}"
                );
            }
        }
        for instance in instances {
            assert_eq!(
                prepared
                    .to_value(instance)
                    .map_err(|error| error.to_string()),
                crate::to_value(&schema, instance).map_err(|error| error.to_string())
            );
        }
    }

    #[test]
    fn prepared_schema_matches_unprepared_reads_and_writes() {
        // Each value is charged three fifths of one document's work budget.
        let pattern = "^(b{1000}|a*)$";
        let Ok(program) = json_pattern::PortableJsonPattern::compile(pattern) else {
            panic!("test pattern compiles");
        };
        let long = "a".repeat(
            json_pattern::DEFAULT_MATCH_WORK_LIMIT as usize / program.instruction_count() * 3 / 5,
        );
        let values = |count| serde_json::to_string(&vec![&long; count]).unwrap_or_default();
        assert_matches(
            patterned(pattern),
            &[&values(1), &values(2), r#"["b"]"#, "[", "\u{feff}[]"],
            &[
                Instance::Repeated(vec![Instance::Scalar(Value::String("a".into()))]),
                Instance::Repeated(vec![Instance::Scalar(Value::String("b".into()))]),
            ],
        );
        let prepared = PreparedSchema::new(patterned(pattern));
        // Repeated calls each start a fresh budget; one document cannot fit two.
        assert!(prepared.from_str(&values(1)).is_ok());
        assert!(prepared.from_str(&values(1)).is_ok());
        assert!(matches!(
            prepared.from_str(&values(2)),
            Err(JsonFormatError::PatternWorkLimit { .. })
        ));

        // A schema the reader rejects keeps no prepared state: syntax errors
        // still precede its metadata error.
        let mut missing_anchor =
            SchemaNode::group("Node", vec![SchemaNode::group("Child", vec![])]);
        if let ir::SchemaKind::Group { children, .. } = &mut missing_anchor.kind {
            children[0].recursive_ref = Some("Missing".into());
        }
        assert!(
            PreparedSchema::new(missing_anchor.clone())
                .patterns
                .is_none()
        );
        assert_matches(
            missing_anchor,
            &["{", r#"{"Child":{}}"#],
            &[Instance::Group(Vec::new().into())],
        );
    }
}
