//! Schema-shaped JSON text and UTF-8 companions over the typed entry points.

use codegen::{Program, serialize_embedded_schema};

use crate::{EmitError, rust_string};

/// Renders the bounded JSON companions; each embedded descriptor is admitted
/// against the JSON descriptor limit.
pub(crate) fn render(program: &Program) -> Result<String, EmitError> {
    let static_sources = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_none())
        .collect::<Vec<_>>();
    let dynamic_sources = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_some())
        .collect::<Vec<_>>();
    let source_schema =
        serialize_embedded_schema(&program.source, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)?;
    let target_schema =
        serialize_embedded_schema(&program.target, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)?;
    let extra_source_schemas = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_none())
        .map(|source| {
            serialize_embedded_schema(&source.source, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .map(|schema| rust_string(&schema))
                .map_err(EmitError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let dynamic_source_schemas = dynamic_sources
        .iter()
        .map(|source| {
            serialize_embedded_schema(&source.source, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .map(|schema| rust_string(&schema))
                .map_err(EmitError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let extra_target_schemas = program
        .extra_targets
        .iter()
        .map(|target| {
            serialize_embedded_schema(&target.target, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .map(|schema| rust_string(&schema))
                .map_err(EmitError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut output = format!(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct NamedJsonInput<'a> {{\n\
             pub name: &'a str,\n\
             pub document: &'a str,\n\
         }}\n\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct NamedJsonBytesInput<'a> {{\n\
             pub name: &'a str,\n\
             pub document: &'a [u8],\n\
         }}\n\n\
         #[derive(Clone, Debug, PartialEq, Eq)]\n\
         pub struct NamedJsonOutput {{\n\
             pub name: &'static str,\n\
             pub document: String,\n\
         }}\n\n\
         #[derive(Clone, Debug, PartialEq, Eq)]\n\
         pub struct NamedJsonBytesOutput {{\n\
             pub name: &'static str,\n\
             pub document: Vec<u8>,\n\
         }}\n\n\
         #[derive(Clone, Debug, PartialEq, Eq)]\n\
         pub struct JsonExecutionOutputs {{\n\
             pub primary: String,\n\
             pub extras: Vec<NamedJsonOutput>,\n\
         }}\n\n\
         #[derive(Clone, Debug, PartialEq, Eq)]\n\
         pub struct JsonBytesExecutionOutputs {{\n\
             pub primary: Vec<u8>,\n\
             pub extras: Vec<NamedJsonBytesOutput>,\n\
         }}\n\n\
         const SOURCE_JSON_SCHEMA: &str = {};\n\
         const TARGET_JSON_SCHEMA: &str = {};\n\
         const EXTRA_TARGET_JSON_SCHEMAS: &[&str] = &[{}];\n\n",
        rust_string(&source_schema),
        rust_string(&target_schema),
        extra_target_schemas.join(", "),
    );
    if !static_sources.is_empty() {
        output.push_str(&format!(
            "const EXTRA_SOURCE_JSON_SCHEMAS: &[&str] = &[{}];\n\n",
            extra_source_schemas.join(", ")
        ));
    }
    if !dynamic_sources.is_empty() {
        output.push_str(&format!(
            "const DYNAMIC_SOURCE_JSON_SCHEMAS: &[&str] = &[{}];\n\n",
            dynamic_source_schemas.join(", ")
        ));
    }
    output.push_str(
        "pub fn execute_json(source: &str) -> Result<String, JsonBoundaryError> {\n\
             Ok(execute_json_outputs(source)?.primary)\n\
         }\n\n\
         pub fn execute_json_with_context(\n\
             source: &str,\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<String, JsonBoundaryError> {\n\
             Ok(execute_json_outputs_with_context(source, execution)?.primary)\n\
         }\n\n\
         pub fn execute_json_with_sources(\n\
             source: &str,\n\
             inputs: &[NamedJsonInput<'_>],\n\
         ) -> Result<String, JsonBoundaryError> {\n\
             Ok(execute_json_outputs_with_sources(source, inputs)?.primary)\n\
         }\n\n\
         pub fn execute_json_with_sources_and_context(\n\
             source: &str,\n\
             inputs: &[NamedJsonInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<String, JsonBoundaryError> {\n\
             Ok(execute_json_outputs_with_sources_and_context(source, inputs, execution)?.primary)\n\
         }\n\n\
         pub fn execute_json_outputs(\n\
             source: &str,\n\
         ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
             execute_json_outputs_with_sources(source, &[])\n\
         }\n\n\
         pub fn execute_json_outputs_with_context(\n\
             source: &str,\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
             execute_json_outputs_with_sources_and_context(source, &[], execution)\n\
         }\n\n\
         pub fn execute_json_outputs_with_sources(\n\
             source: &str,\n\
             inputs: &[NamedJsonInput<'_>],\n\
         ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
             validate_named_json_input_names(inputs)?;\n\
             let source = parse_json(SOURCE_JSON_SCHEMA, source)?;\n\
             let parsed = parse_named_json_inputs(inputs)?;\n\
             let inputs = parsed\n\
                 .iter()\n\
                 .map(|(name, instance)| NamedInput { name, instance })\n\
                 .collect::<Vec<_>>();\n\
             serialize_json_outputs(execute_outputs_with_sources(&source, &inputs)?)\n\
         }\n\n\
         pub fn execute_json_outputs_with_sources_and_context(\n\
             source: &str,\n\
             inputs: &[NamedJsonInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
             validate_named_json_input_names(inputs)?;\n\
             let source = parse_json(SOURCE_JSON_SCHEMA, source)?;\n\
             let parsed = parse_named_json_inputs(inputs)?;\n\
             let inputs = parsed\n\
                 .iter()\n\
                 .map(|(name, instance)| NamedInput { name, instance })\n\
                 .collect::<Vec<_>>();\n\
             serialize_json_outputs(execute_outputs_with_sources_and_context(\n\
                 &source,\n\
                 &inputs,\n\
                 execution,\n\
             )?)\n\
         }\n\n\
         pub fn execute_json_bytes(source: &[u8]) -> Result<Vec<u8>, JsonBoundaryError> {\n\
             Ok(execute_json_bytes_outputs(source)?.primary)\n\
         }\n\n\
         pub fn execute_json_bytes_with_context(\n\
             source: &[u8],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<Vec<u8>, JsonBoundaryError> {\n\
             Ok(execute_json_bytes_outputs_with_context(source, execution)?.primary)\n\
         }\n\n\
         pub fn execute_json_bytes_with_sources(\n\
             source: &[u8],\n\
             inputs: &[NamedJsonBytesInput<'_>],\n\
         ) -> Result<Vec<u8>, JsonBoundaryError> {\n\
             Ok(execute_json_bytes_outputs_with_sources(source, inputs)?.primary)\n\
         }\n\n\
         pub fn execute_json_bytes_with_sources_and_context(\n\
             source: &[u8],\n\
             inputs: &[NamedJsonBytesInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<Vec<u8>, JsonBoundaryError> {\n\
             Ok(execute_json_bytes_outputs_with_sources_and_context(source, inputs, execution)?.primary)\n\
         }\n\n\
         pub fn execute_json_bytes_outputs(\n\
             source: &[u8],\n\
         ) -> Result<JsonBytesExecutionOutputs, JsonBoundaryError> {\n\
             execute_json_bytes_outputs_with_sources(source, &[])\n\
         }\n\n\
         pub fn execute_json_bytes_outputs_with_context(\n\
             source: &[u8],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<JsonBytesExecutionOutputs, JsonBoundaryError> {\n\
             execute_json_bytes_outputs_with_sources_and_context(source, &[], execution)\n\
         }\n\n\
         pub fn execute_json_bytes_outputs_with_sources(\n\
             source: &[u8],\n\
             inputs: &[NamedJsonBytesInput<'_>],\n\
         ) -> Result<JsonBytesExecutionOutputs, JsonBoundaryError> {\n\
             validate_named_json_bytes_input_names(inputs)?;\n\
             let source = parse_json_bytes(SOURCE_JSON_SCHEMA, source)?;\n\
             let parsed = parse_named_json_bytes_inputs(inputs)?;\n\
             let inputs = parsed\n\
                 .iter()\n\
                 .map(|(name, instance)| NamedInput { name, instance })\n\
                 .collect::<Vec<_>>();\n\
             serialize_json_bytes_outputs(execute_outputs_with_sources(&source, &inputs)?)\n\
         }\n\n\
         pub fn execute_json_bytes_outputs_with_sources_and_context(\n\
             source: &[u8],\n\
             inputs: &[NamedJsonBytesInput<'_>],\n\
             execution: &ExecutionContext<'_>,\n\
         ) -> Result<JsonBytesExecutionOutputs, JsonBoundaryError> {\n\
             validate_named_json_bytes_input_names(inputs)?;\n\
             let source = parse_json_bytes(SOURCE_JSON_SCHEMA, source)?;\n\
             let parsed = parse_named_json_bytes_inputs(inputs)?;\n\
             let inputs = parsed\n\
                 .iter()\n\
                 .map(|(name, instance)| NamedInput { name, instance })\n\
                 .collect::<Vec<_>>();\n\
             serialize_json_bytes_outputs(execute_outputs_with_sources_and_context(\n\
                 &source,\n\
                 &inputs,\n\
                 execution,\n\
             )?)\n\
         }\n\n",
    );
    if !dynamic_sources.is_empty() {
        output.push_str(
            "pub fn execute_json_with_dynamic_source_loader(\n\
                 source: &str,\n\
                 loader: &dyn DynamicJsonSourceLoader,\n\
             ) -> Result<String, JsonBoundaryError> {\n\
                 Ok(execute_json_outputs_with_dynamic_source_loader(source, loader)?.primary)\n\
             }\n\n\
             pub fn execute_json_with_sources_and_dynamic_source_loader(\n\
                 source: &str,\n\
                 inputs: &[NamedJsonInput<'_>],\n\
                 loader: &dyn DynamicJsonSourceLoader,\n\
             ) -> Result<String, JsonBoundaryError> {\n\
                 Ok(execute_json_outputs_with_sources_and_dynamic_source_loader(source, inputs, loader)?.primary)\n\
             }\n\n\
             pub fn execute_json_with_sources_context_and_dynamic_source_loader(\n\
                 source: &str,\n\
                 inputs: &[NamedJsonInput<'_>],\n\
                 execution: &ExecutionContext<'_>,\n\
                 loader: &dyn DynamicJsonSourceLoader,\n\
             ) -> Result<String, JsonBoundaryError> {\n\
                 Ok(execute_json_outputs_with_sources_context_and_dynamic_source_loader(source, inputs, execution, loader)?.primary)\n\
             }\n\n\
             pub fn execute_json_outputs_with_dynamic_source_loader(\n\
                 source: &str,\n\
                 loader: &dyn DynamicJsonSourceLoader,\n\
             ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
                 execute_json_outputs_with_json_loader(source, &[], None, loader)\n\
             }\n\n\
             pub fn execute_json_outputs_with_sources_and_dynamic_source_loader(\n\
                 source: &str,\n\
                 inputs: &[NamedJsonInput<'_>],\n\
                 loader: &dyn DynamicJsonSourceLoader,\n\
             ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
                 execute_json_outputs_with_json_loader(source, inputs, None, loader)\n\
             }\n\n\
             pub fn execute_json_outputs_with_sources_context_and_dynamic_source_loader(\n\
                 source: &str,\n\
                 inputs: &[NamedJsonInput<'_>],\n\
                 execution: &ExecutionContext<'_>,\n\
                 loader: &dyn DynamicJsonSourceLoader,\n\
             ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
                 execute_json_outputs_with_json_loader(source, inputs, Some(execution), loader)\n\
             }\n\n\
             fn execute_json_outputs_with_json_loader(\n\
                 source: &str,\n\
                 inputs: &[NamedJsonInput<'_>],\n\
                 execution: Option<&ExecutionContext<'_>>,\n\
                 loader: &dyn DynamicJsonSourceLoader,\n\
             ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
                 validate_named_json_input_names(inputs)?;\n\
                 let source = parse_json(SOURCE_JSON_SCHEMA, source)?;\n\
                 let parsed = parse_named_json_inputs(inputs)?;\n\
                 let inputs = parsed\n\
                     .iter()\n\
                     .map(|(name, instance)| NamedInput { name, instance })\n\
                     .collect::<Vec<_>>();\n\
                 let loader = codegen_runtime::JsonDynamicSourceAdapter::new(\n\
                     loader,\n\
                     dynamic_source_json_schema,\n\
                 );\n\
                 let outputs = match execution {\n\
                     Some(execution) => execute_outputs_with_sources_context_and_dynamic_source_loader(\n\
                         &source,\n\
                         &inputs,\n\
                         execution,\n\
                         &loader,\n\
                     )?,\n\
                     None => execute_outputs_with_sources_and_dynamic_source_loader(\n\
                         &source,\n\
                         &inputs,\n\
                         &loader,\n\
                     )?,\n\
                 };\n\
                 serialize_json_outputs(outputs)\n\
             }\n\n\
             fn dynamic_source_json_schema(source: &str) -> Option<&'static str> {\n\
                 match source {\n",
        );
        for (index, source) in dynamic_sources.iter().enumerate() {
            output.push_str(&format!(
                "        {} => Some(DYNAMIC_SOURCE_JSON_SCHEMAS[{index}]),\n",
                rust_string(&source.name),
            ));
        }
        output.push_str(
            "        _ => None,\n\
                 }\n\
             }\n\n",
        );
    }
    if static_sources.is_empty() {
        output.push_str(
            "fn validate_named_json_input_names(\n\
                 inputs: &[NamedJsonInput<'_>],\n\
             ) -> Result<(), JsonBoundaryError> {\n\
                 if let Some(input) = inputs.first() {\n\
                     return Err(RuntimeError::UnexpectedNamedSource {\n\
                         name: input.name.to_string(),\n\
                     }\n\
                     .into());\n\
                 }\n\
                 Ok(())\n\
             }\n\n\
             fn parse_named_json_inputs<'a>(\n\
                 inputs: &[NamedJsonInput<'a>],\n\
             ) -> Result<Vec<(&'a str, Instance)>, JsonBoundaryError> {\n\
                 let _ = inputs;\n\
                 Ok(Vec::new())\n\
             }\n\n\
             fn validate_named_json_bytes_input_names(\n\
                 inputs: &[NamedJsonBytesInput<'_>],\n\
             ) -> Result<(), JsonBoundaryError> {\n\
                 if let Some(input) = inputs.first() {\n\
                     return Err(RuntimeError::UnexpectedNamedSource {\n\
                         name: input.name.to_string(),\n\
                     }\n\
                     .into());\n\
                 }\n\
                 Ok(())\n\
             }\n\n\
             fn parse_named_json_bytes_inputs<'a>(\n\
                 inputs: &[NamedJsonBytesInput<'a>],\n\
             ) -> Result<Vec<(&'a str, Instance)>, JsonBoundaryError> {\n\
                 let _ = inputs;\n\
                 Ok(Vec::new())\n\
             }\n\n",
        );
    } else {
        output.push_str(
            "fn validate_named_json_input_names(\n\
                 inputs: &[NamedJsonInput<'_>],\n\
             ) -> Result<(), JsonBoundaryError> {\n\
                 let mut matched = vec![false; EXTRA_SOURCE_NAMES.len()];\n\
                 for input in inputs {\n\
                     let index = match input.name {\n",
        );
        for (index, source) in static_sources.iter().enumerate() {
            output.push_str(&format!(
                "            {} => {index},\n",
                rust_string(&source.name)
            ));
        }
        output.push_str(
            "            _ => {\n\
                             return Err(RuntimeError::UnexpectedNamedSource {\n\
                                 name: input.name.to_string(),\n\
                             }\n\
                             .into());\n\
                         }\n\
                     };\n\
                     if matched[index] {\n\
                         return Err(RuntimeError::DuplicateNamedSource {\n\
                             name: EXTRA_SOURCE_NAMES[index],\n\
                         }\n\
                         .into());\n\
                     }\n\
                     matched[index] = true;\n\
                 }\n\
                 for (index, name) in EXTRA_SOURCE_NAMES.iter().copied().enumerate() {\n\
                     if !matched[index] {\n\
                         return Err(RuntimeError::MissingNamedSource { name }.into());\n\
                     }\n\
                 }\n\
                 Ok(())\n\
             }\n\n\
             fn parse_named_json_inputs<'a>(\n\
                 inputs: &[NamedJsonInput<'a>],\n\
             ) -> Result<Vec<(&'a str, Instance)>, JsonBoundaryError> {\n\
                 let mut parsed = Vec::with_capacity(inputs.len());\n\
                 for input in inputs {\n\
                     let schema = match input.name {\n",
        );
        for (index, source) in static_sources.iter().enumerate() {
            output.push_str(&format!(
                "            {} => EXTRA_SOURCE_JSON_SCHEMAS[{index}],\n",
                rust_string(&source.name)
            ));
        }
        output.push_str(
            "            _ => {\n\
                             return Err(RuntimeError::UnexpectedNamedSource {\n\
                                 name: input.name.to_string(),\n\
                             }\n\
                             .into());\n\
                         }\n\
                     };\n\
                     parsed.push((input.name, parse_json(schema, input.document)?));\n\
                 }\n\
                 Ok(parsed)\n\
             }\n\n\
             fn validate_named_json_bytes_input_names(\n\
                 inputs: &[NamedJsonBytesInput<'_>],\n\
             ) -> Result<(), JsonBoundaryError> {\n\
                 let mut matched = vec![false; EXTRA_SOURCE_NAMES.len()];\n\
                 for input in inputs {\n\
                     let index = match input.name {\n",
        );
        for (index, source) in static_sources.iter().enumerate() {
            output.push_str(&format!(
                "            {} => {index},\n",
                rust_string(&source.name)
            ));
        }
        output.push_str(
            "            _ => {\n\
                             return Err(RuntimeError::UnexpectedNamedSource {\n\
                                 name: input.name.to_string(),\n\
                             }\n\
                             .into());\n\
                         }\n\
                     };\n\
                     if matched[index] {\n\
                         return Err(RuntimeError::DuplicateNamedSource {\n\
                             name: EXTRA_SOURCE_NAMES[index],\n\
                         }\n\
                         .into());\n\
                     }\n\
                     matched[index] = true;\n\
                 }\n\
                 for (index, name) in EXTRA_SOURCE_NAMES.iter().copied().enumerate() {\n\
                     if !matched[index] {\n\
                         return Err(RuntimeError::MissingNamedSource { name }.into());\n\
                     }\n\
                 }\n\
                 Ok(())\n\
             }\n\n\
             fn parse_named_json_bytes_inputs<'a>(\n\
                 inputs: &[NamedJsonBytesInput<'a>],\n\
             ) -> Result<Vec<(&'a str, Instance)>, JsonBoundaryError> {\n\
                 let mut parsed = Vec::with_capacity(inputs.len());\n\
                 for input in inputs {\n\
                     let schema = match input.name {\n",
        );
        for (index, source) in static_sources.iter().enumerate() {
            output.push_str(&format!(
                "            {} => EXTRA_SOURCE_JSON_SCHEMAS[{index}],\n",
                rust_string(&source.name)
            ));
        }
        output.push_str(
            "            _ => {\n\
                             return Err(RuntimeError::UnexpectedNamedSource {\n\
                                 name: input.name.to_string(),\n\
                             }\n\
                             .into());\n\
                         }\n\
                     };\n\
                     parsed.push((input.name, parse_json_bytes(schema, input.document)?));\n\
                 }\n\
                 Ok(parsed)\n\
             }\n\n",
        );
    }
    output.push_str(
        "fn serialize_json_outputs(\n\
             outputs: ExecutionOutputs,\n\
         ) -> Result<JsonExecutionOutputs, JsonBoundaryError> {\n\
             if outputs.extras.len() != EXTRA_TARGET_JSON_SCHEMAS.len() {\n\
                 return Err(JsonBoundaryError::InvalidOutput {\n\
                     message: \"generated mapping returned an unexpected number of named targets\".to_string(),\n\
                 });\n\
             }\n\
             let primary = serialize_json(TARGET_JSON_SCHEMA, &outputs.primary)?;\n\
             let extras = outputs\n\
                 .extras\n\
                 .into_iter()\n\
                 .zip(EXTRA_TARGET_JSON_SCHEMAS.iter().copied())\n\
                 .map(|(output, schema)| {\n\
                     serialize_json(schema, &output.instance).map(|document| NamedJsonOutput {\n\
                         name: output.name,\n\
                         document,\n\
                     })\n\
                 })\n\
                 .collect::<Result<Vec<_>, _>>()?;\n\
             Ok(JsonExecutionOutputs { primary, extras })\n\
         }\n\n\
         fn serialize_json_bytes_outputs(\n\
             outputs: ExecutionOutputs,\n\
         ) -> Result<JsonBytesExecutionOutputs, JsonBoundaryError> {\n\
             if outputs.extras.len() != EXTRA_TARGET_JSON_SCHEMAS.len() {\n\
                 return Err(JsonBoundaryError::InvalidOutput {\n\
                     message: \"generated mapping returned an unexpected number of named targets\".to_string(),\n\
                 });\n\
             }\n\
             let primary = serialize_json_bytes(TARGET_JSON_SCHEMA, &outputs.primary)?;\n\
             let extras = outputs\n\
                 .extras\n\
                 .into_iter()\n\
                 .zip(EXTRA_TARGET_JSON_SCHEMAS.iter().copied())\n\
                 .map(|(output, schema)| {\n\
                     serialize_json_bytes(schema, &output.instance).map(|document| NamedJsonBytesOutput {\n\
                         name: output.name,\n\
                         document,\n\
                     })\n\
                 })\n\
                 .collect::<Result<Vec<_>, _>>()?;\n\
             Ok(JsonBytesExecutionOutputs { primary, extras })\n\
         }\n\n",
    );
    output.push_str(
        "pub fn execute_json_outputs_with_filter_map_controls(\n\
             source: &str,\n\
             inputs: &[NamedJsonInput<'_>],\n\
             execution: Option<&ExecutionContext<'_>>,\n\
             limits: FilterMapLimits,\n\
             cancellation: Option<&dyn FilterMapCancellation>,\n\
         ) -> Result<FilterMapExecution<JsonExecutionOutputs, JsonBoundaryError>, JsonBoundaryError> {\n\
             validate_named_json_input_names(inputs)?;\n\
             let source = parse_json(SOURCE_JSON_SCHEMA, source)?;\n\
             let parsed = parse_named_json_inputs(inputs)?;\n\
             let inputs = parsed.iter().map(|(name, instance)| NamedInput { name, instance }).collect::<Vec<_>>();\n\
             let actual = execute_outputs_with_filter_map_controls(&source, &inputs, execution, None, limits, cancellation)?;\n\
             Ok(FilterMapExecution { counters: actual.counters, outcome: actual.outcome.map_err(JsonBoundaryError::from).and_then(serialize_json_outputs) })\n\
         }\n\n\
         pub fn execute_json_bytes_outputs_with_filter_map_controls(\n\
             source: &[u8],\n\
             inputs: &[NamedJsonBytesInput<'_>],\n\
             execution: Option<&ExecutionContext<'_>>,\n\
             limits: FilterMapLimits,\n\
             cancellation: Option<&dyn FilterMapCancellation>,\n\
         ) -> Result<FilterMapExecution<JsonBytesExecutionOutputs, JsonBoundaryError>, JsonBoundaryError> {\n\
             validate_named_json_bytes_input_names(inputs)?;\n\
             let source = parse_json_bytes(SOURCE_JSON_SCHEMA, source)?;\n\
             let parsed = parse_named_json_bytes_inputs(inputs)?;\n\
             let inputs = parsed.iter().map(|(name, instance)| NamedInput { name, instance }).collect::<Vec<_>>();\n\
             let actual = execute_outputs_with_filter_map_controls(&source, &inputs, execution, None, limits, cancellation)?;\n\
             Ok(FilterMapExecution { counters: actual.counters, outcome: actual.outcome.map_err(JsonBoundaryError::from).and_then(serialize_json_bytes_outputs) })\n\
         }\n\n",
    );
    Ok(output)
}
