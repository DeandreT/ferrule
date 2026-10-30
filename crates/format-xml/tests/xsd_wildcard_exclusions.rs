//! Native XML reading, writing and XSD export of hand-built wildcard groups
//! with XSD 1.1 exclusions. The frozen import fixtures live in
//! `xsd_wildcard_exclusion_fixtures.rs`.

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
const X: &str = "http://www.w3.org/XML/1998/namespace";

fn qualified(uri: &str) -> XmlNamespace {
    XmlNamespace::qualified(uri).unwrap()
}

fn negated() -> XmlWildcardNamespaceConstraint {
    XmlWildcardNamespaceConstraint::Not {
        namespaces: XmlWildcardNamespaceList::new([qualified(T), XmlNamespace::Unqualified])
            .unwrap(),
    }
}

fn excluded(names: &[(XmlNamespace, &str)]) -> XmlWildcardExcludedNames {
    XmlWildcardExcludedNames::new(
        names
            .iter()
            .map(|(namespace, local)| XmlExpandedName::new(namespace.clone(), *local).unwrap()),
    )
    .unwrap()
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

fn attributes(
    constraint: XmlWildcardNamespaceConstraint,
    process: XmlWildcardProcessContents,
) -> SchemaNode {
    let mut group = SchemaNode::group(
        XML_ATTRIBUTES_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating()
    .with_xml_wildcard_namespace(constraint)
    .unwrap();
    group.xml_wildcard_process_contents = process;
    group
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

fn declared_id() -> SchemaNode {
    let mut id = SchemaNode::scalar("id", ScalarType::String).attribute();
    id.xml_namespace = Some(XmlNamespace::Unqualified);
    id
}

#[test]
fn element_wildcards_export_not_namespace_and_explicit_qnames() {
    let target_only = XmlWildcardNamespaceConstraint::Not {
        namespaces: XmlWildcardNamespaceList::new([qualified(T)]).unwrap(),
    };
    let wildcard = elements(target_only)
        .with_xml_wildcard_excluded_names(excluded(&[
            (qualified(B), "Late"),
            (XmlNamespace::Unqualified, "Plain"),
            (qualified(A), "Secret"),
            (qualified(X), "Text"),
        ]))
        .unwrap();
    let schema = root(vec![head(), wildcard]);
    for text in [
        export(&schema).unwrap(),
        match export_set(&schema, "root.xsd") {
            Ok(set) => set.root,
            Err(error) => panic!("{error:?}"),
        },
    ] {
        // Canonical order, one prefix per namespace, the predeclared xml
        // prefix, and an empty default namespace for the unqualified name.
        assert!(
            text.contains(
                r###"<xs:any notNamespace="##targetNamespace" xmlns:fwx1="urn:ferrule:wx:a" xmlns:fwx2="urn:ferrule:wx:b" xmlns="" notQName="Plain xml:Text fwx1:Secret fwx2:Late" processContents="lax""###
            ),
            "{text}"
        );
        assert!(!text.contains("namespace=\"##any\""), "{text}");
    }
}

#[test]
fn attribute_wildcards_export_their_canonical_exclusion_group() {
    let wildcard = attributes(
        XmlWildcardNamespaceConstraint::list([XmlNamespace::Unqualified]).unwrap(),
        XmlWildcardProcessContents::Skip,
    )
    .with_xml_wildcard_excluded_names(excluded(&[(XmlNamespace::Unqualified, "internal")]))
    .unwrap();
    let text = export(&root(vec![head(), wildcard.clone()])).unwrap();
    assert!(
        text.contains(
            r###"<xs:anyAttribute namespace="##local" xmlns="" notQName="internal" processContents="skip"/>"###
        ),
        "{text}"
    );

    // Only the exclusions make the namespace-aware group canonical for a
    // local-only skip wildcard.
    let mut without = wildcard;
    without.xml_wildcard_excluded_names = None;
    assert!(matches!(
        export(&root(vec![head(), without])),
        Err(XmlFormatError::UnsupportedXmlAttributeWildcard { reason })
            if reason.contains("canonical")
    ));
}

#[test]
fn excluded_names_in_the_xmlns_namespace_are_refused_on_export() {
    let wildcard = elements(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(excluded(&[(
            qualified("http://www.w3.org/2000/xmlns/"),
            "prefix",
        )]))
        .unwrap();
    assert!(matches!(
        export(&root(vec![head(), wildcard])),
        Err(XmlFormatError::UnsupportedXmlWildcard { reason }) if reason.contains("xmlns")
    ));
}

#[test]
fn misplaced_or_strict_exclusions_are_refused_on_export() {
    // A name the namespace constraint rejects.
    let mut disallowed = elements(negated());
    disallowed.xml_wildcard_excluded_names = Some(excluded(&[(qualified(T), "Tail")]));
    // Names on an ordinary element.
    let mut misplaced = head();
    misplaced.xml_wildcard_excluded_names = Some(secret());
    for schema in [root(vec![head(), disallowed]), root(vec![misplaced])] {
        for error in [
            export(&schema).unwrap_err(),
            export_set(&schema, "root.xsd").unwrap_err(),
        ] {
            assert!(
                matches!(&error, XmlFormatError::UnsupportedXmlWildcard { reason } if reason.contains("excluded wildcard names")),
                "{error:?}"
            );
        }
    }

    let mut strict = elements(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(secret())
        .unwrap();
    strict.xml_wildcard_process_contents = XmlWildcardProcessContents::Strict;
    let mut strict_not = elements(negated());
    strict_not.xml_wildcard_process_contents = XmlWildcardProcessContents::Strict;
    for wildcard in [strict, strict_not] {
        assert!(matches!(
            export(&root(vec![head(), wildcard])),
            Err(XmlFormatError::UnsupportedXmlWildcard { reason }) if reason.contains("strict")
        ));
    }
}

#[test]
fn existing_wildcards_still_export() {
    let schema = root(vec![
        head(),
        elements(XmlWildcardNamespaceConstraint::Other {
            target_namespace: ir::XmlNamespaceUri::new(T),
        }),
        attributes(
            XmlWildcardNamespaceConstraint::Any,
            XmlWildcardProcessContents::Skip,
        ),
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
        .with_xml_wildcard_excluded_names(secret())
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
    excluded(&[(qualified(A), "Secret")])
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

fn is_excluded_element(error: &XmlFormatError, local: &str, namespace: &str) -> bool {
    matches!(
        error,
        XmlFormatError::XmlWildcardExcludedName { name, namespace: actual }
            if name == local && actual == namespace
    )
}

fn is_excluded_attribute(error: &XmlFormatError, local: &str, namespace: &str) -> bool {
    matches!(
        error,
        XmlFormatError::XmlAttributeWildcardExcludedName { name, namespace: actual }
            if name == local && actual == namespace
    )
}

#[test]
fn native_element_io_refuses_only_excluded_names() {
    let wildcard = elements(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(secret())
        .unwrap();
    let schema = root(vec![head(), wildcard]);

    let error = format_xml::from_str(
        &format!("{DOCUMENT}<t:Head>h</t:Head><a:Secret/></t:Root>"),
        &schema,
    )
    .unwrap_err();
    assert!(is_excluded_element(&error, "Secret", A), "{error:?}");
    let read = format_xml::from_str(
        &format!("{DOCUMENT}<t:Head>h</t:Head><a:Other/></t:Root>"),
        &schema,
    )
    .unwrap();
    let written = format_xml::to_string(&schema, &read).unwrap();
    assert_eq!(format_xml::from_str(&written, &schema).unwrap(), read);

    let instance = group(vec![
        ("Head", text("h")),
        (
            XML_ELEMENTS_FIELD,
            Instance::Repeated(vec![generic_item("Secret", A, None)]),
        ),
    ]);
    let error = format_xml::to_string(&schema, &instance).unwrap_err();
    assert!(is_excluded_element(&error, "Secret", A), "{error:?}");
}

#[test]
fn excluded_names_are_checked_after_the_namespace() {
    let wildcard = elements(negated())
        .with_xml_wildcard_excluded_names(secret())
        .unwrap();
    let schema = root(vec![head(), wildcard]);
    let error = format_xml::from_str(
        &format!("{DOCUMENT}<t:Head>h</t:Head><t:Secret/></t:Root>"),
        &schema,
    )
    .unwrap_err();
    assert!(
        matches!(&error, XmlFormatError::XmlWildcardNamespaceMismatch { name, .. } if name == "Secret"),
        "{error:?}"
    );
}

#[test]
fn native_attribute_io_refuses_only_excluded_names() {
    let wildcard = attributes(
        XmlWildcardNamespaceConstraint::Any,
        XmlWildcardProcessContents::Skip,
    )
    .with_xml_wildcard_excluded_names(excluded(&[(XmlNamespace::Unqualified, "internal")]))
    .unwrap();
    let schema = root(vec![head(), declared_id(), wildcard]);

    let read = |attributes: &str| {
        format_xml::from_str(
            &format!(
                r#"<t:Root xmlns:t="urn:ferrule:wx:t" {attributes}><t:Head>h</t:Head></t:Root>"#
            ),
            &schema,
        )
    };
    let error = read(r#"internal="x""#).unwrap_err();
    assert!(
        is_excluded_attribute(&error, "internal", "##local"),
        "{error:?}"
    );

    // With exclusions, a skip group leaves declared attributes to their typed
    // fields, on read and on write.
    let instance = read(r#"id="1" mode="m""#).unwrap();
    let generic = instance
        .field(XML_ATTRIBUTES_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(generic.len(), 1);
    assert_eq!(
        generic[0]
            .field(XML_LOCAL_NAME_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("mode".into()))
    );
    let written = format_xml::to_string(&schema, &instance).unwrap();
    assert_eq!(format_xml::from_str(&written, &schema).unwrap(), instance);

    let attribute_instance = |local: &str| {
        group(vec![
            ("Head", text("h")),
            (
                XML_ATTRIBUTES_FIELD,
                Instance::Repeated(vec![generic_item(local, "", Some("x"))]),
            ),
        ])
    };
    let error = format_xml::to_string(&schema, &attribute_instance("internal")).unwrap_err();
    assert!(
        is_excluded_attribute(&error, "internal", "##local"),
        "{error:?}"
    );
    let error = format_xml::to_string(&schema, &attribute_instance("id")).unwrap_err();
    assert!(
        matches!(&error, XmlFormatError::DuplicateField { field, .. } if field == "id"),
        "{error:?}"
    );
}

#[test]
fn excluded_names_precede_strict_attribute_checks() {
    let wildcard = attributes(
        XmlWildcardNamespaceConstraint::Any,
        XmlWildcardProcessContents::Strict,
    )
    .with_xml_wildcard_excluded_names(excluded(&[(XmlNamespace::Unqualified, "id")]))
    .unwrap();
    let schema = root(vec![head(), declared_id(), wildcard]);
    let read = |attributes: &str| {
        format_xml::from_str(
            &format!(
                r#"<t:Root xmlns:t="urn:ferrule:wx:t" {attributes}><t:Head>h</t:Head></t:Root>"#
            ),
            &schema,
        )
    };
    // A declared attribute is typed and never reaches the wildcard.
    read(r#"id="1""#).unwrap();
    let error = read(r#"other="1""#).unwrap_err();
    assert!(
        matches!(&error, XmlFormatError::UndeclaredStrictXmlWildcardAttribute { name, .. } if name == "other"),
        "{error:?}"
    );

    // Writing an excluded name that is also declared reports the exclusion.
    let instance = group(vec![
        ("Head", text("h")),
        (
            XML_ATTRIBUTES_FIELD,
            Instance::Repeated(vec![generic_item("id", "", Some("x"))]),
        ),
    ]);
    let error = format_xml::to_string(&schema, &instance).unwrap_err();
    assert!(is_excluded_attribute(&error, "id", "##local"), "{error:?}");
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
    // The anchor itself still refuses its own excluded name.
    let error = format_xml::from_str(
        r#"<Root xmlns:a="urn:ferrule:wx:a"><Closed><a:Secret/></Closed></Root>"#,
        &schema,
    )
    .unwrap_err();
    assert!(is_excluded_element(&error, "Secret", A), "{error:?}");
}
