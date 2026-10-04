//! Explicit primary XML document boundaries. JSON codecs never invent XML origins.

mod namespace;

use std::fmt;

use format_xml::{XmlReadOptions, XmlWriteOptions};

use crate::{Instance, RuntimeError};

pub const MAX_XML_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_XML_HINT_DESCRIPTOR_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlBoundaryErrorKind {
    Schema,
    DocumentLimit,
    Utf8,
    Input,
    Mapping,
    Output,
}

/// Retains the original typed parser or mapping error as its standard source.
#[derive(Debug)]
pub struct XmlBoundaryError {
    pub kind: XmlBoundaryErrorKind,
    pub detail: String,
    pub bytes: Option<usize>,
    pub limit: Option<usize>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl XmlBoundaryError {
    fn detail(kind: XmlBoundaryErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            bytes: None,
            limit: None,
            source: None,
        }
    }

    fn with_source<E>(kind: XmlBoundaryErrorKind, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self {
            kind,
            detail: source.to_string(),
            bytes: None,
            limit: None,
            source: Some(Box::new(source)),
        }
    }

    fn document_limit(bytes: usize) -> Self {
        Self {
            kind: XmlBoundaryErrorKind::DocumentLimit,
            detail: format!("XML document is {bytes} bytes; maximum is {MAX_XML_DOCUMENT_BYTES}"),
            bytes: Some(bytes),
            limit: Some(MAX_XML_DOCUMENT_BYTES),
            source: None,
        }
    }
}

impl fmt::Display for XmlBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: XML boundary failed: {}", self.kind, self.detail)
    }
}

impl std::error::Error for XmlBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<RuntimeError> for XmlBoundaryError {
    fn from(source: RuntimeError) -> Self {
        Self::with_source(XmlBoundaryErrorKind::Mapping, source)
    }
}

fn check_document_size(bytes: usize) -> Result<(), XmlBoundaryError> {
    if bytes > MAX_XML_DOCUMENT_BYTES {
        return Err(XmlBoundaryError::document_limit(bytes));
    }
    Ok(())
}

fn schema(descriptor: &str) -> Result<ir::SchemaNode, XmlBoundaryError> {
    if descriptor.len() > crate::MAX_EMBEDDED_XML_SCHEMA_BYTES {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            format!(
                "embedded XML schema is {} bytes; maximum is {}",
                descriptor.len(),
                crate::MAX_EMBEDDED_XML_SCHEMA_BYTES
            ),
        ));
    }
    codegen_schema::decode(descriptor, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Schema, source))
}

/// Parses one bounded observed primary flat XML document with actual reader facts.
/// Unsupported profiles remain explicit; this is not a named-document adapter.
pub fn parse_xml(
    descriptor: &str,
    xml: &str,
    allow_inactive_root_type_members: bool,
    root_view_policy: bool,
) -> Result<Instance, XmlBoundaryError> {
    check_document_size(xml.len())?;
    if !allow_inactive_root_type_members || !root_view_policy {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "generated XML input requires the closed observed primary root policy",
        ));
    }
    let schema = schema(descriptor)?;
    if !input_namespace_identity_supported(&schema) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "XML input schema namespace is invalid or too large",
        ));
    }
    if schema_has_supplementary_name(&schema, true) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "supplementary XML names are unsupported by the generated input and hinted-output parsers",
        ));
    }
    if !ir::xml_root_view_read_policy_is_supported(&schema) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "unsupported generated XML input schema",
        ));
    }
    format_xml::from_str_with_options(
        xml,
        &schema,
        &XmlReadOptions {
            allow_inactive_root_type_members,
            root_view_policy,
        },
    )
    .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Input, source))
}

/// Checks byte size and UTF-8 before any document parsing or schema decoding.
pub fn parse_xml_bytes(
    descriptor: &str,
    xml: &[u8],
    allow_inactive_root_type_members: bool,
    root_view_policy: bool,
) -> Result<Instance, XmlBoundaryError> {
    check_document_size(xml.len())?;
    let xml = std::str::from_utf8(xml)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Utf8, source))?;
    parse_xml(
        descriptor,
        xml,
        allow_inactive_root_type_members,
        root_view_policy,
    )
}

