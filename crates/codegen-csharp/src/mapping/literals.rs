//! Path, target-domain and runtime-name rendering.

use super::*;

pub(super) fn aggregate_collect_prefix(function: AggregateFunction, values: &str) -> String {
    if function == AggregateFunction::DecimalSum {
        format!(
            "global::Ferrule.Runtime.FerruleAggregates.CollectValue(global::Ferrule.Runtime.FerruleAggregateOperation.DecimalSum, {values}, "
        )
    } else {
        format!("{values}.Add(")
    }
}

pub(crate) const fn aggregate_function_name(function: AggregateFunction) -> &'static str {
    match function {
        AggregateFunction::Count => "Count",
        AggregateFunction::Sum => "Sum",
        AggregateFunction::DecimalSum => "DecimalSum",
        AggregateFunction::LastPresentText => "LastPresentText",
        AggregateFunction::LastDistinctText => "LastDistinctText",
        AggregateFunction::Avg => "Avg",
        AggregateFunction::Min => "Min",
        AggregateFunction::Max => "Max",
        AggregateFunction::Join => "Join",
        AggregateFunction::ItemAt => "ItemAt",
    }
}

pub(super) const fn runtime_value_name(value: codegen::RuntimeValue) -> &'static str {
    match value {
        codegen::RuntimeValue::MappingFilePath => "MappingFilePath",
        codegen::RuntimeValue::MainMappingFilePath => "MainMappingFilePath",
        codegen::RuntimeValue::CurrentDateTime => "CurrentDateTime",
    }
}

pub(super) fn render_path(path: &[String], output: &mut String) {
    if path.is_empty() {
        output.push_str("global::System.Array.Empty<string>()");
        return;
    }
    output.push_str("new string[] { ");
    for (index, segment) in path.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str(&literal::string(segment));
    }
    output.push_str(" }");
}

pub(super) fn render_target_domain(domain: ScalarTargetDomain) -> String {
    match domain {
        ScalarTargetDomain::Single(ty) => target_type(ty).to_string(),
        ScalarTargetDomain::Union(types) => types
            .iter()
            .map(target_type)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

pub(super) fn target_type(target_type: ScalarType) -> &'static str {
    match target_type {
        ScalarType::String => "TargetScalarDomain.String",
        ScalarType::Int => "TargetScalarDomain.Int64",
        ScalarType::Float => "TargetScalarDomain.Double",
        ScalarType::Bool => "TargetScalarDomain.Bool",
    }
}

pub(super) const fn scalar_type_name(value: ScalarType) -> &'static str {
    match value {
        ScalarType::String => "String",
        ScalarType::Int => "Int64",
        ScalarType::Float => "Double",
        ScalarType::Bool => "Bool",
    }
}

pub(crate) fn render_optional_path(path: Option<&[String]>, output: &mut String) {
    match path {
        Some(path) => render_path(path, output),
        None => output.push_str("null"),
    }
}

pub(crate) fn basic_selected_output_types() -> &'static str {
    SELECTED_TARGET_OUTPUT_TYPES
}

pub(crate) fn basic_path(path: &[String], output: &mut String) {
    render_path(path, output);
}

pub(super) type BasicScopeIndices<'a> = (usize, Vec<(&'a str, usize)>);
