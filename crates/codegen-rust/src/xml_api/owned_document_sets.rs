//! Owned XML document-set entry points. Every endpoint arrives as an ordered
//! member list; names are admitted like the typed entry points before any
//! member is read, and the parsed sets run through the same checked factory.
use crate::{EmitError, rust_string};
use codegen::Program;

pub(crate) fn render(program: &Program) -> Result<String, EmitError> {
    let schema = |node| {
        codegen::serialize_embedded_schema(node, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)
            .map(|schema| rust_string(&schema))
            .map_err(EmitError::from)
    };
    let extras = program
        .extra_sources
        .iter()
        .map(|source| {
            Ok(format!(
                "({}, {})",
                rust_string(&source.name),
                schema(&source.source)?
            ))
        })
        .collect::<Result<Vec<_>, EmitError>>()?;
    let mut output = format!(
        r#"
// Owned XML document-set entry points: each endpoint is an ordered member list
// read through the structured XML profile, keeping every member's path and
// resolved location.
pub use codegen_runtime::{{NamedXmlDocumentSet, XmlDocumentSetInputError, XmlDocumentSetMember}};

const BASIC_OWNED_SOURCE_XML_SCHEMA: &str = {};
const BASIC_OWNED_EXTRA_SOURCE_XML_SCHEMAS: &[(&str, &str)] = &[{}];

type ParsedXmlDocumentSets<'s> = (Instance, Vec<(&'s str, Instance)>);

// Names first, then every member's shape and size, then each set in order:
// the primary, then the supplied named sets.
fn parse_basic_owned_xml_document_sets<'s, D: codegen_runtime::XmlDocumentSetText + ?Sized>(
    catalogue: &'static [codegen_runtime::BasicNamedSourceDeclaration],
    source: &'s [XmlDocumentSetMember<'s, D>],
    inputs: &'s [NamedXmlDocumentSet<'s, D>],
    limits: OwnedSourceLimits,
) -> Result<ParsedXmlDocumentSets<'s>, XmlDocumentSetInputError> {{
    codegen_runtime::admit_xml_document_set_names(catalogue, inputs.iter().map(|input| input.name), limits)?;
    let mut sets = Vec::with_capacity(inputs.len().saturating_add(1));
    sets.push((codegen_runtime::XmlInputSource::Primary, source));
    let mut schemas = Vec::with_capacity(inputs.len());
    for input in inputs {{
        let Some(index) = BASIC_OWNED_EXTRA_SOURCE_XML_SCHEMAS.iter().position(|(name, _)| *name == input.name) else {{
            return Err(RuntimeError::UnexpectedNamedSource {{ name: input.name.to_string() }}.into());
        }};
        let (name, schema) = BASIC_OWNED_EXTRA_SOURCE_XML_SCHEMAS[index];
        sets.push((codegen_runtime::XmlInputSource::Named {{ index, name }}, input.members));
        schemas.push(schema);
    }}
    codegen_runtime::preflight_xml_document_sets(&sets)?;
    let primary = codegen_runtime::parse_xml_document_set(BASIC_OWNED_SOURCE_XML_SCHEMA, codegen_runtime::XmlInputSource::Primary, source)?;
    let mut named = Vec::with_capacity(inputs.len());
    for ((input, (owner, members)), schema) in inputs.iter().zip(&sets[1..]).zip(schemas) {{
        named.push((input.name, codegen_runtime::parse_xml_document_set(schema, *owner, members)?));
    }}
    Ok((primary, named))
}}

fn basic_owned_xml_document_set_inputs<'s>(named: &'s [(&'s str, Instance)]) -> Vec<BasicNamedSourceInput<'s, 's>> {{
    named.iter().map(|(name, instance)| BasicNamedSourceInput {{ name, instance }}).collect()
}}

fn basic_owned_xml_document_set_catalogue(
    selection: TargetSelection<'_>,
) -> Result<&'static [codegen_runtime::BasicNamedSourceDeclaration], XmlDocumentSetInputError> {{
    match selection {{
        TargetSelection::Primary => Ok(PRIVATE_CHECKED_PRIMARY_INPUTS),
        TargetSelection::Named(name) => match name {{
"#,
        schema(&program.source)?,
        extras.join(", "),
    );
    for (index, target) in program.extra_targets.iter().enumerate() {
        output.push_str(&format!(
            "            {} => Ok(PRIVATE_CHECKED_NAMED_INPUTS_{index}),\n",
            rust_string(&target.name)
        ));
    }
    output.push_str("            _ => Err(RuntimeError::UnknownTarget { name: name.to_string() }.into()),\n        },\n    }\n}\n\n");
    for bytes in [false, true] {
        let (infix, document) = if bytes {
            ("xml_bytes", "[u8]")
        } else {
            ("xml", "str")
        };
        output.push_str(&format!(
            r#"/// Execute every target from XML document sets under an explicit owned-source policy.
pub fn execute_{infix}_document_sets_with_basic_owned_sources(
    source: &[XmlDocumentSetMember<'_, {document}>],
    inputs: &[NamedXmlDocumentSet<'_, {document}>],
    execution: Option<&ExecutionContext<'_>>,
    limits: OwnedSourceLimits,
) -> Result<ExecutionOutputs, XmlDocumentSetInputError> {{
    let (primary, named) = parse_basic_owned_xml_document_sets(PRIVATE_CHECKED_ALL_INPUTS, source, inputs, limits)?;
    let supplied = basic_owned_xml_document_set_inputs(&named);
    Ok(execute_checked_private_owned_basic(&primary, &supplied, execution, limits)?)
}}

/// Validate the selected target, then execute it from XML document sets.
pub fn execute_selected_{infix}_document_sets_with_basic_owned_sources(
    source: &[XmlDocumentSetMember<'_, {document}>],
    inputs: &[NamedXmlDocumentSet<'_, {document}>],
    execution: Option<&ExecutionContext<'_>>,
    limits: OwnedSourceLimits,
    selection: TargetSelection<'_>,
) -> Result<SelectedTargetOutput, XmlDocumentSetInputError> {{
    let catalogue = basic_owned_xml_document_set_catalogue(selection)?;
    let (primary, named) = parse_basic_owned_xml_document_sets(catalogue, source, inputs, limits)?;
    let supplied = basic_owned_xml_document_set_inputs(&named);
    Ok(execute_selected_checked_private_owned_basic(&primary, &supplied, execution, limits, selection)?)
}}

"#
        ));
    }
    Ok(output)
}
