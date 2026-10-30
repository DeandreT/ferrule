//! Native XML reading, writing and XSD export refuse excluded wildcard names
//! until the native codec (#1220) can evaluate and write them. Export also
//! refuses `notNamespace`, which instance I/O already enforces.

use format_xml::XmlFormatError;
use format_xml::xsd::{export, export_set};
use ir::{
    ScalarType, SchemaNode, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD,
    XML_NAMESPACE_URI_FIELD, XML_TEXT_FIELD, XmlExpandedName, XmlNamespace,
    XmlWildcardExcludedNames, XmlWildcardNamespaceConstraint, XmlWildcardNamespaceList,
    XmlWildcardProcessContents, {Instance, Value},
};

const T: &str = "urn:ferrule:wx:t";
const A: &str = "urn:ferrule:wx:a";
const B: &str = "urn:ferrule:wx:b";

fn qualified(uri: &str) -> XmlNamespace {
    XmlNamespace::qualified(uri).unwrap()
}

fn negated() -> XmlWildcardNamespaceConstraint {
    XmlWildcardNamespaceConstraint::Not {
        namespaces: XmlWildcardNamespaceList::new([qualified(T), XmlNamespace::Unqualified])
            .unwrap(),
    }
}

fn excluded(namespace: XmlNamespace, local: &str) -> XmlWildcardExcludedNames {
    XmlWildcardExcludedNames::new([XmlExpandedName::new(namespace, local).unwrap()]).unwrap()
}

fn elements(constraint: XmlWildcardNamespaceConstraint) -> SchemaNode {
    let mut group = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
        ],
    )
    .repeating()
    .with_xml_wildcard_namespace(constraint)
    .unwrap();
    group.xml_wildcard_process_contents = XmlWildcardProcessContents::Lax;
    group
}

