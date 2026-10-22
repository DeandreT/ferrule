use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::{RuntimeError, Value};

pub const MAX_RUNTIME_PARAMETERS: usize = 1_024;
pub const MAX_RUNTIME_PARAMETER_NAME_BYTES: usize = 256;
pub const MAX_RUNTIME_PARAMETER_STRING_BYTES: usize = 8 * 1024 * 1024;

/// Invalid host parameter input rejected before generated mapping execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeParameterError {
    EmptyName,
    NameContainsNul,
    NameTooLong { limit: usize },
    Duplicate { name: String },
    TooMany { limit: usize },
    StringTooLong { name: String, limit: usize },
}

impl fmt::Display for RuntimeParameterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("runtime parameter name cannot be empty"),
            Self::NameContainsNul => {
                formatter.write_str("runtime parameter name cannot contain NUL")
            }
            Self::NameTooLong { limit } => {
                write!(
                    formatter,
                    "runtime parameter name exceeds {limit} UTF-8 bytes"
                )
            }
            Self::Duplicate { name } => {
                write!(formatter, "runtime parameter `{name}` is duplicated")
            }
            Self::TooMany { limit } => {
                write!(formatter, "runtime parameter count exceeds {limit}")
            }
            Self::StringTooLong { name, limit } => write!(
                formatter,
                "runtime parameter `{name}` string value exceeds {limit} UTF-8 bytes"
            ),
        }
    }
}

impl std::error::Error for RuntimeParameterError {}

/// Immutable during execution, bounded named scalar inputs supplied by a host.
#[derive(Debug, Clone, Default)]
pub struct RuntimeParameters {
    values: BTreeMap<String, Value>,
}

impl RuntimeParameters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        name: impl Into<String>,
        value: Value,
    ) -> Result<(), RuntimeParameterError> {
        let name = name.into();
        if name.is_empty() {
            return Err(RuntimeParameterError::EmptyName);
        }
        if name.contains('\0') {
            return Err(RuntimeParameterError::NameContainsNul);
        }
        if name.len() > MAX_RUNTIME_PARAMETER_NAME_BYTES {
            return Err(RuntimeParameterError::NameTooLong {
                limit: MAX_RUNTIME_PARAMETER_NAME_BYTES,
            });
        }
        if self.values.contains_key(&name) {
            return Err(RuntimeParameterError::Duplicate { name });
        }
        if self.values.len() >= MAX_RUNTIME_PARAMETERS {
            return Err(RuntimeParameterError::TooMany {
                limit: MAX_RUNTIME_PARAMETERS,
            });
        }
        if matches!(&value, Value::String(text) if text.len() > MAX_RUNTIME_PARAMETER_STRING_BYTES)
        {
            return Err(RuntimeParameterError::StringTooLong {
                name,
                limit: MAX_RUNTIME_PARAMETER_STRING_BYTES,
            });
        }
        self.values.insert(name, value);
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }
}

/// A scalar supplied by the generated mapping's execution host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeValue {
    /// Path of the mapping that owns the expression being evaluated.
    MappingFilePath,
    /// Path of the top-level mapping for the current run.
    MainMappingFilePath,
    /// One stable local timestamp captured for the current run.
    CurrentDateTime,
}

/// Host values available to runtime expressions in generated mappings.
#[derive(Clone, Copy)]
pub struct ExecutionContext<'a> {
    mapping_file_path: &'a Path,
    main_mapping_file_path: &'a Path,
    current_datetime: Option<&'a str>,
    parameters: Option<&'a RuntimeParameters>,
    invocation_counter_limits: Option<crate::InvocationCounterLimits>,
}