/// Writes one bounded document using literal generated output policy.
fn input_namespace_identity_supported(schema: &ir::SchemaNode) -> bool {
    if let Some(ir::XmlNamespace::Qualified(uri)) = &schema.xml_namespace
        && (uri.as_str().len() + schema.name.len() + 2 > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES
            || !ir::primary_root_type_identity_is_valid(&format!(
                "{{{}}}{}",
                uri.as_str(),
                schema.name
            )))
    {
        return false;
    }
    match &schema.kind {
        ir::SchemaKind::Group { children, .. } => {
            children.iter().all(input_namespace_identity_supported)
        }
        _ => true,
    }
}

fn schema_has_supplementary_name(schema: &ir::SchemaNode, include_types: bool) -> bool {
    if schema
        .name
        .chars()
        .any(|character| character as u32 > 0xFFFF)
    {
        return true;
    }
    match &schema.kind {
        ir::SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } => {
            (include_types
                && alternatives.iter().any(|alternative| {
                    let local = alternative
                        .name
                        .rsplit_once('}')
                        .map_or(alternative.name.as_str(), |(_, local)| local);
                    local.chars().any(|character| character as u32 > 0xFFFF)
                }))
                || children
                    .iter()
                    .any(|child| schema_has_supplementary_name(child, include_types))
                || dynamic
                    .as_ref()
                    .is_some_and(|child| schema_has_supplementary_name(child, include_types))
        }
        _ => false,
    }
}

fn document_namespace_metadata_supported(schema: &ir::SchemaNode, root: bool) -> bool {
    let uri = match &schema.xml_namespace {
        Some(ir::XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    };
    if schema.attribute && schema.name == "xmlns" && uri.is_none() {
        return false;
    }
    if !root
        && (uri == Some("http://www.w3.org/2000/xmlns/")
            || (!schema.attribute && uri == Some("http://www.w3.org/XML/1998/namespace")))
    {
        return false;
    }
    match &schema.kind {
        ir::SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } => {
            !alternatives.iter().any(|alternative| {
                alternative
                    .name
                    .strip_prefix('{')
                    .and_then(|name| name.split_once('}'))
                    .is_some_and(|(uri, _)| {
                        matches!(
                            uri,
                            "http://www.w3.org/XML/1998/namespace"
                                | "http://www.w3.org/2000/xmlns/"
                        )
                    })
            }) && children
                .iter()
                .all(|child| document_namespace_metadata_supported(child, false))
                && dynamic
                    .as_ref()
                    .is_none_or(|child| document_namespace_metadata_supported(child, false))
        }
        _ => true,
    }
}

fn validate_document_names(schema: &ir::SchemaNode, root: bool) -> Result<(), XmlBoundaryError> {
    let virtual_text = !root
        && schema.name == ir::XML_TEXT_FIELD
        && schema.text
        && !schema.attribute
        && !schema.repeating
        && matches!(schema.kind, ir::SchemaKind::Scalar { .. });
    if !virtual_text && !ir::primary_root_ncname_is_valid(&schema.name) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "XML document schema requires local NCNames and canonical type identities",
        ));
    }
    if let ir::SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &schema.kind
    {
        if alternatives
            .iter()
            .any(|alternative| !ir::primary_root_type_identity_is_valid(&alternative.name))
        {
            return Err(XmlBoundaryError::detail(
                XmlBoundaryErrorKind::Schema,
                "XML document schema requires local NCNames and canonical type identities",
            ));
        }
        for child in children {
            validate_document_names(child, false)?;
        }
        if let Some(dynamic) = dynamic {
            validate_document_names(dynamic, false)?;
        }
    }
    Ok(())
}

