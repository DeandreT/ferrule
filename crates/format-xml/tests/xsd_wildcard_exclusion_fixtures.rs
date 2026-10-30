//! The frozen XSD 1.1 wildcard-exclusion fixtures E1–E5 and A1 from
//! `docs/design/xml-wildcard-exclusions.md`: import shape, read rows, write
//! rows, and export/re-import equality.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{XmlFormatError, from_str, to_string, xsd};
use ir::{
    Instance, SchemaNode, Value, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD,
    XML_NAMESPACE_URI_FIELD, XML_TEXT_FIELD, XmlNamespace, XmlWildcardNamespaceConstraint,
    XmlWildcardProcessContents,
};

const T: &str = "urn:ferrule:wx:t";
const A: &str = "urn:ferrule:wx:a";
const B: &str = "urn:ferrule:wx:b";
const X: &str = "http://www.w3.org/XML/1998/namespace";
const ROOT: &str = "{urn:ferrule:wx:t}Root";

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xsd-wildcard-exclusions-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Writes `a.xsd`, `b.xsd` and a T-qualified `root.xsd` with `body`.
    fn schema(&self, form: &str, body: &str) -> Result<PathBuf, std::io::Error> {
        std::fs::write(
            self.path("a.xsd"),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{A}" elementFormDefault="qualified">
  <xs:element name="Known" type="xs:string"/>
  <xs:element name="Note" type="xs:string"/>
  <xs:element name="Secret" type="xs:string"/>
  <xs:attribute name="flag" type="xs:string"/>
</xs:schema>"#
            ),
        )?;
        std::fs::write(
            self.path("b.xsd"),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{B}" elementFormDefault="qualified">
  <xs:element name="Note" type="xs:string"/>
</xs:schema>"#
            ),
        )?;
        let root = self.path("root.xsd");
        std::fs::write(
            &root,
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="{T}" xmlns:a="{A}" xmlns:b="{B}"
    targetNamespace="{T}" elementFormDefault="{form}">
{body}
</xs:schema>"#
            ),
        )?;
        Ok(root)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const IMPORT_A: &str = r#"<xs:import namespace="urn:ferrule:wx:a" schemaLocation="a.xsd"/>"#;
const IMPORT_B: &str = r#"<xs:import namespace="urn:ferrule:wx:b" schemaLocation="b.xsd"/>"#;

fn qualified(uri: &str) -> XmlNamespace {
    XmlNamespace::qualified(uri).unwrap()
}

fn document(content: &str) -> String {
    format!(r#"<t:Root xmlns:t="{T}" xmlns:a="{A}" xmlns:b="{B}">{content}</t:Root>"#)
}

fn names(schema: &SchemaNode) -> Vec<&str> {
    match &schema.kind {
        ir::SchemaKind::Group { children, .. } => {
            children.iter().map(|child| child.name.as_str()).collect()
        }
        _ => Vec::new(),
    }
}

/// The wildcard group's excluded names as `(namespace URI, local)` pairs.
fn excluded(wildcard: &SchemaNode) -> Vec<(Option<&str>, &str)> {
    wildcard
        .xml_wildcard_excluded_names
        .as_ref()
        .map(|names| {
            names
                .iter()
                .map(|name| (name.namespace().uri(), name.local()))
                .collect()
        })
        .unwrap_or_default()
}

fn field<'a>(schema: &'a SchemaNode, name: &str) -> Result<&'a SchemaNode, Box<dyn Error>> {
    schema
        .child(name)
        .ok_or_else(|| format!("missing field {name}").into())
}

fn generic_items(instance: &Instance, field: &str) -> Vec<(String, Option<String>)> {
    let text = |item: &Instance, name: &str| match item.field(name).and_then(Instance::as_scalar) {
        Some(Value::String(value)) => Some(value.clone()),
        _ => None,
    };
    instance
        .field(field)
        .and_then(Instance::as_repeated)
        .unwrap_or_default()
        .iter()
        .map(|item| {
            (
                text(item, XML_LOCAL_NAME_FIELD).unwrap_or_default(),
                text(item, XML_NAMESPACE_URI_FIELD),
            )
        })
        .collect()
}

