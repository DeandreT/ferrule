use std::path::Path;

use crate::Value;

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
}

impl<'a> ExecutionContext<'a> {
    /// Uses one path for both the active and top-level mapping.
    pub fn new(mapping_file_path: &'a Path) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path: mapping_file_path,
            current_datetime: None,
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
        }
    }

    /// Supplies one stable XML `dateTime` lexical value for the run.
    pub fn with_current_datetime(mut self, current_datetime: &'a str) -> Self {
        self.current_datetime = Some(current_datetime);
        self
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GeneratedItems, Instance, RuntimeError, ScopeContext, field, group, repeated, scalar,
    };

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