/// Schema locations are neither opened nor resolved against a host path.
pub fn serialize_xml_document(
    descriptor: &str,
    instance: &Instance,
    declaration: bool,
    indent: bool,
    default_namespace: Option<&str>,
    schema_hints_json: Option<&str>,
) -> Result<String, XmlBoundaryError> {
    let schema = schema(descriptor)?;
    validate_document_names(&schema, true)?;
    if !document_namespace_metadata_supported(&schema, true) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "unsupported XML namespace declaration metadata in document schema",
        ));
    }
    if schema_hints_json.is_some() && schema_has_supplementary_name(&schema, false) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "supplementary XML names are unsupported by the generated input and hinted-output parsers",
        ));
    }
    if default_namespace
        .is_some_and(|uri| uri.is_empty() || uri.len() > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES)
    {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "default XML namespace must contain at most 4096 UTF-8 bytes and be nonempty",
        ));
    }
    if default_namespace.is_some_and(|uri| {
        !uri.chars().all(|character| {
            matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
        })
    }) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "default XML namespace must contain XML 1.0 characters",
        ));
    }
    let schema_root_namespace = match &schema.xml_namespace {
        Some(ir::XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    };
    if [default_namespace, schema_root_namespace]
        .into_iter()
        .any(|uri| {
            matches!(
                uri,
                Some("http://www.w3.org/XML/1998/namespace" | "http://www.w3.org/2000/xmlns/")
            )
        })
    {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "reserved XML namespace cannot be the default namespace",
        ));
    }
    let hints = schema_hints_json
        .map(|descriptor| {
            if descriptor.len() > MAX_XML_HINT_DESCRIPTOR_BYTES {
                return Err(XmlBoundaryError::detail(
                    XmlBoundaryErrorKind::Output,
                    "XML schema-hint descriptor exceeds 2 MiB",
                ));
            }
            let hints: ir::XmlSchemaHints = serde_json::from_str(descriptor).map_err(|source| {
                XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, source)
            })?;
            hints.validate().map_err(|source| {
                XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, source)
            })?;
            Ok(hints)
        })
        .transpose()?;
    let options = XmlWriteOptions {
        declaration,
        indent,
        default_namespace: default_namespace.map(str::to_owned),
        schema_hints: hints,
    };
    let xml = format_xml::to_string_with_options(&schema, instance, &options)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, source))?;
    let xml = namespace::preserve(xml, &schema, instance, &options)?;
    check_document_size(xml.len())?;
    if !xml.chars().all(|character| {
        matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
    }) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "XML output must contain XML 1.0 characters",
        ));
    }
    Ok(xml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn descriptor() -> String {
        let schema: ir::SchemaNode = serde_json::from_str(r#"{
          "name":"Envelope","xml_namespace":{"kind":"unqualified"},"repeating":false,
          "xml_type_alternatives":true,"xml_default_type":"Base",
          "kind":{"kind":"group","children":[
            {"name":"Token","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}},
            {"name":"Supplement","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}
          ],"alternatives":[{"name":"Base","members":["Token"]},{"name":"Extended","members":["Token","Supplement"]}]}
        }"#).unwrap();
        codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap()
    }

    #[test]
    fn document_namespace_metadata_cannot_declare_reserved_bindings_but_xml_attributes_work() {
        let ordinary = ir::Instance::Group(
            vec![(
                "Code".into(),
                ir::Instance::Scalar(ir::Value::String("ordinary".into())),
            )]
            .into(),
        );
        let mut schemas = Vec::new();
        let mut xmlns = ir::SchemaNode::group(
            "Root",
            vec![ir::SchemaNode::scalar("xmlns", ir::ScalarType::String).attribute()],
        );
        xmlns.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new("urn:root").unwrap(),
        ));
        schemas.push(xmlns);
        for (uri, attribute) in [
            ("http://www.w3.org/2000/xmlns/", true),
            ("http://www.w3.org/XML/1998/namespace", false),
        ] {
            let mut child = ir::SchemaNode::scalar("Code", ir::ScalarType::String);
            child.attribute = attribute;
            child.xml_namespace = Some(ir::XmlNamespace::Qualified(
                ir::XmlNamespaceUri::new(uri).unwrap(),
            ));
            schemas.push(ir::SchemaNode::group("Root", vec![child]));
        }
        for uri in [
            "http://www.w3.org/XML/1998/namespace",
            "http://www.w3.org/2000/xmlns/",
        ] {
            let mut typed: ir::SchemaNode = serde_json::from_str(&descriptor()).unwrap();
            if let ir::SchemaKind::Group { alternatives, .. } = &mut typed.kind {
                alternatives[1].name = format!("{{{uri}}}Extended");
            }
            schemas.push(typed);
        }
        for schema in schemas {
            let error = serialize_xml_document(
                &serde_json::to_string(&schema).unwrap(),
                &ordinary,
                false,
                false,
                None,
                None,
            )
            .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
            assert_eq!(
                error.detail,
                "unsupported XML namespace declaration metadata in document schema"
            );
        }
        let mut field = ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute();
        field.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new("http://www.w3.org/XML/1998/namespace").unwrap(),
        ));
        let schema = ir::SchemaNode::group("Root", vec![field]);
        assert_eq!(
            serialize_xml_document(
                &serde_json::to_string(&schema).unwrap(),
                &ordinary,
                false,
                false,
                None,
                None
            )
            .unwrap(),
            "<Root xml:Code=\"ordinary\"/>"
        );
    }

    #[test]
    fn document_schema_names_refuse_malformed_children_and_preserve_qualified_unicode_text() {
        let instance = ir::Instance::Group(
            vec![(
                "Code".into(),
                ir::Instance::Scalar(ir::Value::String("ordinary".into())),
            )]
            .into(),
        );
        for (root_name, child_name, attribute) in [
            ("bad root", "Code", true),
            ("Root", "bad attr", true),
            ("Root", "bad child", false),
            ("p:Root", "Code", true),
            ("Root", "p:Code", true),
        ] {
            let schema = ir::SchemaNode::group(
                root_name,
                vec![{
                    let mut field = ir::SchemaNode::scalar(child_name, ir::ScalarType::String);
                    field.attribute = attribute;
                    field
                }],
            );
            let descriptor = serde_json::to_string(&schema).unwrap();
            let error = serialize_xml_document(&descriptor, &instance, false, false, None, None)
                .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
            assert_eq!(
                error.detail,
                "XML document schema requires local NCNames and canonical type identities"
            );
        }
        for root_name in ["根", "𐀀"] {
            let mut schema = ir::SchemaNode::group(
                root_name,
                vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
            );
            schema.xml_namespace = Some(ir::XmlNamespace::Qualified(
                ir::XmlNamespaceUri::new("urn:root").unwrap(),
            ));
            let xml = serialize_xml_document(
                &serde_json::to_string(&schema).unwrap(),
                &instance,
                false,
                false,
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                xml,
                format!("<{root_name} xmlns=\"urn:root\" Code=\"ordinary\"/>")
            );
        }
        let supplementary = ir::SchemaNode::group(
            "𐀀",
            vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
        );
        let error = serialize_xml_document(
            &serde_json::to_string(&supplementary).unwrap(),
            &instance,
            false,
            false,
            None,
            Some("{}"),
        )
        .unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
        assert!(error.detail.contains("supplementary XML names"));
        let text_schema = ir::SchemaNode::group(
            "Root",
            vec![{
                let mut text = ir::SchemaNode::scalar(ir::XML_TEXT_FIELD, ir::ScalarType::String);
                text.text = true;
                text
            }],
        );
        let text_instance = ir::Instance::Group(
            vec![(
                ir::XML_TEXT_FIELD.into(),
                ir::Instance::Scalar(ir::Value::String("ordinary".into())),
            )]
            .into(),
        );
        assert_eq!(
            serialize_xml_document(
                &serde_json::to_string(&text_schema).unwrap(),
                &text_instance,
                false,
                false,
                None,
                None
            )
            .unwrap(),
            "<Root>ordinary</Root>"
        );
        let mut typed_schema: ir::SchemaNode = serde_json::from_str(&descriptor()).unwrap();
        if let ir::SchemaKind::Group { alternatives, .. } = &mut typed_schema.kind {
            alternatives[1].name = "{urn:type}Extended".into();
        }
        let typed_instance = ir::Instance::Group(
            vec![
                (
                    ir::XML_TYPE_FIELD.into(),
                    ir::Instance::Scalar(ir::Value::String("{urn:type}Extended".into())),
                ),
                (
                    "Token".into(),
                    ir::Instance::Scalar(ir::Value::String("value".into())),
                ),
                (
                    "Supplement".into(),
                    ir::Instance::Scalar(ir::Value::String("extra".into())),
                ),
            ]
            .into(),
        );
        let xml = serialize_xml_document(
            &serde_json::to_string(&typed_schema).unwrap(),
            &typed_instance,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("xmlns:ft=\"urn:type\""));
        assert!(xml.contains("xsi:type=\"ft:Extended\""));
        assert!(xml.contains("Token=\"value\" Supplement=\"extra\""));
        let legacy = ir::SchemaNode::group(
            "bad root",
            vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
        );
        assert_eq!(
            crate::serialize_xml(
                0,
                &serde_json::to_string(&legacy).unwrap(),
                &instance,
                false,
                false,
                None
            )
            .unwrap(),
            ir::Value::String("<bad root Code=\"ordinary\"/>".into())
        );
    }

    #[test]
    fn actual_xml_input_preserves_origin_and_defers_required_fields() {
        let schema = descriptor();
        for (annotation, matches) in [
            ("", false),
            (" xsi:type=\"Extended\"", true),
            (" xsi:type=\"Other\"", false),
            (" xsi:type=\" Extended \"", false),
        ] {
            let xml = format!(
                "<Envelope xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"{annotation} Token=\"\"/>"
            );
            let input = parse_xml(&schema, &xml, true, true).unwrap();
            assert_eq!(
                ir::primary_root_xml_type_equals(Some(&input), "Extended"),
                Ok(matches)
            );
            assert_eq!(
                ir::primary_root_scalar_with_requirement(Some(&input), &["Token"], true),
                Ok(crate::Value::String(String::new()))
            );
            assert!(matches!(
                ir::primary_root_scalar_with_requirement(Some(&input), &["Supplement"], true),
                Err(ir::PrimaryRootError::MissingRequiredField { .. })
            ));
            assert_eq!(
                parse_xml_bytes(&schema, xml.as_bytes(), true, true).unwrap(),
                input
            );
        }
    }

    #[test]
    fn document_and_utf8_limits_precede_schema_and_mapping_work() {
        let error = parse_xml_bytes("bad", &[0xff], true, true).unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Utf8);
        let oversized = vec![0xff; MAX_XML_DOCUMENT_BYTES + 1];
        let error = parse_xml_bytes("bad", &oversized, true, true).unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::DocumentLimit);
        assert_eq!(error.bytes, Some(MAX_XML_DOCUMENT_BYTES + 1));
        assert_eq!(error.limit, Some(MAX_XML_DOCUMENT_BYTES));
        let error = parse_xml(
            &" ".repeat(crate::MAX_EMBEDDED_XML_SCHEMA_BYTES + 1),
            "<Envelope/>",
            true,
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Schema);
    }

    #[test]
    fn numeric_nil_is_typed_input_error_and_mapping_error_retains_original_payload() {
        let error = parse_xml(
            &descriptor(),
            r#"<Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="1"/>"#,
            true,
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, XmlBoundaryErrorKind::Input);
        assert!(
            matches!(error.source().and_then(|source|source.downcast_ref::<format_xml::XmlFormatError>()),
            Some(format_xml::XmlFormatError::InvalidXmlNil { value, .. }) if value=="1")
        );
        let original = RuntimeError::PrimaryRoot {
            node: 9,
            source: ir::PrimaryRootError::MissingRequiredField {
                path: vec!["Supplement".into()],
            },
        };
        let error = XmlBoundaryError::from(original);
        assert_eq!(error.kind, XmlBoundaryErrorKind::Mapping);
        assert!(
            matches!(error.source().and_then(|source|source.downcast_ref::<RuntimeError>()),
            Some(RuntimeError::PrimaryRoot { node:9, source:ir::PrimaryRootError::MissingRequiredField { path } }) if path==&["Supplement".to_string()])
        );
    }

    #[test]
    fn xml_output_policy_escapes_literal_hints_and_preserves_declared_fields() {
        let schema = descriptor();
        let instance = parse_xml(&schema, r#"<Envelope Token="A&amp;B"/>"#, true, true).unwrap();
        let hints = r#"{"no_namespace_location":"schema&lt;\".xsd"}"#;
        let output =
            serialize_xml_document(&schema, &instance, false, false, None, Some(hints)).unwrap();
        assert!(!output.starts_with("<?xml"));
        assert!(output.contains("Token=\"A&amp;B\""));
        assert!(output.contains("xsi:noNamespaceSchemaLocation=\"schema&amp;lt;&quot;.xsd\""));
        assert_eq!(
            parse_xml(&schema, &output, true, true)
                .unwrap()
                .field("Token"),
            instance.field("Token")
        );
    }
}

