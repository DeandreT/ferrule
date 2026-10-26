//! Isolated physical XML output over the existing selected typed executor.
use crate::{EmitError, rust_string};
use codegen::{NamedInputAvailability, Program, XmlEmissionPlan};

pub(super) fn render(program: &Program, plan: &XmlEmissionPlan<'_>) -> Result<String, EmitError> {
    let Some(admission) = plan.selected_inputs() else {
        return Ok(String::new());
    };
    let mut output = String::from(
        r#"
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectedXmlBytesTargetOutput {
    Primary(Vec<u8>),
    Named(NamedXmlBytesOutput),
}

pub fn execute_xml_bytes_selected_target(source: &[u8], selection: TargetSelection<'_>)
    -> Result<SelectedXmlBytesTargetOutput, codegen_runtime::XmlExecutionError>
{
    execute_xml_bytes_selected_target_with_host(source, &[], None, selection)
}

pub fn execute_xml_bytes_selected_target_with_host(
    source: &[u8], inputs: &[NamedXmlBytesInput<'_>],
    execution: Option<&ExecutionContext<'_>>, selection: TargetSelection<'_>,
) -> Result<SelectedXmlBytesTargetOutput, codegen_runtime::XmlExecutionError> {
    let selected = resolve_target(selection).map_err(codegen_runtime::XmlBoundaryError::from)?;
    let expected: &[&'static str] = match selected {
"#,
    );
    for (index, catalogue) in std::iter::once(&admission.primary)
        .chain(
            admission
                .named_targets
                .iter()
                .map(|target| &target.catalogue),
        )
        .enumerate()
    {
        let names = catalogue
            .entries
            .iter()
            .filter(|entry| entry.availability == NamedInputAvailability::RequiredLegacy)
            .map(|entry| rust_string(entry.name))
            .collect::<Vec<_>>()
            .join(", ");
        let variant = if index == 0 {
            "Primary".into()
        } else {
            format!("Named{}", index - 1)
        };
        output.push_str(&format!(
            "        ResolvedTarget::{variant} => &[{names}],\n"
        ));
    }
    output.push_str(r#"    };
    let _count = codegen_runtime::XmlInputSetBudget::new(inputs.len().saturating_add(1))?;
    let names: Vec<&str> = inputs.iter().map(|input| input.name).collect();
    let indices = codegen_runtime::xml_input_indices(expected, &names)?;
    let mut sizes = vec![(codegen_runtime::XmlInputSource::Primary, source.len())];
    for (i, name) in expected.iter().enumerate() {
        sizes.push((selected_xml_input_owner(name), inputs[indices[i]].document.len()));
    }
    codegen_runtime::preflight_xml_input_sizes(&sizes)?;
    let primary = codegen_runtime::parse_structured_xml_bytes(SOURCE_XML_SCHEMA, source)
        .map_err(|error| codegen_runtime::XmlExecutionError::input(codegen_runtime::XmlInputSource::Primary, error))?;
    let mut parsed = Vec::with_capacity(expected.len());
    for (i, name) in expected.iter().enumerate() {
        let value = codegen_runtime::parse_structured_xml_bytes(selected_xml_input_schema(name), inputs[indices[i]].document)
            .map_err(|error| codegen_runtime::XmlExecutionError::input(selected_xml_input_owner(name), error))?;
        parsed.push((*name, value));
    }
    let fields: Vec<NamedInput<'_>> = parsed.iter().map(|(name, instance)| NamedInput { name, instance }).collect();
    let context = match execution {
        Some(execution) => ScopeContext::with_named_inputs_and_execution_context(&primary, &fields, execution),
        None => ScopeContext::with_named_inputs(&primary, &fields),
    };
    let mapped = execute_selected_target_from_context(&context, selected)
        .map_err(codegen_runtime::XmlBoundaryError::from)?;
    let mut budget = codegen_runtime::XmlOutputSetBudget::new(1)?;
    match (selected, mapped) {
"#);
    for (i, (schema, policy, name)) in
        std::iter::once(("TARGET_XML_SCHEMA".to_string(), &plan.policy().output, None))
            .chain(program.extra_targets.iter().enumerate().map(|(i, target)| {
                (
                    format!("EXTRA_XML_SCHEMA_{i}"),
                    &plan.policy().extra_outputs[i].output,
                    Some(target.name.as_str()),
                )
            }))
            .enumerate()
    {
        let args = super::output_arguments(policy)?;
        let (pattern, instance, owner, result) = match name {
            None => (
                "(ResolvedTarget::Primary, SelectedTargetOutput::Primary(value))".to_string(),
                "&value",
                "codegen_runtime::XmlOutputTarget::Primary".to_string(),
                "SelectedXmlBytesTargetOutput::Primary(xml.into_bytes())".to_string(),
            ),
            Some(name) => {
                let name = rust_string(name);
                (
                    format!(
                        "(ResolvedTarget::Named{}, SelectedTargetOutput::Named(value)) if value.name == {name}",
                        i - 1
                    ),
                    "&value.instance",
                    format!(
                        "codegen_runtime::XmlOutputTarget::Named {{ index: {}, name: {name} }}",
                        i - 1
                    ),
                    format!(
                        "SelectedXmlBytesTargetOutput::Named(NamedXmlBytesOutput {{ name: {name}, document: xml.into_bytes() }})"
                    ),
                )
            }
        };
        output.push_str(&format!("        {pattern} => {{\n            let owner = {owner};\n            let xml = codegen_runtime::serialize_xml_document({schema}, {instance}, {args})\n                .map_err(|error| codegen_runtime::XmlOutputSetError::new(Some(owner), error))?;\n            budget.charge(owner, xml.len())?;\n            Ok({result})\n        }}\n"));
    }
    output.push_str("        _ => Err(codegen_runtime::XmlOutputSetError::alignment(\"XML selected output does not match its target\").into()),\n");
    output.push_str("    }\n}\n\nfn selected_xml_input_owner(name: &str) -> codegen_runtime::XmlInputSource {\n    match name {\n");
    for (i, source) in program.extra_sources.iter().enumerate() {
        let name = rust_string(&source.name);
        output.push_str(&format!("        {name} => codegen_runtime::XmlInputSource::Named {{ index: {i}, name: {name} }},\n"));
    }
    output.push_str("        _ => unreachable!(\"validated selected XML input name\"),\n    }\n}\nfn selected_xml_input_schema(name: &str) -> &'static str {\n    match name {\n");
    for (i, source) in program.extra_sources.iter().enumerate() {
        output.push_str(&format!(
            "        {} => EXTRA_XML_INPUT_SCHEMA_{i},\n",
            rust_string(&source.name)
        ));
    }
    output
        .push_str("        _ => unreachable!(\"validated selected XML input name\"),\n    }\n}\n");
    Ok(output)
}