fn scalar(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
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

/// A root with `Head` and one generic item in `field`.
fn with_generic(field: &str, local: &str, namespace: Option<&str>) -> Instance {
    let mut item = vec![(XML_LOCAL_NAME_FIELD, scalar(local))];
    if let Some(namespace) = namespace {
        item.push((XML_NAMESPACE_URI_FIELD, scalar(namespace)));
    }
    if field == XML_ATTRIBUTES_FIELD {
        item.push((XML_TEXT_FIELD, scalar("x")));
    }
    group(vec![
        ("Head", scalar("h")),
        (field, Instance::Repeated(vec![group(item)])),
    ])
}

fn excluded_element(error: &XmlFormatError, local: &str, namespace: &str) -> bool {
    matches!(
        error,
        XmlFormatError::XmlWildcardExcludedName { name, namespace: actual }
            if name == local && actual == namespace
    )
}

fn namespace_mismatch(error: &XmlFormatError, local: &str, namespace: &str) -> bool {
    matches!(
        error,
        XmlFormatError::XmlWildcardNamespaceMismatch { name, namespace: actual }
            if name == local && actual == namespace
    )
}

/// Exports through the export set, re-imports `Root`, and checks equality.
fn assert_export_round_trip(
    directory: &TempDirectory,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), Box<dyn Error>> {
    let exported = xsd::export_set(schema, "exported.xsd")?;
    let root = directory.path("exported.xsd");
    std::fs::write(&root, &exported.root)?;
    for dependency in exported.dependencies {
        std::fs::write(directory.path(&dependency.filename), dependency.contents)?;
    }
    let reimported = xsd::import_root(&root, Some(ROOT))?;
    assert_eq!(&reimported, schema, "{}", exported.root);
    // The same document reads to the same instance under both schemas.
    let written = to_string(schema, instance)?;
    assert_eq!(
        from_str(&written, &reimported)?,
        from_str(&written, schema)?
    );
    Ok(())
}

#[test]
fn e1_negated_namespaces_with_an_explicit_name() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema(
        "qualified",
        &format!(
            r###"{IMPORT_A}
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:any xmlns:a="urn:ferrule:wx:a" notNamespace="##targetNamespace ##local"
            notQName="a:Secret" processContents="lax"
            minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>"###
        ),
    )?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    assert_eq!(
        names(&schema),
        ["Head", "Known", "Note", XML_ELEMENTS_FIELD]
    );
    assert_eq!(field(&schema, "Known")?.xml_namespace, Some(qualified(A)));
    assert_eq!(field(&schema, "Note")?.xml_namespace, Some(qualified(A)));
    let wildcard = field(&schema, XML_ELEMENTS_FIELD)?;
    assert_eq!(
        wildcard.xml_wildcard_process_contents,
        XmlWildcardProcessContents::Lax
    );
    assert!(matches!(
        &wildcard.xml_wildcard_namespace,
        Some(XmlWildcardNamespaceConstraint::Not { namespaces })
            if namespaces.iter().cloned().collect::<Vec<_>>()
                == [qualified(T), XmlNamespace::Unqualified]
    ));
    assert_eq!(excluded(wildcard), [(Some(A), "Secret")]);

    let accepted = from_str(
        &document("<t:Head/><a:Known>k</a:Known><b:Extra/>"),
        &schema,
    )?;
    assert_eq!(
        generic_items(&accepted, XML_ELEMENTS_FIELD),
        [("Extra".to_string(), Some(B.to_string()))]
    );
    for (content, local, namespace) in [
        ("<t:Head/><a:Other/>", "Other", A),
        // Exclusion matches the exact expanded name.
        ("<t:Head/><b:Secret/>", "Secret", B),
    ] {
        let instance = from_str(&document(content), &schema)?;
        assert_eq!(
            generic_items(&instance, XML_ELEMENTS_FIELD),
            [(local.to_string(), Some(namespace.to_string()))]
        );
    }
    let error = from_str(&document("<t:Head/><a:Secret>s</a:Secret>"), &schema).unwrap_err();
    assert!(excluded_element(&error, "Secret", A), "{error:?}");
    let error = from_str(&document("<t:Head/><t:Tail/>"), &schema).unwrap_err();
    assert!(namespace_mismatch(&error, "Tail", T), "{error:?}");
    let error = from_str(&document(r#"<t:Head/><Tail xmlns=""/>"#), &schema).unwrap_err();
    assert!(namespace_mismatch(&error, "Tail", "##local"), "{error:?}");

    let error = to_string(
        &schema,
        &with_generic(XML_ELEMENTS_FIELD, "Secret", Some(A)),
    )
    .unwrap_err();
    assert!(excluded_element(&error, "Secret", A), "{error:?}");
    assert!(matches!(
        to_string(&schema, &with_generic(XML_ELEMENTS_FIELD, "Known", Some(A))),
        Err(XmlFormatError::KnownXmlWildcardElementRequiresTypedField { name, namespace })
            if name == "Known" && namespace == A
    ));
    let extra = from_str(
        &to_string(&schema, &with_generic(XML_ELEMENTS_FIELD, "Extra", Some(B)))?,
        &schema,
    )?;
    assert_eq!(
        generic_items(&extra, XML_ELEMENTS_FIELD),
        [("Extra".to_string(), Some(B.to_string()))]
    );

    assert_export_round_trip(&directory, &schema, &accepted)
}

const E2: &str = r###"<xs:element name="Global" type="xs:string"/>
  <xs:group name="Open"><xs:sequence>
    <xs:any namespace="##targetNamespace" notQName="##defined ##definedSibling"
            processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:group>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:group ref="t:Open"/>
    <xs:element name="Tail" type="xs:string" minOccurs="0"/>
  </xs:sequence></xs:complexType></xs:element>"###;

#[test]
fn e2_keywords_through_a_named_group() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema("qualified", E2)?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    // The lax projection adds no typed fields.
    assert_eq!(names(&schema), ["Head", XML_ELEMENTS_FIELD, "Tail"]);
    let wildcard = field(&schema, XML_ELEMENTS_FIELD)?;
    // `Tail` follows the group reference: resolution waits for the type.
    assert_eq!(
        excluded(wildcard),
        [
            (Some(T), "Global"),
            (Some(T), "Head"),
            (Some(T), "Root"),
            (Some(T), "Tail")
        ]
    );

    let accepted = from_str(&document("<t:Head/><t:Note>n</t:Note><t:Tail/>"), &schema)?;
    assert_eq!(
        generic_items(&accepted, XML_ELEMENTS_FIELD),
        [("Note".to_string(), Some(T.to_string()))]
    );
    for local in ["Global", "Root", "Head"] {
        let error = from_str(&document(&format!("<t:Head/><t:{local}/>")), &schema).unwrap_err();
        assert!(excluded_element(&error, local, T), "{local}: {error:?}");
    }
    let error = from_str(&document("<t:Head/><b:X/>"), &schema).unwrap_err();
    assert!(namespace_mismatch(&error, "X", B), "{error:?}");

    let error = to_string(&schema, &with_generic(XML_ELEMENTS_FIELD, "Tail", Some(T))).unwrap_err();
    assert!(excluded_element(&error, "Tail", T), "{error:?}");

    // The single-document export writes the four names as explicit QNames.
    let exported = xsd::export(&schema)?;
    assert!(
        exported.contains(r#"notQName="fwx1:Global fwx1:Head fwx1:Root fwx1:Tail""#),
        "{exported}"
    );
    assert!(!exported.contains("##defined"), "{exported}");
    assert_export_round_trip(&directory, &schema, &accepted)
}

fn e3(not_qname: &str, process: &str) -> String {
    format!(
        r###"{IMPORT_A}
  {IMPORT_B}
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:any namespace="##other" notQName="{not_qname}"{process}
            minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>"###
    )
}

#[test]
fn e3_same_local_names_across_imports() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema("qualified", &e3("a:Secret", r#" processContents="lax""#))?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    assert_eq!(
        names(&schema),
        ["Head", "Known", "Note", XML_ELEMENTS_FIELD]
    );
    let note = field(&schema, "Note")?;
    assert_eq!(note.xml_namespace, Some(qualified(A)));
    assert_eq!(note.xml_name_alternatives, [qualified(B)]);
    assert_eq!(
        excluded(field(&schema, XML_ELEMENTS_FIELD)?),
        [(Some(A), "Secret")]
    );
    let accepted = from_str(
        &document("<t:Head/><a:Note>a</a:Note><b:Note>b</b:Note>"),
        &schema,
    )?;
    assert!(generic_items(&accepted, XML_ELEMENTS_FIELD).is_empty());
    assert_export_round_trip(&directory, &schema, &accepted)?;

    let directory = TempDirectory::new()?;
    let root = directory.schema(
        "qualified",
        &e3("a:Secret a:Note", r#" processContents="lax""#),
    )?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    let note = field(&schema, "Note")?;
    assert_eq!(note.xml_namespace, Some(qualified(B)));
    assert!(note.xml_name_alternatives.is_empty());
    let accepted = from_str(&document("<t:Head/><b:Note>b</b:Note>"), &schema)?;
    assert!(generic_items(&accepted, XML_ELEMENTS_FIELD).is_empty());
    assert_eq!(
        accepted
            .field("Note")
            .and_then(Instance::as_repeated)
            .map(<[Instance]>::len),
        Some(1)
    );
    let error = from_str(&document("<t:Head/><a:Note>a</a:Note>"), &schema).unwrap_err();
    assert!(excluded_element(&error, "Note", A), "{error:?}");
    assert_export_round_trip(&directory, &schema, &accepted)?;

    // Strict element wildcards with exclusions stay refused.
    for process in [r#" processContents="strict""#, ""] {
        let directory = TempDirectory::new()?;
        let root = directory.schema("qualified", &e3("a:Secret", process))?;
        assert!(
            matches!(
                xsd::import_root(&root, Some(ROOT)),
                Err(XmlFormatError::UnsupportedXmlWildcard { reason }) if reason.contains("strict")
            ),
            "{process}"
        );
    }
    Ok(())
}

#[test]
fn e4_siblings_outside_the_namespace_constraint() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema(
        "qualified",
        &format!(
            r###"{IMPORT_A}
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:element ref="a:Known"/>
    <xs:any namespace="##other" notQName="##definedSibling"
            processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>"###
        ),
    )?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    // `##other` already rejects T, so `{T}Head` is dropped.
    assert_eq!(
        excluded(field(&schema, XML_ELEMENTS_FIELD)?),
        [(Some(A), "Known")]
    );
    assert_eq!(
        names(&schema),
        ["Head", "Known", "Note", "Secret", XML_ELEMENTS_FIELD]
    );
    assert!(!field(&schema, "Known")?.repeating);
    assert!(field(&schema, "Note")?.repeating);

    let accepted = from_str(&document("<t:Head/><a:Known/><b:Y/>"), &schema)?;
    assert_eq!(
        generic_items(&accepted, XML_ELEMENTS_FIELD),
        [("Y".to_string(), Some(B.to_string()))]
    );
    let error =
        to_string(&schema, &with_generic(XML_ELEMENTS_FIELD, "Known", Some(A))).unwrap_err();
    assert!(excluded_element(&error, "Known", A), "{error:?}");
    assert_export_round_trip(&directory, &schema, &accepted)
}

const E5: &str = r###"<xs:complexType name="Base"><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:any namespace="##local" notQName="##definedSibling"
            processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType>
  <xs:complexType name="Derived"><xs:complexContent>
    <xs:extension base="t:Base"><xs:sequence>
      <xs:element name="Extra" type="xs:string" minOccurs="0"/>
      <xs:element name="Gone" type="xs:string" minOccurs="0" maxOccurs="0"/>
    </xs:sequence></xs:extension>
  </xs:complexContent></xs:complexType>
  <xs:element name="Plain" type="t:Base"/>
  <xs:element name="Root" type="t:Derived"/>"###;

#[test]
fn e5_extension_and_disabled_particles() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema("unqualified", E5)?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    assert!(schema.alternatives().is_empty());
    assert_eq!(names(&schema), ["Head", XML_ELEMENTS_FIELD, "Extra"]);
    // A set reused from the cached `Base` expansion would hold only `Head`.
    assert_eq!(
        excluded(field(&schema, XML_ELEMENTS_FIELD)?),
        [(None, "Extra"), (None, "Head")]
    );

    let root_document = |content: &str| format!(r#"<t:Root xmlns:t="{T}">{content}</t:Root>"#);
    for (content, local) in [("<Head/><Gone/>", "Gone"), ("<Head/><Note/>", "Note")] {
        let instance = from_str(&root_document(content), &schema)?;
        assert_eq!(
            generic_items(&instance, XML_ELEMENTS_FIELD),
            [(local.to_string(), None)]
        );
    }
    let error = to_string(&schema, &with_generic(XML_ELEMENTS_FIELD, "Extra", None)).unwrap_err();
    assert!(excluded_element(&error, "Extra", "##local"), "{error:?}");
    let error = from_str(&root_document("<Head/><Head/>"), &schema).unwrap_err();
    assert!(excluded_element(&error, "Head", "##local"), "{error:?}");

    let accepted = from_str(&root_document("<Head/><Gone/>"), &schema)?;
    assert_export_round_trip(&directory, &schema, &accepted)?;

    // One shared `element()` field cannot hold `Base`'s and `Derived`'s sets.
    assert!(matches!(
        xsd::import_root(&root, Some("{urn:ferrule:wx:t}Plain")),
        Err(XmlFormatError::UnsupportedXmlWildcard { reason })
            if reason.contains("xsi:type")
    ));
    Ok(())
}

#[test]
fn a1_attribute_exclusions() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema(
        "qualified",
        &format!(
            r###"{IMPORT_A}
  <xs:element name="Root"><xs:complexType>
    <xs:attribute name="id" type="xs:string"/>
    <xs:anyAttribute notNamespace="##targetNamespace"
                     notQName="##defined id internal xml:space"
                     processContents="skip"/>
  </xs:complexType></xs:element>"###
        ),
    )?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    assert_eq!(names(&schema), [XML_ATTRIBUTES_FIELD, "id"]);
    let wildcard = field(&schema, XML_ATTRIBUTES_FIELD)?;
    assert!(wildcard.child(XML_NAMESPACE_URI_FIELD).is_some());
    assert_eq!(
        excluded(wildcard),
        [
            (None, "id"),
            (None, "internal"),
            (Some(X), "space"),
            (Some(A), "flag")
        ]
    );

    let element = |attributes: &str| {
        format!(r#"<t:Root xmlns:t="{T}" xmlns:a="{A}" xmlns:b="{B}" {attributes}/>"#)
    };
    let accepted = from_str(
        &element(r#"id="1" mode="m" b:z="z" xml:lang="en""#),
        &schema,
    )?;
    assert_eq!(
        accepted.field("id").and_then(Instance::as_scalar),
        Some(&Value::String("1".into()))
    );
    assert_eq!(
        generic_items(&accepted, XML_ATTRIBUTES_FIELD),
        [
            ("mode".to_string(), None),
            ("z".to_string(), Some(B.to_string())),
            ("lang".to_string(), Some(X.to_string()))
        ]
    );
    for (attributes, local, namespace) in [
        (r#"internal="x""#, "internal", "##local"),
        (r#"a:flag="x""#, "flag", A),
        (r#"xml:space="preserve""#, "space", X),
    ] {
        assert!(
            matches!(
                from_str(&element(attributes), &schema),
                Err(XmlFormatError::XmlAttributeWildcardExcludedName { name, namespace: actual })
                    if name == local && actual == namespace
            ),
            "{attributes}"
        );
    }
    assert!(matches!(
        from_str(&element(r#"t:code="x""#), &schema),
        Err(XmlFormatError::XmlAttributeWildcardNamespaceMismatch { name, .. }) if name == "code"
    ));

    for local in ["id", "internal"] {
        let mut instance = with_generic(XML_ATTRIBUTES_FIELD, local, None);
        if let Instance::Group(fields) = &mut instance {
            fields.retain(|(name, _)| name != "Head");
        }
        assert!(
            matches!(
                to_string(&schema, &instance),
                Err(XmlFormatError::XmlAttributeWildcardExcludedName { name, .. }) if name == local
            ),
            "{local}"
        );
    }

    assert_export_round_trip(&directory, &schema, &accepted)
}

/// `##definedSibling` also covers the concrete substitution members of a
/// declared sibling, and the lax projection keeps no port for them.
#[test]
fn defined_sibling_includes_substitution_members() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema(
        "qualified",
        r###"<xs:complexType name="ItemType"><xs:sequence>
    <xs:element name="Title" type="xs:string"/>
  </xs:sequence></xs:complexType>
  <xs:element name="Item" type="t:ItemType" abstract="true"/>
  <xs:element name="Book" type="t:ItemType" substitutionGroup="t:Item"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element ref="t:Item"/>
    <xs:any namespace="##targetNamespace" notQName="##definedSibling"
            processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>"###,
    )?;
    let schema = xsd::import_root(&root, Some(ROOT))?;
    assert_eq!(names(&schema), ["Item", XML_ELEMENTS_FIELD]);
    assert_eq!(
        excluded(field(&schema, XML_ELEMENTS_FIELD)?),
        [(Some(T), "Book"), (Some(T), "Item")]
    );
    let mut book = with_generic(XML_ELEMENTS_FIELD, "Book", Some(T));
    if let Instance::Group(fields) = &mut book {
        fields.retain(|(name, _)| name != "Head");
    }
    let error = to_string(&schema, &book).unwrap_err();
    assert!(excluded_element(&error, "Book", T), "{error:?}");
    Ok(())
}

/// A projected port that shares its local name with a declared sibling is
/// judged on every name its declarations have, as without exclusions: a
/// partly covered `lax` port is dropped, so `{B}Note` becomes generic instead
/// of a second `Note` field.
#[test]
fn partly_excluded_ports_beside_a_declared_sibling_are_dropped() -> Result<(), Box<dyn Error>> {
    let declared = r#"<xs:element ref="a:Note"/>"#;
    let wildcard = |not_qname: &str| {
        format!(
            r###"<xs:any namespace="##other" notQName="{not_qname}"
            processContents="lax" minOccurs="0" maxOccurs="unbounded"/>"###
        )
    };
    for not_qname in ["##definedSibling", "a:Note"] {
        for declared_first in [true, false] {
            let label = format!("{not_qname}, declared first: {declared_first}");
            let particles = if declared_first {
                format!("{declared}{}", wildcard(not_qname))
            } else {
                format!("{}{declared}", wildcard(not_qname))
            };
            let directory = TempDirectory::new()?;
            let root = directory.schema(
                "qualified",
                &format!(
                    r###"{IMPORT_A}
  {IMPORT_B}
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    {particles}
  </xs:sequence></xs:complexType></xs:element>"###
                ),
            )?;
            let schema = xsd::import_root(&root, Some(ROOT))?;
            let note = field(&schema, "Note")?;
            assert_eq!(note.xml_namespace, Some(qualified(A)), "{label}");
            assert!(note.xml_name_alternatives.is_empty(), "{label}");
            assert!(!note.repeating, "{label}");
            assert_eq!(
                excluded(field(&schema, XML_ELEMENTS_FIELD)?),
                [(Some(A), "Note")],
                "{label}"
            );
            assert!(names(&schema).contains(&"Known"), "{label}");

            let content = if declared_first {
                "<t:Head/><a:Note>a</a:Note><b:Note>b</b:Note>"
            } else {
                "<t:Head/><b:Note>b</b:Note><a:Note>a</a:Note>"
            };
            let accepted = from_str(&document(content), &schema)?;
            assert_eq!(
                accepted.field("Note").and_then(Instance::as_scalar),
                Some(&Value::String("a".into())),
                "{label}"
            );
            assert_eq!(
                generic_items(&accepted, XML_ELEMENTS_FIELD),
                [("Note".to_string(), Some(B.to_string()))],
                "{label}"
            );
            let error =
                to_string(&schema, &with_generic(XML_ELEMENTS_FIELD, "Note", Some(A))).unwrap_err();
            assert!(excluded_element(&error, "Note", A), "{label}: {error:?}");
            assert_export_round_trip(&directory, &schema, &accepted)?;
        }
    }
    Ok(())
}

/// The excluded-name sets are compared before any other field, so a
/// difference in a projected port cannot hide them.
#[test]
fn differing_sets_are_refused_before_other_alternative_differences() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = directory.schema(
        "qualified",
        &format!(
            r###"{IMPORT_A}
  {IMPORT_B}
  <xs:complexType name="Base"><xs:sequence>
    <xs:any namespace="##other" notQName="##definedSibling"
            processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType>
  <xs:complexType name="Derived"><xs:complexContent>
    <xs:extension base="t:Base"><xs:sequence>
      <xs:element ref="a:Note"/>
    </xs:sequence></xs:extension>
  </xs:complexContent></xs:complexType>
  <xs:element name="Plain" type="t:Base"/>"###
        ),
    )?;
    let result = xsd::import_root(&root, Some("{urn:ferrule:wx:t}Plain"));
    assert!(
        matches!(
            &result,
            Err(XmlFormatError::UnsupportedXmlWildcard { reason }) if reason.contains("xsi:type")
        ),
        "{result:?}"
    );
    Ok(())
}