#[cfg(test)]
#[test]
fn invalid_default_namespace_xml_character_is_typed_output_refusal() {
    let target = ir::SchemaNode::group(
        "Target",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
    );
    let descriptor = codegen_schema::encode(&target, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let instance = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("v".into())),
        )]
        .into(),
    );
    let result = serialize_xml_document(&descriptor, &instance, false, false, Some("\0"), None);
    assert!(
        matches!(
            result,
            Err(XmlBoundaryError {
                kind: XmlBoundaryErrorKind::Output,
                ..
            })
        ),
        "must refuse forbidden XML namespace character, actual: {result:?}"
    );
}

#[cfg(test)]
#[test]
fn reserved_default_namespace_is_typed_output_refusal_without_or_with_hints() {
    let target = ir::SchemaNode::group("Target", vec![]);
    let descriptor = codegen_schema::encode(&target, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let instance = Instance::Group(vec![].into());
    for namespace in [
        "http://www.w3.org/XML/1998/namespace",
        "http://www.w3.org/2000/xmlns/",
    ] {
        for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
            let error = serialize_xml_document(
                &descriptor,
                &instance,
                false,
                false,
                Some(namespace),
                hints,
            )
            .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Output);
            assert_eq!(
                error.detail,
                "reserved XML namespace cannot be the default namespace"
            );
        }
    }
}