impl<'a> ExecutionContext<'a> {
    /// Uses one path for both the active and top-level mapping.
    pub fn new(mapping_file_path: &'a Path) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path: mapping_file_path,
            current_datetime: None,
            parameters: None,
            invocation_counter_limits: None,
        }
    }

    /// Distinguishes a reusable mapping's path from its top-level caller.
    pub fn with_main_mapping_file_path(
        mapping_file_path: &'a Path,
        main_mapping_file_path: &'a Path,
    ) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path,
            current_datetime: None,
            parameters: None,
            invocation_counter_limits: None,
        }
    }

    /// Supplies one stable XML `dateTime` lexical value for the run.
    pub fn with_current_datetime(mut self, current_datetime: &'a str) -> Self {
        self.current_datetime = Some(current_datetime);
        self
    }

    /// Supplies bounded named scalar inputs for runtime-parameter expressions.
    pub fn with_parameters(mut self, parameters: &'a RuntimeParameters) -> Self {
        self.parameters = Some(parameters);
        self
    }

    /// Supplies immutable limits, never a mutable table reused across calls.
    pub fn with_invocation_counter_limits(
        mut self,
        limits: crate::InvocationCounterLimits,
    ) -> Self {
        self.invocation_counter_limits = Some(limits);
        self
    }

    pub(crate) fn invocation_counter_limits(self) -> Option<crate::InvocationCounterLimits> {
        self.invocation_counter_limits
    }

    /// Resolve a named host scalar before any source instance is acquired.
    /// Uses the same coercion and typed failures as a bound scope context.
    pub fn resolve_runtime_parameter(
        self,
        node: u32,
        name: &str,
        expected: crate::ScalarType,
    ) -> Result<Value, RuntimeError> {
        resolve_runtime_parameter(Some(self), node, name, expected)
    }

    /// Resolve a global filename parameter without a source or preview fallback.
    /// Empty strings and path syntax remain the acquisition policy's decision.
    pub fn resolve_primary_filename(self, node: u32, name: &str) -> Result<String, RuntimeError> {
        match self.resolve_runtime_parameter(node, name, crate::ScalarType::String)? {
            Value::String(text) => Ok(text),
            value => Err(RuntimeError::PrimaryFilenameType {
                node,
                found: value.type_name(),
            }),
        }
    }

    pub(crate) fn value(self, value: RuntimeValue) -> Option<Value> {
        match value {
            RuntimeValue::MappingFilePath => Some(Value::String(
                self.mapping_file_path.to_string_lossy().into_owned(),
            )),
            RuntimeValue::MainMappingFilePath => Some(Value::String(
                self.main_mapping_file_path.to_string_lossy().into_owned(),
            )),
            RuntimeValue::CurrentDateTime => self
                .current_datetime
                .map(|value| Value::String(value.to_string())),
        }
    }

    pub(crate) fn parameter(self, name: &str) -> Option<&'a Value> {
        self.parameters.and_then(|parameters| parameters.get(name))
    }
}

