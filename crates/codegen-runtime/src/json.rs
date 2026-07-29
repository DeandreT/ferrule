use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;

use ir::{Instance, SchemaKind, SchemaNode, Value};
use json_pattern::{DEFAULT_MATCH_WORK_LIMIT, PortableJsonPattern};

use crate::RuntimeError;

mod allowed_values;
#[cfg(test)]
mod contains;
#[cfg(test)]
mod dependent_schemas;
mod multiple_of;
#[cfg(test)]
mod property_names;
mod unique_items;

pub const MAX_EMBEDDED_JSON_SCHEMA_BYTES: usize = 1024 * 1024;
pub const MAX_JSON_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// Structured failure from a generated mapping's JSON document boundary.
#[derive(Debug, PartialEq)]
pub enum JsonBoundaryError {
    EmbeddedSchemaTooLarge { bytes: usize, max: usize },
    InvalidEmbeddedSchema { message: String },
    InputTooLarge { bytes: usize, max: usize },
    InvalidInput { message: String },
    Execution(RuntimeError),
    InvalidOutput { message: String },
    OutputTooLarge { bytes: usize, max: usize },
}

impl fmt::Display for JsonBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmbeddedSchemaTooLarge { bytes, max } => write!(
                formatter,
                "embedded JSON schema is {bytes} bytes; maximum is {max}"
            ),
            Self::InvalidEmbeddedSchema { message } => {
                write!(formatter, "embedded JSON schema is invalid: {message}")
            }
            Self::InputTooLarge { bytes, max } => {
                write!(formatter, "JSON input is {bytes} bytes; maximum is {max}")
            }
            Self::InvalidInput { message } => write!(formatter, "JSON input is invalid: {message}"),
            Self::Execution(error) => error.fmt(formatter),
            Self::InvalidOutput { message } => {
                write!(formatter, "JSON output is invalid: {message}")
            }
            Self::OutputTooLarge { bytes, max } => {
                write!(formatter, "JSON output is {bytes} bytes; maximum is {max}")
            }
        }
    }
}

impl std::error::Error for JsonBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Execution(error) => Some(error),
            Self::EmbeddedSchemaTooLarge { .. }
            | Self::InvalidEmbeddedSchema { .. }
            | Self::InputTooLarge { .. }
            | Self::InvalidInput { .. }
            | Self::InvalidOutput { .. }
            | Self::OutputTooLarge { .. } => None,
        }
    }
}

impl From<RuntimeError> for JsonBoundaryError {
    fn from(error: RuntimeError) -> Self {
        Self::Execution(error)
    }
}

/// Per-document portable-constraint state and deterministic pattern work budget.
#[derive(Debug)]
struct ConstraintBoundary {
    patterns: RefCell<BTreeMap<String, PortableJsonPattern>>,
    remaining_pattern_work: Cell<u64>,
}

impl ConstraintBoundary {
    fn new() -> Self {
        Self {
            patterns: RefCell::new(BTreeMap::new()),
            remaining_pattern_work: Cell::new(DEFAULT_MATCH_WORK_LIMIT),
        }
    }

    #[cfg(test)]
    fn with_pattern_work_limit(limit: u64) -> Self {
        Self {
            patterns: RefCell::new(BTreeMap::new()),
            remaining_pattern_work: Cell::new(limit),
        }
    }

    /// Parses one bounded JSON document and applies embedded constraints
    /// against the normalized typed instance.
    fn parse_json(&self, schema: &str, document: &str) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        let schema = self.parse_schema(schema)?;
        if unique_items::schema_contains_assertion(&schema) {
            format_json::json_schema::unique_items::validate_raw_json_unique_items(
                &schema, document,
            )
            .map_err(|error| JsonBoundaryError::InvalidInput {
                message: error.to_string(),
            })?;
        }
        let formatter_schema = without_boundary_constraints(schema.clone());
        let instance = format_json::from_str(document, &formatter_schema).map_err(|error| {
            JsonBoundaryError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        self.validate_input(&schema, &instance)?;
        Ok(instance)
    }

    /// Parses one bounded UTF-8 JSON payload.
    fn parse_json_bytes(
        &self,
        schema: &str,
        document: &[u8],
    ) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        let document =
            std::str::from_utf8(document).map_err(|error| JsonBoundaryError::InvalidInput {
                message: format!("document is not UTF-8: {error}"),
            })?;
        self.parse_json(schema, document)
    }