fn attributes(constraint: XmlWildcardNamespaceConstraint) -> SchemaNode {
    SchemaNode::group(
        XML_ATTRIBUTES_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating()
    .with_xml_wildcard_namespace(constraint)
    .unwrap()
}

fn root(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Root", children)
        .xml_qualified(T)
        .unwrap()
}

fn head() -> SchemaNode {
    SchemaNode::scalar("Head", ScalarType::String)
        .xml_qualified(T)
        .unwrap()
}

/// Exports through both entry points and returns their two errors.
fn refusals(schema: &SchemaNode) -> [XmlFormatError; 2] {
    [
        export(schema).expect_err("single-document export must refuse"),
        export_set(schema, "root.xsd").expect_err("export set must refuse"),
    ]
}

#[test]
fn element_wildcards_with_exclusions_are_refused() {
    let not_only = elements(negated());
    let names_only = elements(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(excluded(qualified(A), "Secret"))
        .unwrap();
    let both = elements(negated())
        .with_xml_wildcard_excluded_names(excluded(qualified(A), "Secret"))
        .unwrap();
    for wildcard in [not_only, names_only, both] {
        for error in refusals(&root(vec![head(), wildcard.clone()])) {
            assert!(
                matches!(
                    error,
                    XmlFormatError::UnsupportedXmlWildcard { reason }
                        if reason.contains("XSD 1.1")
                ),
                "{error:?}"
            );
        }
    }
}

#[test]
fn attribute_wildcards_with_exclusions_are_refused() {
    let not_only = attributes(negated());
    let names_only = attributes(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(excluded(XmlNamespace::Unqualified, "internal"))
        .unwrap();
    for wildcard in [not_only, names_only] {
        for error in refusals(&root(vec![head(), wildcard.clone()])) {
            assert!(
                matches!(
                    error,
                    XmlFormatError::UnsupportedXmlAttributeWildcard { reason }
                        if reason.contains("XSD 1.1")
                ),
                "{error:?}"
            );
        }
    }
}

#[test]
fn existing_wildcards_still_export() {
    let schema = root(vec![
        head(),
        elements(XmlWildcardNamespaceConstraint::Other {
            target_namespace: ir::XmlNamespaceUri::new(T),
        }),
        attributes(XmlWildcardNamespaceConstraint::Any),
    ]);
    let text = export(&schema).unwrap();
    assert!(text.contains(r###"<xs:any namespace="##other" processContents="lax""###));
    assert!(text.contains(r###"<xs:anyAttribute namespace="##any""###));
    assert!(!text.contains("notNamespace") && !text.contains("notQName"));
    export_set(&schema, "root.xsd").unwrap();
}

/// Two concrete anchors whose only difference is the anchor's own excluded
/// names. Without comparing that field, both would look like one definition.
fn anchor_pair(reference: SchemaNode) -> SchemaNode {
    let plain = elements(XmlWildcardNamespaceConstraint::Any);
    let excluding = plain
        .clone()
        .with_xml_wildcard_excluded_names(excluded(qualified(A), "Secret"))
        .unwrap();
    SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group("First", vec![plain]),
            SchemaNode::group("Second", vec![excluding]),
            reference,
        ],
    )
}

#[test]
fn recursive_anchors_differing_only_by_exclusions_are_refused() {
    let reference = SchemaNode::recursive_group("Again", XML_ELEMENTS_FIELD);
    let error = export(&anchor_pair(reference)).unwrap_err();
    assert!(
        matches!(
            &error,
            XmlFormatError::UnsupportedRecursiveAnchor { node, anchor }
                if node == "Again" && anchor == XML_ELEMENTS_FIELD
        ),
        "{error:?}"
    );

    // A cross-namespace occurrence is materialized by the export-set planner,
    // which compares the anchors on its own.
    let reference = SchemaNode::recursive_group("Again", XML_ELEMENTS_FIELD)
        .xml_qualified(B)
        .unwrap();
    let error = export_set(&anchor_pair(reference), "root.xsd").unwrap_err();
    assert!(
        matches!(
            &error,
            XmlFormatError::UnsupportedRecursiveAnchor { node, anchor }
                if node == "Again" && anchor == XML_ELEMENTS_FIELD
        ),
        "{error:?}"
    );
}

const DOCUMENT: &str = r#"<t:Root xmlns:t="urn:ferrule:wx:t" xmlns:a="urn:ferrule:wx:a">"#;

fn secret() -> XmlWildcardExcludedNames {
    excluded(qualified(A), "Secret")
}

fn internal() -> XmlWildcardExcludedNames {
    excluded(XmlNamespace::Unqualified, "internal")
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn generic_item(local: &str, namespace: &str, value: Option<&str>) -> Instance {
    let mut fields = vec![
        (XML_LOCAL_NAME_FIELD, text(local)),
        (XML_NAMESPACE_URI_FIELD, text(namespace)),
    ];
    if let Some(value) = value {
        fields.push((XML_TEXT_FIELD, text(value)));
    }
    group(fields)
}

fn excluded_names_refused(error: &XmlFormatError) -> bool {
    matches!(
        error,
        XmlFormatError::UnsupportedXmlWildcard { reason }
            | XmlFormatError::UnsupportedXmlAttributeWildcard { reason }
            if reason.contains("excluded names")
    )
}

#[test]
fn native_element_io_refuses_excluded_names() {
    let wildcard = elements(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(secret())
        .unwrap();
    let schema = root(vec![head(), wildcard]);

    // Any generic item fails closed, excluded or not; documents without
    // wildcard content still read.
    for content in ["<a:Secret/>", "<a:Other/>"] {
        let error = format_xml::from_str(
            &format!("{DOCUMENT}<t:Head>h</t:Head>{content}</t:Root>"),
            &schema,
        )
        .unwrap_err();
        assert!(
            matches!(&error, XmlFormatError::UnsupportedXmlWildcard { .. })
                && excluded_names_refused(&error),
            "{content}: {error:?}"
        );
    }
    let empty =
        format_xml::from_str(&format!("{DOCUMENT}<t:Head>h</t:Head></t:Root>"), &schema).unwrap();

    for local in ["Secret", "Other"] {
        let instance = group(vec![
            ("Head", text("h")),
            (
                XML_ELEMENTS_FIELD,
                Instance::Repeated(vec![generic_item(local, A, None)]),
            ),
        ]);
        let error = format_xml::to_string(&schema, &instance).unwrap_err();
        assert!(
            matches!(&error, XmlFormatError::UnsupportedXmlWildcard { .. })
                && excluded_names_refused(&error),
            "{local}: {error:?}"
        );
    }
    format_xml::to_string(&schema, &empty).unwrap();
}

#[test]
fn native_attribute_io_refuses_excluded_names() {
    let wildcard = attributes(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(internal())
        .unwrap();
    let schema = root(vec![head(), wildcard]);

    for attribute in [r#"internal="x""#, r#"mode="m""#] {
        let document = format!(
            r#"<t:Root xmlns:t="urn:ferrule:wx:t" {attribute}><t:Head>h</t:Head></t:Root>"#
        );
        let error = format_xml::from_str(&document, &schema).unwrap_err();
        assert!(
            matches!(
                &error,
                XmlFormatError::UnsupportedXmlAttributeWildcard { .. }
            ) && excluded_names_refused(&error),
            "{attribute}: {error:?}"
        );
    }

    for local in ["internal", "mode"] {
        let instance = group(vec![
            ("Head", text("h")),
            (
                XML_ATTRIBUTES_FIELD,
                Instance::Repeated(vec![generic_item(local, "", Some("x"))]),
            ),
        ]);
        let error = format_xml::to_string(&schema, &instance).unwrap_err();
        assert!(
            matches!(
                &error,
                XmlFormatError::UnsupportedXmlAttributeWildcard { .. }
            ) && excluded_names_refused(&error),
            "{local}: {error:?}"
        );
    }
}

#[test]
fn negated_namespaces_stay_enforced_by_native_io() {
    let schema = root(vec![head(), elements(negated())]);
    let read = format_xml::from_str(
        &format!("{DOCUMENT}<t:Head>h</t:Head><a:Other/></t:Root>"),
        &schema,
    )
    .unwrap();
    let written = format_xml::to_string(&schema, &read).unwrap();
    assert_eq!(format_xml::from_str(&written, &schema).unwrap(), read);
    let error = format_xml::from_str(
        &format!("{DOCUMENT}<t:Head>h</t:Head><t:Tail/></t:Root>"),
        &schema,
    )
    .unwrap_err();
    assert!(
        matches!(&error, XmlFormatError::XmlWildcardNamespaceMismatch { name, .. } if name == "Tail"),
        "{error:?}"
    );
}

/// A recursive generic occurrence takes its exclusions from the occurrence,
/// not from the anchor whose shape it reuses.
#[test]
fn recursive_occurrences_do_not_inherit_anchor_exclusions() {
    // The first concrete `element()` group is the recursion anchor and
    // excludes a name; the nested marker under `Open` reuses its shape.
    let anchor = elements(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(secret())
        .unwrap();
    let mut open = elements(XmlWildcardNamespaceConstraint::Any);
    if let ir::SchemaKind::Group { children, .. } = &mut open.kind {
        children
            .push(SchemaNode::recursive_group(XML_ELEMENTS_FIELD, XML_ELEMENTS_FIELD).repeating());
    }
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group("Closed", vec![anchor]),
            SchemaNode::group("Open", vec![open]),
        ],
    );
    let document = r#"<Root xmlns:a="urn:ferrule:wx:a"><Closed/><Open><a:Outer><a:Secret/></a:Outer></Open></Root>"#;
    let read = format_xml::from_str(document, &schema).unwrap();
    let written = format_xml::to_string(&schema, &read).unwrap();
    assert_eq!(format_xml::from_str(&written, &schema).unwrap(), read);
    // The anchor itself still refuses its own generic content.
    let error = format_xml::from_str(
        r#"<Root xmlns:a="urn:ferrule:wx:a"><Closed><a:Other/></Closed></Root>"#,
        &schema,
    )
    .unwrap_err();
    assert!(excluded_names_refused(&error), "{error:?}");
}