#[cfg(test)]
#[test]
fn new_document_namespace_preserves_value_without_changing_legacy_writer() {
    use quick_xml::{Reader, XmlVersion, events::Event};
    let schema = ir::SchemaNode::group(
        "Root",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
    );
    let descriptor = codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let value = "ordinary xmlns=\"\t\" data\r\n";
    let instance = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String(value.into())),
        )]
        .into(),
    );
    for uri in ["\t", "\n", "\r", "urn:literal:\t\n\r<&\"😀"] {
        for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
            let output =
                serialize_xml_document(&descriptor, &instance, false, false, Some(uri), hints)
                    .unwrap();
            let mut reader = Reader::from_str(&output);
            let start = match reader.read_event().unwrap() {
                Event::Empty(start) => start,
                actual => panic!("unexpected root event: {actual:?}"),
            };
            let attributes = start
                .attributes()
                .map(|attribute| {
                    let attribute = attribute.unwrap();
                    (
                        String::from_utf8(attribute.key.as_ref().to_vec()).unwrap(),
                        attribute
                            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                            .unwrap()
                            .into_owned(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(attributes["xmlns"], uri);
            assert_eq!(attributes["Code"], value);
        }
        let legacy = format_xml::to_string_with_options(
            &schema,
            &instance,
            &XmlWriteOptions {
                declaration: false,
                indent: false,
                default_namespace: Some(uri.into()),
                schema_hints: None,
            },
        )
        .unwrap();
        assert!(
            legacy.contains(if uri.len() == 1 {
                uri
            } else {
                "urn:literal:\t\n\r"
            }),
            "legacy namespace whitespace bytes must remain unchanged: {legacy:?}"
        );
    }
}

#[cfg(test)]
#[test]
fn new_document_rejects_forbidden_scalar_characters_and_preserves_legal_values() {
    for attribute in [false, true] {
        let mut field = ir::SchemaNode::scalar("Code", ir::ScalarType::String);
        field.attribute = attribute;
        let schema = ir::SchemaNode::group("Root", vec![field]);
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        for character in ['\0', '\u{1}', '\u{B}', '\t', '\n', '\r', '😀'] {
            let value = format!("a{character}b");
            let instance = Instance::Group(
                vec![(
                    "Code".into(),
                    Instance::Scalar(crate::Value::String(value.clone())),
                )]
                .into(),
            );
            for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
                let output =
                    serialize_xml_document(&descriptor, &instance, false, false, None, hints);
                if matches!(character, '\0' | '\u{1}' | '\u{B}') {
                    let error = output.unwrap_err();
                    assert_eq!(error.kind, XmlBoundaryErrorKind::Output);
                    assert_eq!(error.detail, "XML output must contain XML 1.0 characters");
                } else {
                    let output = output.unwrap();
                    let parsed = format_xml::from_str(&output, &schema).unwrap();
                    assert_eq!(
                        ir::primary_root_scalar_with_requirement(Some(&parsed), &["Code"], false),
                        Ok(crate::Value::String(value.clone()))
                    );
                }
            }
            if matches!(character, '\0' | '\u{1}' | '\u{B}') {
                // The new boundary validation must not change the legacy writer.
                let legacy = format_xml::to_string(&schema, &instance).unwrap();
                assert!(legacy.contains(character));
            }
        }
    }
}

#[cfg(test)]
#[test]
fn new_document_preserves_schema_hint_and_document_limit_error_precedence() {
    let schema = ir::SchemaNode::group(
        "Root",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String)],
    );
    let descriptor = codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let invalid = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("\0".into())),
        )]
        .into(),
    );
    assert_eq!(
        serialize_xml_document("{}", &invalid, false, false, None, None)
            .unwrap_err()
            .kind,
        XmlBoundaryErrorKind::Schema
    );
    let hint =
        serialize_xml_document(&descriptor, &invalid, false, false, None, Some("{}")).unwrap_err();
    assert_eq!(hint.kind, XmlBoundaryErrorKind::Output);
    assert_ne!(hint.detail, "XML output must contain XML 1.0 characters");
    let oversized = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("\0".repeat(MAX_XML_DOCUMENT_BYTES))),
        )]
        .into(),
    );
    let error =
        serialize_xml_document(&descriptor, &oversized, false, false, None, None).unwrap_err();
    assert_eq!(error.kind, XmlBoundaryErrorKind::DocumentLimit);
    assert!(error.bytes.unwrap() > MAX_XML_DOCUMENT_BYTES);
    assert_eq!(error.limit, Some(MAX_XML_DOCUMENT_BYTES));
}

