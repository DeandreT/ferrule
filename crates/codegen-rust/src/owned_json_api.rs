//! Strict compact JSON companions for control-free Basic owned-source emission.

use codegen::{Program, serialize_embedded_schema};

use super::{EmitError, rust_string};

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    let schema = |node| {
        serialize_embedded_schema(node, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
            .map(|schema| rust_string(&schema))
            .map_err(EmitError::from)
    };
    let source_schema = schema(&program.source)?;
    let target_schema = schema(&program.target)?;
    // Admission returns original declaration ordinals, including unavailable
    // declarations. Never filter or reorder the embedded source descriptors.
    let source_schemas = program
        .extra_sources
        .iter()
        .map(|source| schema(&source.source))
        .collect::<Result<Vec<_>, _>>()?;
    let target_schemas = program
        .extra_targets
        .iter()
        .map(|target| {
            schema(&target.target)
                .map(|schema| format!("({}, {schema})", rust_string(&target.name)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut output = String::from(
        r#"
// JSON admission keeps borrowed documents until metadata checks have completed.
use codegen_runtime::{BasicJsonDocumentRef, BasicNamedJsonDocumentRef,
    JsonBoundaryError, admit_basic_json_inputs, parse_json,
    parse_json_bytes_with_utf8_cause, serialize_json_compact, serialize_json_compact_bytes};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedJsonInput<'a> {
    pub name: &'a str,
    pub document: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedJsonBytesInput<'a> {
    pub name: &'a str,
    pub document: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedJsonOutput {
    pub name: &'static str,
    pub document: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedJsonBytesOutput {
    pub name: &'static str,
    pub document: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonExecutionOutputs {
    pub primary: String,
    pub extras: Vec<NamedJsonOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonBytesExecutionOutputs {
    pub primary: Vec<u8>,
    pub extras: Vec<NamedJsonBytesOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectedJsonTargetOutput {
    Primary(String),
    Named(NamedJsonOutput),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectedJsonBytesTargetOutput {
    Primary(Vec<u8>),
    Named(NamedJsonBytesOutput),
}

"#,
    );
    output.push_str(&format!(
        "const BASIC_OWNED_SOURCE_JSON_SCHEMA: &str = {source_schema};\n\
         const BASIC_OWNED_TARGET_JSON_SCHEMA: &str = {target_schema};\n\
         const BASIC_OWNED_EXTRA_SOURCE_JSON_SCHEMAS: &[&str] = &[{}];\n\
         const BASIC_OWNED_EXTRA_TARGET_JSON_DESCRIPTORS: &[(&str, &str)] = &[{}];\n\n",
        source_schemas.join(", "),
        target_schemas.join(", "),
    ));
    output.push_str(
        r#"fn parse_basic_owned_named_json_document(
    declaration_index: usize,
    document: BasicJsonDocumentRef<'_>,
) -> Result<Instance, JsonBoundaryError> {
    let schema = BASIC_OWNED_EXTRA_SOURCE_JSON_SCHEMAS[declaration_index];
    match document {
        BasicJsonDocumentRef::Text(document) => parse_json(schema, document),
        BasicJsonDocumentRef::Utf8(document) => parse_json_bytes_with_utf8_cause(schema, document),
    }
}

"#,
    );
    for bytes in [false, true] {
        let (
            prefix,
            source,
            input,
            document_kind,
            parse,
            result,
            selected_result,
            named,
            serialize,
        ) = if bytes {
            (
                "execute_json_bytes",
                "&[u8]",
                "NamedJsonBytesInput",
                "Utf8",
                "parse_json_bytes_with_utf8_cause",
                "JsonBytesExecutionOutputs",
                "SelectedJsonBytesTargetOutput",
                "NamedJsonBytesOutput",
                "serialize_json_compact_bytes",
            )
        } else {
            (
                "execute_json",
                "&str",
                "NamedJsonInput",
                "Text",
                "parse_json",
                "JsonExecutionOutputs",
                "SelectedJsonTargetOutput",
                "NamedJsonOutput",
                "serialize_json_compact",
            )
        };
        for selected in [false, true] {
            let (suffix, returned) = if selected {
                ("selected_target", selected_result)
            } else {
                ("outputs", result)
            };
            output.push_str(&format!(
                "/// Execute strict JSON with an explicit policy and metadata-first admission.\n\
                 pub fn {prefix}_{suffix}_with_basic_owned_sources(\n\
                     source: {source},\n\
                     inputs: &[{input}<'_>],\n\
                     execution: Option<&ExecutionContext<'_>>,\n\
                     limits: OwnedSourceLimits,\n"
            ));
            if selected {
                output.push_str("    selection: TargetSelection<'_>,\n");
            }
            output.push_str(&format!(") -> Result<{returned}, JsonBoundaryError> {{\n"));
            if selected {
                output.push_str("    let (selected, catalogue) = match selection {\n        TargetSelection::Primary => (0, PRIVATE_CHECKED_PRIMARY_INPUTS),\n        TargetSelection::Named(name) => match name {\n");
                for (index, target) in program.extra_targets.iter().enumerate() {
                    output.push_str(&format!(
                        "            {} => ({}, PRIVATE_CHECKED_NAMED_INPUTS_{index}),\n",
                        rust_string(&target.name),
                        index + 1,
                    ));
                }
                output.push_str("            _ => return Err(RuntimeError::UnknownTarget { name: name.to_string() }.into()),\n        },\n    };\n");
            } else {
                output.push_str("    let catalogue = PRIVATE_CHECKED_ALL_INPUTS;\n");
            }
            output.push_str(&format!(
                "    let mut supplied = inputs.iter().map(|input| BasicNamedJsonDocumentRef {{\n\
                         name: input.name, document: BasicJsonDocumentRef::{document_kind}(input.document),\n\
                     }});\n\
                     let admission = admit_basic_json_inputs(catalogue, &mut supplied, limits)?;\n\
                     let source = {parse}(BASIC_OWNED_SOURCE_JSON_SCHEMA, source)?;\n\
                     let mut parsed = admission.parse_named_with(&mut parse_basic_owned_named_json_document)?;\n\
                     let roots = parsed.basic_roots(&source)?;\n\
                     let context = match execution {{\n\
                         Some(execution) => roots.root_context_with_execution(execution)?,\n\
                         None => roots.root_context()?,\n\
                     }};\n"
            ));
            let (variant, selection, helper) = if selected {
                (
                    "Selected",
                    "Some(selected)",
                    format!("serialize_basic_owned_selected_{prefix}(output, selected)"),
                )
            } else {
                (
                    "All",
                    "None",
                    format!("serialize_basic_owned_{prefix}_outputs(output)"),
                )
            };
            output.push_str(&format!(
                "    let output = match execute_private_checked_basic_context(&context, {selection})? {{\n\
                         PrivateCheckedBasicOutput::{variant}(output) => output,\n\
                         _ => unreachable!(\"checked JSON output route\"),\n\
                     }};\n\
                     {helper}\n\
                 }}\n\n"
            ));
        }
        output.push_str(&format!(
            "fn serialize_basic_owned_{prefix}_outputs(\n\
                 outputs: ExecutionOutputs,\n\
             ) -> Result<{result}, JsonBoundaryError> {{\n\
                 if outputs.extras.len() != BASIC_OWNED_EXTRA_TARGET_JSON_DESCRIPTORS.len() {{\n\
                     return Err(JsonBoundaryError::InvalidOutput {{ message: \"generated mapping returned an unexpected number of named targets\".to_string() }});\n\
                 }}\n\
                 let primary = {serialize}(BASIC_OWNED_TARGET_JSON_SCHEMA, &outputs.primary)?;\n\
                 let extras = outputs.extras.into_iter()\n\
                     .zip(BASIC_OWNED_EXTRA_TARGET_JSON_DESCRIPTORS.iter().copied())\n\
                     .map(|(output, (name, schema))| {{\n\
                         if output.name != name {{\n\
                             return Err(JsonBoundaryError::InvalidOutput {{ message: \"generated mapping returned a target that does not match its declaration\".to_string() }});\n\
                         }}\n\
                         {serialize}(schema, &output.instance).map(|document| {named} {{ name, document }})\n\
                     }})\n\
                     .collect::<Result<Vec<_>, _>>()?;\n\
                 Ok({result} {{ primary, extras }})\n\
             }}\n\n\
             fn serialize_basic_owned_selected_{prefix}(\n\
                 output: SelectedTargetOutput, selected: usize,\n\
             ) -> Result<{selected_result}, JsonBoundaryError> {{\n\
                 match (selected, output) {{\n\
                     (0, SelectedTargetOutput::Primary(instance)) => Ok({selected_result}::Primary({serialize}(BASIC_OWNED_TARGET_JSON_SCHEMA, &instance)?)),\n"
        ));
        for (index, target) in program.extra_targets.iter().enumerate() {
            let name = rust_string(&target.name);
            output.push_str(&format!(
                "        ({}, SelectedTargetOutput::Named(output)) if output.name == {name} => Ok({selected_result}::Named({named} {{ name: {name}, document: {serialize}(BASIC_OWNED_EXTRA_TARGET_JSON_DESCRIPTORS[{index}].1, &output.instance)? }})),\n",
                index + 1,
            ));
        }
        output.push_str("        _ => Err(JsonBoundaryError::InvalidOutput { message: \"generated mapping returned a target that does not match the resolved selection\".to_string() }),\n    }\n}\n\n");
    }
    Ok(output)
}