    /// Serializes one instance and applies embedded constraints after all
    /// JSON scalar normalization and coercion.
    fn serialize_json(
        &self,
        schema: &str,
        instance: &Instance,
    ) -> Result<String, JsonBoundaryError> {
        let schema = self.parse_schema(schema)?;
        let formatter_schema = without_boundary_constraints(schema.clone());
        let document = format_json::to_string(&formatter_schema, instance).map_err(|error| {
            JsonBoundaryError::InvalidOutput {
                message: error.to_string(),
            }
        })?;
        if document.len() > MAX_JSON_DOCUMENT_BYTES {
            return Err(JsonBoundaryError::OutputTooLarge {
                bytes: document.len(),
                max: MAX_JSON_DOCUMENT_BYTES,
            });
        }
        let value =
            serde_json::from_str(&document).map_err(|error| JsonBoundaryError::InvalidOutput {
                message: error.to_string(),
            })?;
        self.validate_output(&schema, &value)?;
        Ok(document)
    }

    /// Serializes one instance as a bounded UTF-8 JSON payload.
    fn serialize_json_bytes(
        &self,
        schema: &str,
        instance: &Instance,
    ) -> Result<Vec<u8>, JsonBoundaryError> {
        self.serialize_json(schema, instance)
            .map(String::into_bytes)
    }

    fn parse_schema(&self, schema: &str) -> Result<SchemaNode, JsonBoundaryError> {
        let schema = parse_schema(schema)?;
        if !schema.json_pattern_budget_is_valid() {
            return Err(JsonBoundaryError::InvalidEmbeddedSchema {
                message:
                    "schema-wide JSON pattern metadata, program, or fixed-value work budget is invalid"
                        .to_string(),
            });
        }
        register_patterns(&schema, &mut self.patterns.borrow_mut())?;
        Ok(schema)
    }

    fn validate_input(
        &self,
        schema: &SchemaNode,
        instance: &Instance,
    ) -> Result<(), JsonBoundaryError> {
        let mut remaining = self.remaining_pattern_work.get();
        let result =
            validate_instance_document(schema, instance, &self.patterns.borrow(), &mut remaining);
        self.remaining_pattern_work.set(remaining);
        result.map_err(constraint_input_error)
    }

    fn validate_output(
        &self,
        schema: &SchemaNode,
        value: &serde_json::Value,
    ) -> Result<(), JsonBoundaryError> {
        unique_items::validate_document(schema, value).map_err(unique_items::output_error)?;
        let mut remaining = self.remaining_pattern_work.get();
        let result = validate_json_document(schema, value, &self.patterns.borrow(), &mut remaining);
        self.remaining_pattern_work.set(remaining);
        result.map_err(constraint_output_error)
    }
}

/// Parses one bounded JSON document using an emitter-owned schema.
pub fn parse_json(schema: &str, document: &str) -> Result<Instance, JsonBoundaryError> {
    ConstraintBoundary::new().parse_json(schema, document)
}

/// Parses one bounded UTF-8 JSON payload using an emitter-owned schema.
pub fn parse_json_bytes(schema: &str, document: &[u8]) -> Result<Instance, JsonBoundaryError> {
    ConstraintBoundary::new().parse_json_bytes(schema, document)
}

fn check_input_size(bytes: usize) -> Result<(), JsonBoundaryError> {
    if bytes > MAX_JSON_DOCUMENT_BYTES {
        return Err(JsonBoundaryError::InputTooLarge {
            bytes,
            max: MAX_JSON_DOCUMENT_BYTES,
        });
    }
    Ok(())
}

/// Serializes one instance as a bounded pretty-printed JSON document.
pub fn serialize_json(schema: &str, instance: &Instance) -> Result<String, JsonBoundaryError> {
    ConstraintBoundary::new().serialize_json(schema, instance)
}

/// Serializes one instance as a bounded pretty-printed UTF-8 JSON payload.
pub fn serialize_json_bytes(
    schema: &str,
    instance: &Instance,
) -> Result<Vec<u8>, JsonBoundaryError> {
    ConstraintBoundary::new().serialize_json_bytes(schema, instance)
}

fn parse_schema(schema: &str) -> Result<SchemaNode, JsonBoundaryError> {
    if schema.len() > MAX_EMBEDDED_JSON_SCHEMA_BYTES {
        return Err(JsonBoundaryError::EmbeddedSchemaTooLarge {
            bytes: schema.len(),
            max: MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        });
    }
    serde_json::from_str(schema).map_err(|error| JsonBoundaryError::InvalidEmbeddedSchema {
        message: error.to_string(),
    })
}