pub(crate) fn resolve_runtime_parameter(
    execution: Option<ExecutionContext<'_>>,
    node: u32,
    name: &str,
    expected: crate::ScalarType,
) -> Result<Value, RuntimeError> {
    let value = execution
        .and_then(|execution| execution.parameter(name))
        .ok_or_else(|| RuntimeError::MissingRuntimeParameter {
            node,
            name: name.to_string(),
        })?;
    crate::value_map::coerce_input(value, expected).ok_or_else(|| {
        RuntimeError::RuntimeParameterType {
            node,
            name: name.to_string(),
            expected,
            found: value.type_name(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GeneratedItems, Instance, RuntimeError, ScopeContext, field, group, repeated, scalar,
    };

    #[test]
    fn source_independent_parameters_preserve_scope_values_and_errors() {
        let mut parameters = RuntimeParameters::new();
        parameters
            .insert("filename", Value::String(" ./input-*.csv ".into()))
            .unwrap();
        parameters.insert("number", Value::Int(42)).unwrap();
        parameters.insert("null", Value::Null).unwrap();
        parameters.insert("json-null", Value::json_null()).unwrap();
        parameters.insert("xml-nil", Value::xml_nil()).unwrap();
        parameters.insert("bad", Value::Float(f64::NAN)).unwrap();
        let execution =
            ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&parameters);
        let source = group([field(
            "filename",
            scalar(Value::String("not-the-parameter.csv".into())),
        )]);
        let scope = ScopeContext::with_execution_context(&source, &execution);
        for (name, expected) in [
            ("filename", Value::String(" ./input-*.csv ".into())),
            ("number", Value::String("42".into())),
            ("null", Value::Null),
            ("json-null", Value::json_null()),
            ("xml-nil", Value::xml_nil()),
        ] {
            assert_eq!(
                execution.resolve_runtime_parameter(17, name, crate::ScalarType::String),
                Ok(expected.clone())
            );
            assert_eq!(
                scope.runtime_parameter(17, name, crate::ScalarType::String),
                Ok(expected)
            );
        }
        for name in ["missing", "bad"] {
            let direct = execution.resolve_runtime_parameter(19, name, crate::ScalarType::String);
            assert_eq!(
                direct,
                scope.runtime_parameter(19, name, crate::ScalarType::String)
            );
            assert!(direct.is_err());
        }
        assert_eq!(
            execution.resolve_runtime_parameter(19, "missing", crate::ScalarType::String),
            Err(RuntimeError::MissingRuntimeParameter {
                node: 19,
                name: "missing".into()
            })
        );
        assert_eq!(
            execution.resolve_runtime_parameter(19, "bad", crate::ScalarType::String),
            Err(RuntimeError::RuntimeParameterType {
                node: 19,
                name: "bad".into(),
                expected: crate::ScalarType::String,
                found: "float"
            })
        );
        let absent = ExecutionContext::new(Path::new("mapping.ferrule"));
        assert_eq!(
            absent.resolve_runtime_parameter(23, "filename", crate::ScalarType::String),
            Err(RuntimeError::MissingRuntimeParameter {
                node: 23,
                name: "filename".into(),
            })
        );
        assert_eq!(
            absent.resolve_runtime_parameter(23, "filename", crate::ScalarType::String),
            ScopeContext::new(&source).runtime_parameter(23, "filename", crate::ScalarType::String)
        );
    }

    fn assert_host_values(context: &ScopeContext<'_>) {
        assert_eq!(
            context.runtime_value(RuntimeValue::MappingFilePath),
            Ok(Value::String("relative/library.ferrule.json".into()))
        );
        assert_eq!(
            context.runtime_value(RuntimeValue::MainMappingFilePath),
            Ok(Value::String("/maps/main.ferrule.json".into()))
        );
        assert_eq!(
            context.runtime_value(RuntimeValue::CurrentDateTime),
            Ok(Value::String("2026-07-19T11:22:33.45-07:00".into()))
        );
    }

    fn source() -> Instance {
        group([field(
            "Rows",
            repeated([
                group([field("Value", scalar(Value::Int(1)))]),
                group([field("Value", scalar(Value::Int(2)))]),
            ]),
        )])
    }

    fn host_context<'a>(source: &'a Instance) -> ScopeContext<'a> {
        let execution = ExecutionContext::with_main_mapping_file_path(
            Path::new("relative/library.ferrule.json"),
            Path::new("/maps/main.ferrule.json"),
        )
        .with_current_datetime("2026-07-19T11:22:33.45-07:00");
        ScopeContext::with_execution_context(source, &execution)
    }

    #[test]
    fn one_path_aliases_active_and_main_while_datetime_remains_optional() {
        let source = source();
        let execution = ExecutionContext::new(Path::new("relative/map.ferrule.json"));
        let context = ScopeContext::with_execution_context(&source, &execution);

        for value in [
            RuntimeValue::MappingFilePath,
            RuntimeValue::MainMappingFilePath,
        ] {
            assert_eq!(
                context.runtime_value(value),
                Ok(Value::String("relative/map.ferrule.json".into()))
            );
        }
        assert_eq!(
            context.runtime_value(RuntimeValue::CurrentDateTime),
            Err(RuntimeError::MissingRuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            })
        );
    }

    #[test]
    fn active_main_and_current_values_are_distinct_exact_host_text() {
        let source = source();
        assert_host_values(&host_context(&source));
    }

    #[test]
    fn typed_parameters_coerce_and_report_missing_or_wrong_values() {
        let source = source();
        let mut parameters = RuntimeParameters::new();
        parameters
            .insert("control_number", Value::String(" 42 ".into()))
            .unwrap();
        parameters.insert("test_mode", Value::Bool(false)).unwrap();
        let execution =
            ExecutionContext::new(Path::new("map.ferrule")).with_parameters(&parameters);
        let context = ScopeContext::with_execution_context(&source, &execution);

        assert_eq!(
            context.runtime_parameter(7, "control_number", crate::ScalarType::Int),
            Ok(Value::Int(42))
        );
        assert_eq!(
            context.runtime_parameter(8, "missing", crate::ScalarType::String),
            Err(RuntimeError::MissingRuntimeParameter {
                node: 8,
                name: "missing".into(),
            })
        );
        assert_eq!(
            context.runtime_parameter(9, "test_mode", crate::ScalarType::Int),
            Err(RuntimeError::RuntimeParameterType {
                node: 9,
                name: "test_mode".into(),
                expected: crate::ScalarType::Int,
                found: "bool",
            })
        );
    }

    #[test]
    fn parameter_sets_reject_ambiguous_and_unbounded_inputs() {
        let mut parameters = RuntimeParameters::new();
        assert_eq!(
            parameters.insert("", Value::Null),
            Err(RuntimeParameterError::EmptyName)
        );
        assert_eq!(
            parameters.insert("bad\0name", Value::Null),
            Err(RuntimeParameterError::NameContainsNul)
        );
        assert_eq!(
            parameters.insert(
                "x".repeat(MAX_RUNTIME_PARAMETER_NAME_BYTES + 1),
                Value::Null,
            ),
            Err(RuntimeParameterError::NameTooLong {
                limit: MAX_RUNTIME_PARAMETER_NAME_BYTES,
            })
        );
        parameters.insert("duplicate", Value::Int(1)).unwrap();
        assert_eq!(
            parameters.insert("duplicate", Value::Int(2)),
            Err(RuntimeParameterError::Duplicate {
                name: "duplicate".into(),
            })
        );
        assert_eq!(
            parameters.insert(
                "large",
                Value::String("x".repeat(MAX_RUNTIME_PARAMETER_STRING_BYTES + 1)),
            ),
            Err(RuntimeParameterError::StringTooLong {
                name: "large".into(),
                limit: MAX_RUNTIME_PARAMETER_STRING_BYTES,
            })
        );

        let mut parameters = RuntimeParameters::new();
        for index in 0..MAX_RUNTIME_PARAMETERS {
            parameters
                .insert(format!("parameter_{index}"), Value::Int(index as i64))
                .unwrap();
        }
        assert_eq!(
            parameters.insert("one_too_many", Value::Null),
            Err(RuntimeParameterError::TooMany {
                limit: MAX_RUNTIME_PARAMETERS,
            })
        );
    }

    #[test]
    fn root_without_an_execution_context_reports_each_missing_kind() {
        let source = source();
        let context = ScopeContext::new(&source);
        for value in [
            RuntimeValue::MappingFilePath,
            RuntimeValue::MainMappingFilePath,
            RuntimeValue::CurrentDateTime,
        ] {
            assert_eq!(
                context.runtime_value(value),
                Err(RuntimeError::MissingRuntimeValue { value })
            );
        }
    }

    #[test]
    fn source_iteration_and_compacted_views_retain_execution_context() {
        let source = source();
        let context = host_context(&source);
        let rows = context.walk_source(&["Rows"]);
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_host_values(&row);
            assert_host_values(&row.with_compact_last_position(1));
        }
    }

    #[test]
    fn generated_lazy_and_materialized_views_retain_execution_context() {
        let source = source();
        let context = host_context(&source);
        let items = GeneratedItems::new(vec![Value::String("a".into()), Value::Int(2)]);

        let lazy = context.generated_item_contexts(&items).collect::<Vec<_>>();
        assert_eq!(lazy.len(), 2);
        lazy.iter().for_each(assert_host_values);

        let materialized = context.generated_items(&items);
        assert_eq!(materialized.len(), 2);
        materialized.iter().for_each(assert_host_values);
    }

    #[test]
    fn aggregate_item_views_retain_execution_context() {
        let source = source();
        let context = host_context(&source);
        let items = context.aggregate_items(&["Rows"]);
        assert_eq!(items.len(), 2);
        items.iter().for_each(assert_host_values);
    }

    #[cfg(unix)]
    #[test]
    fn rust_paths_use_lossy_host_conversion_like_the_engine() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let source = source();
        let path = Path::new(OsStr::from_bytes(b"map-\xff.ferrule.json"));
        let execution = ExecutionContext::new(path);
        let context = ScopeContext::with_execution_context(&source, &execution);
        assert_eq!(
            context.runtime_value(RuntimeValue::MappingFilePath),
            Ok(Value::String("map-\u{fffd}.ferrule.json".into()))
        );
    }
}