#[cfg(test)]
#[test]
fn new_document_rejects_reserved_schema_root_namespace_without_changing_legacy() {
    for uri in [
        "http://www.w3.org/XML/1998/namespace",
        "http://www.w3.org/2000/xmlns/",
    ] {
        let mut schema = ir::SchemaNode::group("Root", vec![]);
        schema.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new(uri).unwrap(),
        ));
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        let instance = Instance::Group(vec![].into());
        for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
            let error = serialize_xml_document(&descriptor, &instance, false, false, None, hints)
                .unwrap_err();
            assert_eq!(error.kind, XmlBoundaryErrorKind::Output);
            assert_eq!(
                error.detail,
                "reserved XML namespace cannot be the default namespace"
            );
        }
        let legacy = format_xml::to_string(&schema, &instance).unwrap();
        assert!(legacy.contains(&format!("xmlns=\"{uri}\"")));
    }
}

#[cfg(test)]
#[test]
fn new_document_preserves_schema_root_namespace_and_explicit_unqualified_override() {
    use quick_xml::{Reader, XmlVersion, events::Event};
    let instance = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(crate::Value::String("ordinary\tdata".into())),
        )]
        .into(),
    );
    let mut schema = ir::SchemaNode::group(
        "Root",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String).attribute()],
    );
    for character in ['\t', '\n', '\r'] {
        let uri = format!("urn:qualified:{character}value");
        schema.xml_namespace = Some(ir::XmlNamespace::Qualified(
            ir::XmlNamespaceUri::new(&uri).unwrap(),
        ));
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        for policy in [None, Some("urn:ignored")] {
            for hints in [None, Some(r#"{"no_namespace_location":"literal.xsd"}"#)] {
                let xml =
                    serialize_xml_document(&descriptor, &instance, false, false, policy, hints)
                        .unwrap();
                let mut reader = Reader::from_str(&xml);
                let start = match reader.read_event().unwrap() {
                    Event::Empty(start) => start,
                    actual => panic!("unexpected root event: {actual:?}"),
                };
                let attribute = start
                    .attributes()
                    .map(Result::unwrap)
                    .find(|attribute| attribute.key.as_ref() == b"xmlns")
                    .unwrap();
                assert_eq!(
                    attribute
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .unwrap(),
                    uri
                );
            }
        }
    }
    schema.xml_namespace = Some(ir::XmlNamespace::Unqualified);
    let descriptor = codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
    let xml = serialize_xml_document(
        &descriptor,
        &instance,
        false,
        false,
        Some("urn:ignored:\tvalue"),
        None,
    )
    .unwrap();
    assert!(!xml.contains("xmlns="));
}
