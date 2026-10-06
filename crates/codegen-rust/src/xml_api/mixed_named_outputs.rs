use super::output_arguments;
use crate::{EmitError, rust_string};
use codegen::{Program, ProgramValidationError, XmlOutputMode};

const TYPES: &str = r#"
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlMixedDocumentOutput { pub path: String, pub document: String }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlMixedBytesDocumentOutput { pub path: String, pub document: Vec<u8> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedXmlMixedOutput {
    SingleDocument { declaration_index: usize, name: &'static str, document: String },
    DocumentList { declaration_index: usize, name: &'static str, documents: Vec<XmlMixedDocumentOutput> },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedXmlMixedBytesOutput {
    SingleDocument { declaration_index: usize, name: &'static str, document: Vec<u8> },
    DocumentList { declaration_index: usize, name: &'static str, documents: Vec<XmlMixedBytesDocumentOutput> },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlMixedExecutionOutputs { pub primary: String, pub extras: Vec<NamedXmlMixedOutput> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlMixedBytesExecutionOutputs { pub primary: Vec<u8>, pub extras: Vec<NamedXmlMixedBytesOutput> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlMixedOutputOwner {
    Primary,
    Named { declaration_index: usize, name: String },
    Member { declaration_index: usize, name: String, member_index: usize, path: String },
}
#[derive(Debug)]
pub struct XmlMixedExecutionError {
    pub owner: Option<XmlMixedOutputOwner>,
    pub boundary: Box<codegen_runtime::XmlBoundaryError>,
}
impl From<codegen_runtime::XmlBoundaryError> for XmlMixedExecutionError {
    fn from(boundary: codegen_runtime::XmlBoundaryError) -> Self {
        Self { owner: None, boundary: Box::new(boundary) }
    }
}
impl std::fmt::Display for XmlMixedExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.owner {
            Some(XmlMixedOutputOwner::Primary) => write!(f, "primary XML output: ")?,
            Some(XmlMixedOutputOwner::Named { declaration_index, name }) => {
                write!(f, "named XML output {declaration_index} `{name}`: ")?;
            }
            Some(XmlMixedOutputOwner::Member { declaration_index, name, member_index, path }) => {
                write!(f, "named XML output {declaration_index} `{name}` member {member_index} `{path}`: ")?;
            }
            None => {}
        }
        std::fmt::Display::fmt(self.boundary.as_ref(), f)
    }
}
impl std::error::Error for XmlMixedExecutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}
fn mixed_xml_alignment(detail: &str) -> XmlMixedExecutionError {
    codegen_runtime::XmlOutputSetError::alignment(detail).into_boundary().into()
}
fn mixed_xml_error(owner: XmlMixedOutputOwner, boundary: codegen_runtime::XmlBoundaryError) -> XmlMixedExecutionError {
    let owner = (boundary.kind != codegen_runtime::XmlBoundaryErrorKind::Schema).then_some(owner);
    XmlMixedExecutionError { owner, boundary: Box::new(boundary) }
}

"#;

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    if program.xml_output_mode()? != Some(XmlOutputMode::StaticPrimaryMixedNamedXmlOutputs) {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "mixed named XML renderer requires its exact validated mode".into(),
        }
        .into());
    }
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "missing mixed XML boundary".into(),
        }
    })?;
    let source = codegen::serialize_embedded_schema(
        &program.source,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let target = codegen::serialize_embedded_schema(
        &program.target,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let mut output = format!(
        "\nconst SOURCE_XML_SCHEMA: &str = {};\nconst TARGET_XML_SCHEMA: &str = {};\n",
        rust_string(&source),
        rust_string(&target),
    );
    for (index, named) in program.extra_targets.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &named.target,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "const MIXED_XML_SCHEMA_{index}: &str = {};\nconst MIXED_XML_NAME_{index}: &str = {};\n",
            rust_string(&descriptor), rust_string(&named.name),
        ));
    }
    output.push_str(TYPES);
    let dynamic_index = program
        .extra_targets
        .iter()
        .position(|target| {
            target
                .root
                .iteration
                .as_ref()
                .is_some_and(|iteration| iteration.dynamic_document_iteration().is_some())
        })
        .ok_or_else(|| ProgramValidationError::InvalidXmlBoundary {
            reason: "missing mixed XML document list".into(),
        })?;
    let primary_arguments = output_arguments(&policy.output)?;
    for bytes in [false, true] {
        let (stem, source_type, parser, result, named_dto, member_dto, helper, conversion) =
            if bytes {
                (
                    "execute_xml_bytes_mixed_outputs",
                    "&[u8]",
                    "parse_structured_xml_bytes",
                    "XmlMixedBytesExecutionOutputs",
                    "NamedXmlMixedBytesOutput",
                    "XmlMixedBytesDocumentOutput",
                    "serialize_mixed_xml_bytes_outputs",
                    "xml.into_bytes()",
                )
            } else {
                (
                    "execute_xml_mixed_outputs",
                    "&str",
                    "parse_structured_xml",
                    "XmlMixedExecutionOutputs",
                    "NamedXmlMixedOutput",
                    "XmlMixedDocumentOutput",
                    "serialize_mixed_xml_outputs",
                    "xml",
                )
            };
        for context in [false, true] {
            let suffix = if context { "_with_context" } else { "" };
            let context_argument = if context {
                ", execution: &codegen_runtime::ExecutionContext<'_>"
            } else {
                ""
            };
            let context_call = if context { ", execution" } else { "" };
            let execute = if context {
                "execute_outputs_with_context"
            } else {
                "execute_outputs"
            };
            output.push_str(&format!(r#"pub fn {stem}{suffix}(source: {source_type}{context_argument}) -> Result<{result}, XmlMixedExecutionError> {{
    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source).map_err(XmlMixedExecutionError::from)?;
    let mapped = {execute}(&parsed{context_call}).map_err(codegen_runtime::XmlBoundaryError::from).map_err(XmlMixedExecutionError::from)?;
    {helper}(mapped)
}}

"#));
        }
        output.push_str(&format!(
            r#"fn {helper}(mapped: ExecutionOutputs) -> Result<{result}, XmlMixedExecutionError> {{
    if !matches!(&mapped.primary, Instance::Group(_)) {{
        return Err(mixed_xml_alignment("mixed XML outputs require a primary Group"));
    }}
    if mapped.extras.len() != 2 {{
        return Err(mixed_xml_alignment("mixed XML outputs require every declared named target"));
    }}
    let mut named_targets = mapped.extras.into_iter();
"#
        ));
        // Align every envelope and its actual cardinality before any writer.
        for index in 0..2 {
            output.push_str(&format!(r#"    let named_{index} = named_targets.next().ok_or_else(|| mixed_xml_alignment("mixed XML outputs require every named envelope"))?;
    if named_{index}.name != MIXED_XML_NAME_{index} {{
        return Err(mixed_xml_alignment("mixed XML outputs do not match exact declaration order"));
    }}
"#));
            if index == dynamic_index {
                output.push_str(&format!(r#"    let Instance::DocumentSet(members_{index}) = named_{index}.instance else {{
        return Err(mixed_xml_alignment("mixed XML document-list output requires a DocumentSet"));
    }};
"#));
            } else {
                output.push_str(&format!(
                    r#"    if !matches!(&named_{index}.instance, Instance::Group(_)) {{
        return Err(mixed_xml_alignment("mixed XML single-document output requires a Group"));
    }}
"#
                ));
            }
        }
        output.push_str(&format!(r#"    let artifact_count = members_{dynamic_index}.len().checked_add(2).ok_or_else(|| mixed_xml_alignment("mixed XML output count exceeds the host index range"))?;
    let mut budget = codegen_runtime::XmlOutputSetBudget::new(artifact_count).map_err(|error| XmlMixedExecutionError::from(error.into_boundary()))?;
    let xml = codegen_runtime::serialize_xml_document(TARGET_XML_SCHEMA, &mapped.primary, {primary_arguments}).map_err(|error| mixed_xml_error(XmlMixedOutputOwner::Primary, error))?;
    budget.charge(codegen_runtime::XmlOutputTarget::Primary, xml.len()).map_err(|error| mixed_xml_error(XmlMixedOutputOwner::Primary, error.into_boundary()))?;
    let primary = {conversion};
    let mut extras = Vec::with_capacity(2);
"#));
        for (index, named_policy) in policy.extra_outputs.iter().enumerate() {
            let arguments = output_arguments(&named_policy.output)?;
            if index == dynamic_index {
                output.push_str(&format!(r#"    let mut documents = Vec::with_capacity(members_{index}.len());
    for (member_index, member) in members_{index}.into_iter().enumerate() {{
        let xml = codegen_runtime::serialize_xml_document(MIXED_XML_SCHEMA_{index}, member.value(), {arguments}).map_err(|error| mixed_xml_error(XmlMixedOutputOwner::Member {{ declaration_index: {index}, name: MIXED_XML_NAME_{index}.to_owned(), member_index, path: member.path().to_owned() }}, error))?;
        budget.charge(codegen_runtime::XmlOutputTarget::Named {{ index: {index}, name: MIXED_XML_NAME_{index} }}, xml.len()).map_err(|error| mixed_xml_error(XmlMixedOutputOwner::Member {{ declaration_index: {index}, name: MIXED_XML_NAME_{index}.to_owned(), member_index, path: member.path().to_owned() }}, error.into_boundary()))?;
        documents.push({member_dto} {{ path: member.path().to_owned(), document: {conversion} }});
    }}
    extras.push({named_dto}::DocumentList {{ declaration_index: {index}, name: MIXED_XML_NAME_{index}, documents }});
"#));
            } else {
                output.push_str(&format!(r#"    let xml = codegen_runtime::serialize_xml_document(MIXED_XML_SCHEMA_{index}, &named_{index}.instance, {arguments}).map_err(|error| mixed_xml_error(XmlMixedOutputOwner::Named {{ declaration_index: {index}, name: MIXED_XML_NAME_{index}.to_owned() }}, error))?;
    budget.charge(codegen_runtime::XmlOutputTarget::Named {{ index: {index}, name: MIXED_XML_NAME_{index} }}, xml.len()).map_err(|error| mixed_xml_error(XmlMixedOutputOwner::Named {{ declaration_index: {index}, name: MIXED_XML_NAME_{index}.to_owned() }}, error.into_boundary()))?;
    extras.push({named_dto}::SingleDocument {{ declaration_index: {index}, name: MIXED_XML_NAME_{index}, document: {conversion} }});
"#));
            }
        }
        output.push_str(&format!("    Ok({result} {{ primary, extras }})\n}}\n\n"));
    }
    Ok(output)
}