fn register_patterns(
    schema: &SchemaNode,
    programs: &mut BTreeMap<String, PortableJsonPattern>,
) -> Result<(), JsonBoundaryError> {
    if !schema.json_patterns_are_valid() {
        return Err(JsonBoundaryError::InvalidEmbeddedSchema {
            message: format!(
                "JSON pattern metadata on schema node `{}` is invalid",
                schema.name
            ),
        });
    }
    if let Some(constraints) = &schema.json_patterns {
        for source in constraints.any_of().iter().flatten() {
            if programs.contains_key(source) {
                continue;
            }
            let program = PortableJsonPattern::compile(source).map_err(|error| {
                JsonBoundaryError::InvalidEmbeddedSchema {
                    message: error.to_string(),
                }
            })?;
            programs.insert(source.clone(), program);
        }
    }
    if let SchemaKind::Group {
        children, dynamic, ..
    } = &schema.kind
    {
        for child in children {
            register_patterns(child, programs)?;
        }
        if let Some(dynamic) = dynamic {
            register_patterns(dynamic, programs)?;
        }
    }
    Ok(())
}

fn without_boundary_constraints(mut schema: SchemaNode) -> SchemaNode {
    clear_boundary_constraints(&mut schema);
    schema
}

fn clear_boundary_constraints(schema: &mut SchemaNode) {
    schema.json_allowed_values = None;
    schema.json_multiple_of = None;
    schema.json_unique_items = false;
    if let SchemaKind::Group {
        children, dynamic, ..
    } = &mut schema.kind
    {
        for child in children {
            clear_boundary_constraints(child);
        }
        if let Some(dynamic) = dynamic {
            clear_boundary_constraints(dynamic);
        }
    }
}

#[derive(Debug)]
enum ConstraintValidationError {
    AllowedValueMismatch { name: String },
    MissingPatternProgram { source: String },
    PatternMismatch { name: String },
    MultipleOfMismatch { name: String },
    PatternWorkLimit { name: String },
}

fn constraint_input_error(error: ConstraintValidationError) -> JsonBoundaryError {
    match error {
        ConstraintValidationError::AllowedValueMismatch { name } => {
            JsonBoundaryError::InvalidInput {
                message: format!("`{name}` is not one of its JSON Schema allowed values"),
            }
        }
        ConstraintValidationError::MissingPatternProgram { source } => {
            JsonBoundaryError::InvalidEmbeddedSchema {
                message: format!("compiled JSON pattern `{source}` is missing"),
            }
        }
        ConstraintValidationError::PatternMismatch { name } => JsonBoundaryError::InvalidInput {
            message: format!("`{name}` does not match its JSON Schema pattern constraints"),
        },
        ConstraintValidationError::MultipleOfMismatch { name } => JsonBoundaryError::InvalidInput {
            message: format!(
                "`{name}` is not an exact multiple of its JSON Schema multipleOf constraints"
            ),
        },
        ConstraintValidationError::PatternWorkLimit { name } => JsonBoundaryError::InvalidInput {
            message: format!("JSON pattern matching for `{name}` exceeds the bounded work limit"),
        },
    }
}

fn constraint_output_error(error: ConstraintValidationError) -> JsonBoundaryError {
    match error {
        ConstraintValidationError::AllowedValueMismatch { name } => {
            JsonBoundaryError::InvalidOutput {
                message: format!("`{name}` is not one of its JSON Schema allowed values"),
            }
        }
        ConstraintValidationError::MissingPatternProgram { source } => {
            JsonBoundaryError::InvalidEmbeddedSchema {
                message: format!("compiled JSON pattern `{source}` is missing"),
            }
        }
        ConstraintValidationError::PatternMismatch { name } => JsonBoundaryError::InvalidOutput {
            message: format!("`{name}` does not match its JSON Schema pattern constraints"),
        },
        ConstraintValidationError::MultipleOfMismatch { name } => {
            JsonBoundaryError::InvalidOutput {
                message: format!(
                    "`{name}` is not an exact multiple of its JSON Schema multipleOf constraints"
                ),
            }
        }
        ConstraintValidationError::PatternWorkLimit { name } => JsonBoundaryError::InvalidOutput {
            message: format!("JSON pattern matching for `{name}` exceeds the bounded work limit"),
        },
    }
}

fn validate_instance_document(
    schema: &SchemaNode,
    instance: &Instance,
    programs: &BTreeMap<String, PortableJsonPattern>,
    remaining_work: &mut u64,
) -> Result<(), ConstraintValidationError> {
    if schema.repeating {
        if let Instance::Repeated(items) = instance {
            for item in items {
                validate_instance_node(schema, item, programs, remaining_work)?;
            }
        }
        return Ok(());
    }
    validate_instance_node(schema, instance, programs, remaining_work)
}

