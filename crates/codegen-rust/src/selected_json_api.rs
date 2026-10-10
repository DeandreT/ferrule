//! Additive strict JSON adapters over the existing selected typed execution.

use codegen::Program;

pub(super) fn render(program: &Program) -> String {
    let dynamic = program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some());
    let mut output = String::from(
        r#"#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectedJsonTargetOutput {
    Primary(String),
    Named(NamedJsonOutput),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectedJsonBytesTargetOutput {
    Primary(Vec<u8>),
    Named(NamedJsonBytesOutput),
}

pub fn execute_json_selected_target(
    source: &str,
    selection: TargetSelection<'_>,
) -> Result<SelectedJsonTargetOutput, JsonBoundaryError> {
    execute_json_selected_target_with_host(source, &[], None, None, selection)
}

pub fn execute_json_bytes_selected_target(
    source: &[u8],
    selection: TargetSelection<'_>,
) -> Result<SelectedJsonBytesTargetOutput, JsonBoundaryError> {
    execute_json_bytes_selected_target_with_host(source, &[], None, None, selection)
}

"#,
    );
    for bytes in [false, true] {
        let prefix = if bytes {
            "execute_json_bytes"
        } else {
            "execute_json"
        };
        let source = if bytes { "&[u8]" } else { "&str" };
        let input = if bytes {
            "NamedJsonBytesInput"
        } else {
            "NamedJsonInput"
        };
        let result = if bytes {
            "SelectedJsonBytesTargetOutput"
        } else {
            "SelectedJsonTargetOutput"
        };
        let parse = if bytes {
            "parse_json_bytes"
        } else {
            "parse_json"
        };
        let validate = if bytes {
            "validate_named_json_bytes_input_names"
        } else {
            "validate_named_json_input_names"
        };
        let parse_named = if bytes {
            "parse_named_json_bytes_inputs"
        } else {
            "parse_named_json_inputs"
        };
        let serialize = if bytes {
            "serialize_selected_json_bytes"
        } else {
            "serialize_selected_json"
        };
        for controlled in [false, true] {
            let suffix = if controlled {
                "with_filter_map_controls"
            } else {
                "with_host"
            };
            output.push_str(&format!(
                "pub fn {prefix}_selected_target_{suffix}(\n    source: {source},\n    inputs: &[{input}<'_>],\n    execution: Option<&ExecutionContext<'_>>,\n    loader: Option<&dyn DynamicJsonSourceLoader>,\n    selection: TargetSelection<'_>,\n"
            ));
            if controlled {
                output.push_str("    limits: FilterMapLimits,\n    cancellation: Option<&dyn FilterMapCancellation>,\n");
            }
            let return_type = if controlled {
                format!("FilterMapExecution<{result}, JsonBoundaryError>")
            } else {
                result.to_string()
            };
            output.push_str(&format!(
                ") -> Result<{return_type}, JsonBoundaryError> {{\n    let selected = resolve_target(selection)?;\n    {validate}(inputs)?;\n    let source = {parse}(SOURCE_JSON_SCHEMA, source)?;\n    let parsed = {parse_named}(inputs)?;\n    let inputs = parsed.iter().map(|(name, instance)| NamedInput {{ name, instance }}).collect::<Vec<_>>();\n"
            ));
            if dynamic {
                output.push_str(
                    "    let loader = loader.map(|loader| GeneratedDynamicJsonSourceLoader { loader, total_bytes: std::cell::Cell::new(0) });\n    let loader = loader.as_ref().map(|loader| loader as &dyn DynamicSourceLoader);\n",
                );
            } else {
                output.push_str("    let _ = loader;\n    let loader = None;\n");
            }
            if controlled {
                output.push_str(&format!(
                    "    let actual = execute_selected_target_with_filter_map_controls(&source, &inputs, execution, loader, selection, limits, cancellation)?;\n    Ok(FilterMapExecution {{ counters: actual.counters, outcome: actual.outcome.map_err(JsonBoundaryError::from).and_then(|output| {serialize}(output, selected)) }})\n}}\n\n"
                ));
            } else {
                output.push_str(&format!(
                    "    let output = execute_selected_target_with_host(&source, &inputs, execution, loader, selection)?;\n    {serialize}(output, selected)\n}}\n\n"
                ));
            }
        }
    }
    for bytes in [false, true] {
        let (name, result, serialize) = if bytes {
            (
                "serialize_selected_json_bytes",
                "SelectedJsonBytesTargetOutput",
                "serialize_json_bytes",
            )
        } else {
            (
                "serialize_selected_json",
                "SelectedJsonTargetOutput",
                "serialize_json",
            )
        };
        let named = if bytes {
            "NamedJsonBytesOutput"
        } else {
            "NamedJsonOutput"
        };
        output.push_str(&format!(
            "fn {name}(\n    output: SelectedTargetOutput,\n    selected: ResolvedTarget,\n) -> Result<{result}, JsonBoundaryError> {{\n    match (selected, output) {{\n        (ResolvedTarget::Primary, SelectedTargetOutput::Primary(instance)) => Ok({result}::Primary({serialize}(TARGET_JSON_SCHEMA, &instance)?)),\n"
        ));
        for (index, target) in program.extra_targets.iter().enumerate() {
            let target_name = super::rust_string(&target.name);
            output.push_str(&format!(
                "        (ResolvedTarget::Named{index}, SelectedTargetOutput::Named(output)) if output.name == {target_name} => Ok({result}::Named({named} {{ name: output.name, document: {serialize}(EXTRA_TARGET_JSON_SCHEMAS[{index}], &output.instance)? }})),\n"
            ));
        }
        output.push_str(
            "        _ => Err(JsonBoundaryError::InvalidOutput { message: \"generated mapping returned a target that does not match the resolved selection\".to_string() }),\n    }\n}\n\n",
        );
    }
    output
}