fn validate_instance_node(
    schema: &SchemaNode,
    instance: &Instance,
    programs: &BTreeMap<String, PortableJsonPattern>,
    remaining_work: &mut u64,
) -> Result<(), ConstraintValidationError> {
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }, Instance::Scalar(value)) => {
            if let Value::String(value) = value {
                validate_pattern_value(schema, value, programs, remaining_work)?;
            }
            multiple_of::validate_instance_value(schema, value)?;
            allowed_values::validate_instance_value(schema, value)
        }
        (
            SchemaKind::Group {
                children, dynamic, ..
            },
            Instance::Group(fields),
        ) => {
            for (name, value) in fields {
                let child = children
                    .iter()
                    .find(|child| child.name == *name)
                    .or(dynamic.as_deref());
                if let Some(child) = child {
                    validate_instance_document(child, value, programs, remaining_work)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_json_document(
    schema: &SchemaNode,
    value: &serde_json::Value,
    programs: &BTreeMap<String, PortableJsonPattern>,
    remaining_work: &mut u64,
) -> Result<(), ConstraintValidationError> {
    if schema.repeating {
        if let serde_json::Value::Array(items) = value {
            for item in items {
                validate_json_node(schema, item, programs, remaining_work)?;
            }
        }
        return Ok(());
    }
    // Generated root scopes may produce flat rows against a non-repeating row
    // schema. Every serialized row still shares the one output budget.
    if let serde_json::Value::Array(items) = value {
        for item in items {
            validate_json_node(schema, item, programs, remaining_work)?;
        }
        return Ok(());
    }
    validate_json_node(schema, value, programs, remaining_work)
}

fn validate_json_node(
    schema: &SchemaNode,
    value: &serde_json::Value,
    programs: &BTreeMap<String, PortableJsonPattern>,
    remaining_work: &mut u64,
) -> Result<(), ConstraintValidationError> {
    match (&schema.kind, value) {
        (SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }, value) => {
            if let serde_json::Value::String(value) = value {
                validate_pattern_value(schema, value, programs, remaining_work)?;
            }
            if let serde_json::Value::Number(value) = value {
                multiple_of::validate_json_value(schema, value)?;
            }
            allowed_values::validate_json_value(schema, value)
        }
        (
            SchemaKind::Group {
                children, dynamic, ..
            },
            serde_json::Value::Object(fields),
        ) => {
            for (name, value) in fields {
                let child = children
                    .iter()
                    .find(|child| child.name == *name)
                    .or(dynamic.as_deref());
                if let Some(child) = child {
                    if child.repeating {
                        if let serde_json::Value::Array(items) = value {
                            for item in items {
                                validate_json_node(child, item, programs, remaining_work)?;
                            }
                        }
                    } else {
                        validate_json_node(child, value, programs, remaining_work)?;
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_pattern_value(
    schema: &SchemaNode,
    value: &str,
    programs: &BTreeMap<String, PortableJsonPattern>,
    remaining_work: &mut u64,
) -> Result<(), ConstraintValidationError> {
    let Some(constraints) = &schema.json_patterns else {
        return Ok(());
    };
    for alternative in constraints.any_of() {
        let mut matched = true;
        for source in alternative {
            let Some(program) = programs.get(source) else {
                return Err(ConstraintValidationError::MissingPatternProgram {
                    source: source.clone(),
                });
            };
            match program.is_match_with_budget(value, remaining_work) {
                Ok(true) => {}
                Ok(false) => {
                    matched = false;
                    break;
                }
                Err(_) => {
                    return Err(ConstraintValidationError::PatternWorkLimit {
                        name: schema.name.clone(),
                    });
                }
            }
        }
        if matched {
            return Ok(());
        }
    }
    Err(ConstraintValidationError::PatternMismatch {
        name: schema.name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use ir::{
        IntegerRange, ItemCountRange, JsonMultipleOf, JsonMultipleOfConstraints,
        JsonPatternConstraints, NumericRange, ScalarType, ScalarTypeSet, StringLengthRange, Value,
    };

    use super::*;

    fn schema() -> (SchemaNode, String) {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Count", ScalarType::Int),
            ],
        );
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        (schema, encoded)
    }

    #[test]
    fn parses_and_serializes_with_the_embedded_schema() {
        let (_, schema) = schema();
        let parsed = parse_json(&schema, r#"{"Name":"sample","Count":3}"#);
        assert_eq!(
            parsed,
            Ok(Instance::Group(vec![
                (
                    "Name".into(),
                    Instance::Scalar(Value::String("sample".into()))
                ),
                ("Count".into(), Instance::Scalar(Value::Int(3))),
            ]))
        );
        let rendered = parsed.and_then(|instance| serialize_json(&schema, &instance));
        assert_eq!(
            rendered.as_deref(),
            Ok("{\n  \"Name\": \"sample\",\n  \"Count\": 3\n}\n")
        );
    }

    #[test]
    fn enforces_embedded_numeric_ranges_on_input_and_output() {
        let Some(range) = IntegerRange::new(Some(5), Some(8)).map(NumericRange::Integer) else {
            panic!("test range is valid");
        };
        let Some(schema) = SchemaNode::scalar("Count", ScalarType::Int).with_numeric_range(range)
        else {
            panic!("test range matches its scalar type");
        };
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, "5"),
            Ok(Instance::Scalar(Value::Int(5)))
        );
        assert!(matches!(
            parse_json(&encoded, "4"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("numeric range")
        ));
        assert!(matches!(
            serialize_json(
                &encoded,
                &Instance::Scalar(Value::String("9".to_string()))
            ),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("numeric range")
        ));
    }

    #[test]
    fn enforces_embedded_item_counts_on_input_and_output() {
        let Some(range) = ItemCountRange::new(1, Some(2)) else {
            panic!("test item-count range is valid");
        };
        let Some(schema) = SchemaNode::scalar("Values", ScalarType::Int)
            .repeating()
            .with_item_count_range(range)
        else {
            panic!("test item-count range matches a repeating node");
        };
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert!(matches!(
            parse_json(&encoded, "[]"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("between 1 and 2 array items")
        ));
        assert_eq!(
            parse_json(&encoded, "[1,2]"),
            Ok(Instance::Repeated(vec![
                Instance::Scalar(Value::Int(1)),
                Instance::Scalar(Value::Int(2)),
            ]))
        );
        assert!(matches!(
            serialize_json(&encoded, &Instance::Repeated(Vec::new())),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("between 1 and 2 array items")
        ));
        assert_eq!(
            serialize_json(
                &encoded,
                &Instance::Repeated(vec![Instance::Scalar(Value::Int(1))])
            )
            .as_deref(),
            Ok("[\n  1\n]\n")
        );

        let invalid = encoded.replace("\"repeating\":true", "\"repeating\":false");
        assert!(matches!(
            parse_json(&invalid, "1"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("item-count")
        ));
    }

    #[test]
    fn enforces_embedded_unicode_string_lengths_on_input_and_output() {
        let Some(range) = StringLengthRange::new(1, Some(1)) else {
            panic!("test string-length range is valid");
        };
        let Some(schema) =
            SchemaNode::scalar("Code", ScalarType::String).with_string_length_range(range)
        else {
            panic!("test string-length range matches its scalar type");
        };
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, r#""😀""#),
            Ok(Instance::Scalar(Value::String("😀".into())))
        );
        assert!(matches!(
            parse_json(&encoded, r#""e\u0301""#),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("Unicode scalar")
        ));
        assert!(matches!(
            serialize_json(
                &encoded,
                &Instance::Scalar(Value::String("e\u{301}".to_string()))
            ),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("Unicode scalar")
        ));

        let invalid = encoded.replace(r#""ty":"string""#, r#""ty":"int""#);
        assert!(matches!(
            parse_json(&invalid, "1"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("string-length range")
        ));
    }

    #[test]
    fn embedded_format_annotations_are_validated_but_not_asserted() {
        let Ok(formats) = ir::JsonFormatAnnotations::new([String::new(), "email".to_string()])
        else {
            panic!("test format annotations are bounded");
        };
        let Some(schema) =
            SchemaNode::scalar("Contact", ScalarType::String).with_json_formats(formats)
        else {
            panic!("format annotations match a string scalar");
        };
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, r#""not an email""#),
            Ok(Instance::Scalar(Value::String("not an email".into())))
        );
        assert_eq!(
            serialize_json(
                &encoded,
                &Instance::Scalar(Value::String("also not an email".into()))
            )
            .as_deref(),
            Ok("\"also not an email\"\n")
        );

        let invalid_type = encoded.replace(r#""ty":"string""#, r#""ty":"int""#);
        assert!(matches!(
            parse_json(&invalid_type, "1"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("format annotations")
        ));
        let invalid_any = encoded.replace(r#""kind":"#, r#""json_any":true,"kind":"#);
        assert!(matches!(
            parse_json(&invalid_any, r#""value""#),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("format annotations")
        ));
    }

    #[test]
    fn parses_and_serializes_utf8_payloads() {
        let (_, schema) = schema();
        let parsed = parse_json_bytes(
            &schema,
            b"\xEF\xBB\xBF{\"Name\":\"caf\xC3\xA9\",\"Count\":3}",
        );
        assert_eq!(
            parsed,
            Ok(Instance::Group(vec![
                (
                    "Name".into(),
                    Instance::Scalar(Value::String("café".into()))
                ),
                ("Count".into(), Instance::Scalar(Value::Int(3))),
            ]))
        );
        let rendered = parsed.and_then(|instance| serialize_json_bytes(&schema, &instance));
        assert_eq!(
            rendered.as_deref(),
            Ok(&b"{\n  \"Name\": \"caf\xC3\xA9\",\n  \"Count\": 3\n}\n"[..])
        );
        assert!(matches!(
            parse_json_bytes(&schema, b"{\"Name\":\"\xFF\"}"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("not UTF-8")
        ));
    }

    #[test]
    fn enforces_required_properties_at_native_boundaries() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Id", ScalarType::Int),
                SchemaNode::scalar("Note", ScalarType::String)
                    .nullable()
                    .unwrap(),
            ],
        )
        .with_required_fields(vec!["Id".into(), "Note".into()])
        .unwrap();
        let encoded = serde_json::to_string(&schema).unwrap_or_default();

        assert!(matches!(
            parse_json(&encoded, r#"{"Note":null}"#),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("requires property `Id`")
        ));
        assert!(parse_json(&encoded, r#"{"Id":7,"Note":null}"#).is_ok());

        let missing = Instance::Group(vec![
            ("Id".into(), Instance::Scalar(Value::Null)),
            (
                "Note".into(),
                Instance::Scalar(Value::String("present".into())),
            ),
        ]);
        assert!(matches!(
            serialize_json(&encoded, &missing),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("requires property `Id`")
        ));
    }

    #[test]
    fn retains_boundary_and_schema_failure_categories() {
        assert!(matches!(
            parse_json("{}", "{}"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { .. })
        ));
        assert!(matches!(
            check_input_size(MAX_JSON_DOCUMENT_BYTES.saturating_add(1)),
            Err(JsonBoundaryError::InputTooLarge { .. })
        ));
    }

    #[test]
    fn preserves_heterogeneous_scalar_union_tags_at_json_boundaries() {
        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
            panic!("test union must contain distinct scalar types");
        };
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar_union("Value", types),
                SchemaNode::scalar_union("Items", types).repeating(),
            ],
        );
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        let parsed = parse_json(&encoded, r#"{"Value":7,"Items":["A",8]}"#);
        assert_eq!(
            parsed,
            Ok(Instance::Group(vec![
                ("Value".into(), Instance::Scalar(Value::Int(7))),
                (
                    "Items".into(),
                    Instance::Repeated(vec![
                        Instance::Scalar(Value::String("A".into())),
                        Instance::Scalar(Value::Int(8)),
                    ]),
                ),
            ]))
        );
        let rendered = parsed.and_then(|instance| serialize_json(&encoded, &instance));
        assert_eq!(
            rendered.as_deref(),
            Ok("{\n  \"Value\": 7,\n  \"Items\": [\n    \"A\",\n    8\n  ]\n}\n")
        );
    }

    fn patterned_schema(name: &str, alternatives: &[&[&str]]) -> SchemaNode {
        let patterns = JsonPatternConstraints::new(
            alternatives
                .iter()
                .map(|terms| terms.iter().copied().map(str::to_string)),
        );
        let Ok(patterns) = patterns else {
            panic!("test JSON patterns are valid: {patterns:?}");
        };
        let schema = SchemaNode::scalar(name, ScalarType::String).with_json_patterns(patterns);
        let Some(schema) = schema else {
            panic!("test JSON patterns match a string schema");
        };
        schema
    }

    fn multiple_of_constraints(divisor: &str) -> JsonMultipleOfConstraints {
        let Some(divisor) = JsonMultipleOf::from_decimal_lexical(divisor) else {
            panic!("test multipleOf divisor is valid");
        };
        let Ok(constraints) = JsonMultipleOfConstraints::new([[divisor]]) else {
            panic!("test multipleOf constraints are valid");
        };
        constraints
    }

    fn multiple_of_schema(name: &str, ty: ScalarType, divisor: &str) -> SchemaNode {
        let constraints = multiple_of_constraints(divisor);
        let schema = SchemaNode::scalar(name, ty).with_json_multiple_of(constraints);
        let Some(schema) = schema else {
            panic!("test multipleOf constraints match the numeric schema");
        };
        schema
    }

    #[test]
    fn enforces_exact_integer_and_decimal_multiple_of_on_input() {
        let integer = multiple_of_schema("Quantity", ScalarType::Int, "3");
        let encoded = serde_json::to_string(&integer).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, "12"),
            Ok(Instance::Scalar(Value::Int(12)))
        );
        assert!(matches!(
            parse_json(&encoded, "13"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("exact multiple")
        ));

        let decimal = multiple_of_schema("Rate", ScalarType::Float, "0.1");
        let encoded = serde_json::to_string(&decimal).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, "0.3"),
            Ok(Instance::Scalar(Value::Float(0.3)))
        );
        assert!(matches!(
            parse_json(&encoded, "0.30000000000000004"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("exact multiple")
        ));

        let Some(types) =
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int, ScalarType::Float])
        else {
            panic!("test scalar union contains distinct scalar types");
        };
        let Some(union) = SchemaNode::scalar_union("Value", types)
            .with_json_multiple_of(multiple_of_constraints("3"))
        else {
            panic!("test multipleOf constraints match the numeric scalar union");
        };
        let encoded = serde_json::to_string(&union).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, r#""not-numeric""#),
            Ok(Instance::Scalar(Value::String("not-numeric".into())))
        );
        assert_eq!(
            parse_json(&encoded, "6.0"),
            Ok(Instance::Scalar(Value::Float(6.0)))
        );
        assert!(matches!(
            parse_json(&encoded, "7"),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));
    }

    #[test]
    fn handles_signed_zero_subnormals_and_signed_integer_minimum() {
        let integer = multiple_of_schema("Minimum", ScalarType::Int, "9223372036854775808");
        let encoded = serde_json::to_string(&integer).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, "-9223372036854775808"),
            Ok(Instance::Scalar(Value::Int(i64::MIN)))
        );
        assert!(matches!(
            parse_json(&encoded, "9223372036854775807"),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));

        let subnormal = multiple_of_schema("Subnormal", ScalarType::Float, "5e-324");
        let encoded = serde_json::to_string(&subnormal).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, "5e-324"),
            Ok(Instance::Scalar(Value::Float(f64::from_bits(1))))
        );
        let negative_zero = parse_json(&encoded, "-0.0");
        assert!(matches!(
            negative_zero,
            Ok(Instance::Scalar(Value::Float(value))) if value.to_bits() == (-0.0_f64).to_bits()
        ));
        assert_eq!(
            serialize_json(&encoded, &Instance::Scalar(Value::Float(f64::from_bits(1)))).as_deref(),
            Ok("5e-324\n")
        );

        let off_grid = multiple_of_schema("Subnormal", ScalarType::Float, "3e-324");
        let encoded = serde_json::to_string(&off_grid).unwrap_or_default();
        assert!(matches!(
            parse_json(&encoded, "5e-324"),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));
        assert!(matches!(
            serialize_json(&encoded, &Instance::Scalar(Value::Float(f64::from_bits(1)))),
            Err(JsonBoundaryError::InvalidOutput { .. })
        ));
    }

    #[test]
    fn validates_multiple_of_after_output_numeric_normalization() {
        let schema = multiple_of_schema("Amount", ScalarType::Float, "0.25");
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert_eq!(
            serialize_json(&encoded, &Instance::Scalar(Value::String("1.50".into()))).as_deref(),
            Ok("1.5\n")
        );
        assert!(matches!(
            serialize_json(&encoded, &Instance::Scalar(Value::String("1.3".into()))),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("exact multiple")
        ));

        let integer = multiple_of_schema("Count", ScalarType::Int, "3");
        let encoded = serde_json::to_string(&integer).unwrap_or_default();
        assert_eq!(
            serialize_json(&encoded, &Instance::Scalar(Value::String("12".into()))).as_deref(),
            Ok("12\n")
        );
        assert!(matches!(
            serialize_json(&encoded, &Instance::Scalar(Value::String("13".into()))),
            Err(JsonBoundaryError::InvalidOutput { .. })
        ));
    }

    #[test]
    fn rejects_invalid_embedded_multiple_of_domains() {
        let mut invalid = SchemaNode::scalar("Code", ScalarType::String);
        invalid.json_multiple_of = Some(multiple_of_constraints("2"));
        let encoded = serde_json::to_string(&invalid).unwrap_or_default();
        assert!(matches!(
            parse_json(&encoded, r#""value""#),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("multiple")
        ));
    }

    #[test]
    fn enforces_portable_pattern_dnf_and_scalar_union_string_tags() {
        let schema = patterned_schema("Code", &[&["^A", "Z$"], &["^(?:😀|[B-C]+)$"]]);
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        for value in [r#""ABZ""#, r#""😀""#, r#""BCC""#] {
            assert_eq!(
                parse_json(&encoded, value),
                serde_json::from_str::<String>(value)
                    .map(|value| Instance::Scalar(Value::String(value)))
                    .map_err(|error| JsonBoundaryError::InvalidInput {
                        message: error.to_string(),
                    })
            );
        }
        assert!(matches!(
            parse_json(&encoded, r#""AZZQ""#),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("pattern constraints")
        ));

        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
            panic!("test scalar union is heterogeneous");
        };
        let patterns = JsonPatternConstraints::new([["^S$"]]);
        let Ok(patterns) = patterns else {
            panic!("test scalar-union pattern is valid");
        };
        let Some(union) = SchemaNode::scalar_union("Value", types).with_json_patterns(patterns)
        else {
            panic!("string-containing scalar union accepts patterns");
        };
        let encoded = serde_json::to_string(&union).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, "7"),
            Ok(Instance::Scalar(Value::Int(7)))
        );
        assert!(matches!(
            parse_json(&encoded, r#""X""#),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));
    }

    #[test]
    fn preserves_unicode_anchor_and_empty_class_semantics() {
        let dot = patterned_schema("Value", &[&["^.$"]]);
        let encoded = serde_json::to_string(&dot).unwrap_or_default();
        assert!(parse_json(&encoded, r#""😀""#).is_ok());
        assert!(matches!(
            parse_json(&encoded, r#""\n""#),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));

        let empty = patterned_schema("Value", &[&["[]"]]);
        let encoded = serde_json::to_string(&empty).unwrap_or_default();
        assert!(matches!(
            parse_json(&encoded, r#""x""#),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));

        let complement = patterned_schema("Value", &[&["^[^]$"]]);
        let encoded = serde_json::to_string(&complement).unwrap_or_default();
        assert!(parse_json(&encoded, r#""\n""#).is_ok());
        assert!(matches!(
            parse_json(&encoded, r#""""#),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));
    }

    #[test]
    fn applies_one_shared_pattern_budget_across_repeated_values() {
        let schema = patterned_schema("Value", &[&["^a$"]]);
        let Ok(program) = PortableJsonPattern::compile("^a$") else {
            panic!("test pattern compiles");
        };
        let charge = program.work_estimate("a");
        let repeated = schema.repeating();
        let encoded = serde_json::to_string(&repeated).unwrap_or_default();
        let boundary =
            ConstraintBoundary::with_pattern_work_limit(charge.saturating_mul(2).saturating_sub(1));
        assert!(matches!(
            boundary.parse_json(&encoded, r#"["a","a"]"#),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("work limit")
        ));
        assert_eq!(boundary.patterns.borrow().len(), 1);
    }

    #[test]
    fn validates_patterns_after_output_string_normalization() {
        let schema = patterned_schema("Value", &[&["^7$"]]);
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert_eq!(
            serialize_json(&encoded, &Instance::Scalar(Value::Int(7))).as_deref(),
            Ok("\"7\"\n")
        );

        let schema = patterned_schema("Value", &[&["^8$"]]);
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert!(matches!(
            serialize_json(&encoded, &Instance::Scalar(Value::Int(7))),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("pattern constraints")
        ));
    }

    #[test]
    fn rejects_invalid_node_and_schema_wide_pattern_metadata() {
        let schema = patterned_schema("Value", &[&["^ok$"]]);
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        let invalid_domain = encoded.replace(r#""ty":"string""#, r#""ty":"int""#);
        assert!(matches!(
            parse_json(&invalid_domain, "1"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("JSON pattern")
        ));

        let mut children = Vec::new();
        for index in 0..=ir::MAX_DISTINCT_JSON_PATTERNS {
            let pattern = format!("^value-{index}$");
            let patterns = JsonPatternConstraints::new([[pattern]]);
            let Ok(patterns) = patterns else {
                panic!("individual test constraint is bounded");
            };
            let schema = SchemaNode::scalar(format!("Value{index}"), ScalarType::String)
                .with_json_patterns(patterns);
            let Some(schema) = schema else {
                panic!("individual test pattern matches its string node");
            };
            children.push(schema);
        }
        let encoded =
            serde_json::to_string(&SchemaNode::group("Root", children)).unwrap_or_default();
        assert!(matches!(
            parse_json(&encoded, "{}"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("schema-wide JSON pattern")
        ));
    }
}
