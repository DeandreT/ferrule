//! Import refusals for XSD 1.1 wildcard exclusions, and the refusals that
//! still apply when exclusions are present (`R` in
//! `docs/design/xml-wildcard-exclusions.md`).

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{XmlFormatError, xsd};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xsd-wildcard-exclusion-refusals-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write(&self, name: &str, contents: &str) -> Result<PathBuf, std::io::Error> {
        let path = self.0.join(name);
        std::fs::write(&path, contents)?;
        Ok(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Element,
    Attribute,
}

fn refusal(kind: Kind, error: &XmlFormatError, expected: &str) -> bool {
    match (kind, error) {
        (Kind::Element, XmlFormatError::UnsupportedXmlWildcard { reason })
        | (Kind::Attribute, XmlFormatError::UnsupportedXmlAttributeWildcard { reason }) => {
            reason.contains(expected)
        }
        _ => false,
    }
}

fn import(directory: &TempDirectory, schema: &str) -> Result<(), XmlFormatError> {
    let path = directory
        .write("root.xsd", schema)
        .map_err(XmlFormatError::Io)?;
    xsd::import_root(&path, Some("Root")).map(|_| ())
}

/// A namespace-less schema around one `Root` complex type body.
fn root_schema(namespaces: &str, body: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" {namespaces}>
  <xs:element name="Root"><xs:complexType>{body}</xs:complexType></xs:element>
</xs:schema>"#
    )
}

fn any(attributes: &str) -> String {
    format!(
        r#"<xs:sequence><xs:any {attributes} minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"#
    )
}

fn any_attribute(attributes: &str) -> String {
    format!(r#"<xs:anyAttribute {attributes}/>"#)
}

fn assert_refused(kind: Kind, label: &str, schema: &str, expected: &str) {
    let directory = TempDirectory::new().unwrap();
    match import(&directory, schema) {
        Err(error) => assert!(refusal(kind, &error, expected), "{label}: {error:?}"),
        Ok(()) => panic!("{label}: imported successfully"),
    }
}

#[test]
fn rejects_each_exclusion_refusal_row() {
    let too_many_namespaces = (0..65)
        .map(|index| format!("urn:n{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let element_cases = [
        (
            "namespace-and-not-namespace",
            any(r###"namespace="##any" notNamespace="urn:x" processContents="lax""###),
            "namespace and notNamespace",
        ),
        (
            "empty-not-namespace",
            any(r#"notNamespace="" processContents="lax""#),
            "at least one namespace",
        ),
        (
            "over-limit-not-namespace",
            any(&format!(
                r#"notNamespace="{too_many_namespaces}" processContents="lax""#
            )),
            "64-member",
        ),
        (
            "not-namespace-any",
            any(r###"notNamespace="##any" processContents="lax""###),
            "##any or ##other",
        ),
        (
            "not-namespace-other",
            any(r###"notNamespace="urn:x ##other" processContents="lax""###),
            "##any or ##other",
        ),
        (
            "not-namespace-unknown-token",
            any(r###"notNamespace="##unknown" processContents="lax""###),
            "unsupported special token",
        ),
        (
            "undeclared-prefix",
            any(r#"notQName="missing:Name" processContents="lax""#),
            "undeclared namespace prefix",
        ),
        (
            "invalid-qname",
            any(r#"notQName="1Name" processContents="lax""#),
            "lexically invalid QName",
        ),
        (
            "invalid-prefix",
            any(r#"xmlns:p="urn:p" notQName=":Name" processContents="lax""#),
            "lexically invalid QName",
        ),
        (
            "disallowed-namespace",
            any(
                r###"namespace="##local" xmlns:p="urn:p" notQName="p:Name" processContents="lax""###,
            ),
            "does not allow",
        ),
        (
            "versioning-on-wildcard",
            any(
                r#"xmlns:vc="http://www.w3.org/2007/XMLSchema-versioning" vc:minVersion="1.1" notQName="Name" processContents="lax""#,
            ),
            "vc:",
        ),
        (
            "foreign-namespace-attribute",
            any(r#"xmlns:f="urn:foreign" f:notQName="Name" processContents="lax""#),
            "foreign namespace",
        ),
        (
            "strict",
            any(r#"notQName="Name" processContents="strict""#),
            "strict",
        ),
        ("defaulted-strict", any(r#"notQName="Name""#), "strict"),
        (
            "too-many-names",
            any(&format!(
                r#"notQName="{}" processContents="lax""#,
                (0..257)
                    .map(|index| format!("N{index}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )),
            "more than 256",
        ),
        (
            "too-many-qname-tokens",
            any(&format!(
                r#"notQName="{}" processContents="lax""#,
                vec!["Name"; 1025].join(" ")
            )),
            "1024-token",
        ),
        (
            "sibling-without-name",
            r###"<xs:sequence><xs:element type="xs:string"/>
              <xs:any namespace="##any" notQName="##definedSibling" processContents="lax"
                      minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"###
                .to_string(),
            "no exact expanded name",
        ),
    ];
    for (label, body, expected) in element_cases {
        assert_refused(Kind::Element, label, &root_schema("", &body), expected);
    }
    // The versioning attribute on an ancestor counts as well.
    assert_refused(
        Kind::Element,
        "versioning-on-ancestor",
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:vc="http://www.w3.org/2007/XMLSchema-versioning" vc:minVersion="1.1">
              <xs:element name="Root"><xs:complexType>{}</xs:complexType></xs:element>
            </xs:schema>"#,
            any(r#"notQName="Name" processContents="lax""#)
        ),
        "vc:",
    );

    let attribute_cases = [
        (
            "sibling-keyword",
            any_attribute(r###"notQName="##definedSibling" processContents="skip""###),
            "##definedSibling",
        ),
        (
            "namespace-and-not-namespace",
            any_attribute(r###"namespace="##local" notNamespace="urn:x""###),
            "namespace and notNamespace",
        ),
        (
            "over-limit-not-namespace",
            any_attribute(&format!(r#"notNamespace="{too_many_namespaces}""#)),
            "64-member",
        ),
        (
            "disallowed-namespace",
            any_attribute(r###"notNamespace="##local" notQName="name""###),
            "does not allow",
        ),
        (
            "undeclared-prefix",
            any_attribute(r#"notQName="missing:name""#),
            "undeclared namespace prefix",
        ),
    ];
    for (label, body, expected) in attribute_cases {
        assert_refused(Kind::Attribute, label, &root_schema("", &body), expected);
    }
}

#[test]
fn defined_requires_a_closed_declaration_set() -> Result<(), Box<dyn Error>> {
    // R-unresolved: an import without a schemaLocation that `##any` allows.
    let unresolved = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:import namespace="urn:ferrule:wx:missing"/>
  <xs:element name="Root"><xs:complexType>
    <xs:sequence><xs:any namespace="##any" notQName="##defined" processContents="lax"
        minOccurs="0" maxOccurs="unbounded"/></xs:sequence>
    <xs:anyAttribute notQName="##defined" processContents="skip"/>
  </xs:complexType></xs:element>
</xs:schema>"###;
    assert_refused(
        Kind::Element,
        "R-unresolved",
        unresolved,
        "closed declaration set",
    );
    let attributes_only = unresolved.replace(
        r###"<xs:sequence><xs:any namespace="##any" notQName="##defined" processContents="lax"
        minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"###,
        "",
    );
    assert_refused(
        Kind::Attribute,
        "R-unresolved attributes",
        &attributes_only,
        "closed declaration set",
    );

    // A namespace the constraint rejects leaves the set closed.
    let directory = TempDirectory::new()?;
    import(
        &directory,
        &unresolved
            .replace(r###"namespace="##any""###, r###"namespace="##local""###)
            .replace(
                r###"<xs:anyAttribute notQName="##defined" processContents="skip"/>"###,
                "",
            ),
    )?;

    for redefinition in ["redefine", "override"] {
        let directory = TempDirectory::new()?;
        directory.write(
            "base.xsd",
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"/>"#,
        )?;
        let schema = format!(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:{redefinition} schemaLocation="base.xsd"/>
  <xs:element name="Root"><xs:complexType>{}</xs:complexType></xs:element>
</xs:schema>"###,
            any(r###"namespace="##local" notQName="##defined" processContents="lax""###)
        );
        match import(&directory, &schema) {
            Err(error) => assert!(
                refusal(Kind::Element, &error, "xs:redefine or xs:override"),
                "{redefinition}: {error:?}"
            ),
            Ok(()) => panic!("{redefinition}: imported successfully"),
        }
    }
    Ok(())
}

/// R-chameleon: one namespace-less document included from two target
/// namespaces cannot resolve `##targetNamespace` or a namespace-less QName.
#[test]
fn chameleon_documents_with_several_contexts_are_refused() -> Result<(), Box<dyn Error>> {
    for (label, attributes, expected) in [
        (
            "target-namespace",
            r###"notNamespace="##targetNamespace""###,
            "##targetNamespace is unknowable",
        ),
        (
            "namespace-less-qname",
            r#"notQName="Name""#,
            "namespace-less",
        ),
    ] {
        let directory = TempDirectory::new()?;
        directory.write(
            "chameleon.xsd",
            &format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:group name="Open"><xs:sequence>
    <xs:any {attributes} processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:group>
</xs:schema>"#
            ),
        )?;
        directory.write(
            "other.xsd",
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:ferrule:wx:a">
  <xs:include schemaLocation="chameleon.xsd"/>
</xs:schema>"#,
        )?;
        let root = directory.write(
            "root.xsd",
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:wx:t" targetNamespace="urn:ferrule:wx:t">
  <xs:include schemaLocation="chameleon.xsd"/>
  <xs:import namespace="urn:ferrule:wx:a" schemaLocation="other.xsd"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:group ref="t:Open"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
        )?;
        match xsd::import_root(&root, Some("{urn:ferrule:wx:t}Root")) {
            Err(error) => assert!(
                refusal(Kind::Element, &error, expected),
                "{label}: {error:?}"
            ),
            Ok(_) => panic!("{label}: imported successfully"),
        }
    }
    Ok(())
}

/// A chameleon document reached through one context adopts its namespace.
#[test]
fn chameleon_documents_with_one_context_adopt_its_namespace() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    directory.write(
        "chameleon.xsd",
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:group name="Open"><xs:sequence>
    <xs:any notNamespace="##targetNamespace ##local" notQName="Name"
            processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:group>
</xs:schema>"###,
    )?;
    let root = directory.write(
        "root.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:wx:t" targetNamespace="urn:ferrule:wx:t">
  <xs:include schemaLocation="chameleon.xsd"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:group ref="t:Open"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    // `Name` takes the adopted namespace, which `notNamespace` rejects.
    match xsd::import_root(&root, Some("{urn:ferrule:wx:t}Root")) {
        Err(error) => assert!(
            refusal(Kind::Element, &error, "does not allow"),
            "{error:?}"
        ),
        Ok(_) => panic!("imported a name the wildcard does not allow"),
    }
    Ok(())
}

/// R-retained: every current refusal still applies with a `notQName` added.
#[test]
fn current_refusals_still_apply_with_exclusions() {
    let exclusion = r#"notQName="Blocked""#;
    let cases = [
        (
            "all",
            format!(
                r###"<xs:all><xs:any namespace="##local" {exclusion} processContents="skip" minOccurs="0" maxOccurs="unbounded"/></xs:all>"###
            ),
            "xs:any inside xs:all",
        ),
        (
            "required-unbounded",
            format!(
                r###"<xs:sequence><xs:any namespace="##local" {exclusion} processContents="skip" minOccurs="1" maxOccurs="unbounded"/></xs:sequence>"###
            ),
            "minOccurs=\"0\"",
        ),
        (
            "bounded",
            format!(
                r###"<xs:sequence><xs:any namespace="##local" {exclusion} processContents="skip" minOccurs="0" maxOccurs="2"/></xs:sequence>"###
            ),
            "maxOccurs=\"1\" or maxOccurs=\"unbounded\"",
        ),
        (
            "bounded-minimum",
            format!(
                r###"<xs:sequence><xs:any namespace="##local" {exclusion} processContents="skip" minOccurs="2" maxOccurs="unbounded"/></xs:sequence>"###
            ),
            "minOccurs=\"0\" or minOccurs=\"1\"",
        ),
        (
            "optional-single-skip",
            format!(
                r###"<xs:sequence><xs:any namespace="##local" {exclusion} processContents="skip" minOccurs="0"/></xs:sequence>"###
            ),
            "explicit occurrence metadata",
        ),
        (
            "several-wildcards",
            format!(
                r###"<xs:sequence><xs:any namespace="##local" {exclusion} processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
                  <xs:any namespace="##other" processContents="lax" minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"###
            ),
            "multiple element wildcard",
        ),
        (
            "skip-beside-declared",
            format!(
                r###"<xs:sequence><xs:element name="Head" type="xs:string"/>
                  <xs:any namespace="##local" {exclusion} processContents="skip" minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"###
            ),
            "skip-validation element wildcard beside declared elements",
        ),
        (
            "strict-unresolved",
            format!(
                r###"<xs:sequence><xs:any namespace="##local" {exclusion} processContents="strict" minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"###
            ),
            "strict",
        ),
    ];
    for (label, body, expected) in cases {
        assert_refused(Kind::Element, label, &root_schema("", &body), expected);
    }
    assert_refused(
        Kind::Attribute,
        "several-attribute-wildcards",
        &root_schema(
            "",
            &format!(
                r###"<xs:anyAttribute namespace="##local" {exclusion} processContents="skip"/><xs:anyAttribute namespace="##local" processContents="skip"/>"###
            ),
        ),
        "multiple attribute wildcard",
    );

    // A wildcard in a complex-content restriction.
    let restriction = format!(
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="Base"><xs:sequence>
    <xs:element name="Value" type="xs:string"/>
  </xs:sequence></xs:complexType>
  <xs:element name="Root"><xs:complexType><xs:complexContent>
    <xs:restriction base="Base">
      <xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence>
      <xs:anyAttribute {exclusion} processContents="skip"/>
    </xs:restriction>
  </xs:complexContent></xs:complexType></xs:element>
</xs:schema>"###
    );
    let directory = TempDirectory::new().unwrap();
    let result = import(&directory, &restriction);
    assert!(
        result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("wildcard restriction")),
        "{result:?}"
    );
}

#[test]
fn empty_exclusions_record_no_metadata() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let path = directory.write(
        "root.xsd",
        &root_schema(
            "",
            &format!(
                "{}{}",
                any(r###"namespace="##local" notQName="" processContents="lax""###),
                any_attribute(r###"namespace="##local" notQName="" processContents="skip""###)
            ),
        ),
    )?;
    let with = xsd::import_root(&path, Some("Root"))?;
    std::fs::write(
        &path,
        root_schema(
            "",
            &format!(
                "{}{}",
                any(r###"namespace="##local" processContents="lax""###),
                any_attribute(r###"namespace="##local" processContents="skip""###)
            ),
        ),
    )?;
    assert_eq!(with, xsd::import_root(&path, Some("Root"))?);
    Ok(())
}

#[test]
fn repeated_qname_tokens_are_one_member_up_to_the_token_limit() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let path = directory.write(
        "root.xsd",
        &root_schema(
            "",
            &any(&format!(
                r#"notQName="{}" processContents="lax""#,
                vec!["Name"; 1024].join(" ")
            )),
        ),
    )?;
    let schema = xsd::import_root(&path, Some("Root"))?;
    let names = schema
        .child(ir::XML_ELEMENTS_FIELD)
        .and_then(|wildcard| wildcard.xml_wildcard_excluded_names.as_ref())
        .map(|names| names.iter().count());
    assert_eq!(names, Some(1));
    Ok(())
}

/// `vc:` attributes refuse only wildcards that carry exclusions.
#[test]
fn versioning_attributes_leave_wildcards_without_exclusions_alone() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    import(
        &directory,
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:vc="http://www.w3.org/2007/XMLSchema-versioning" vc:minVersion="1.1">
              <xs:element name="Root"><xs:complexType>{}{}</xs:complexType></xs:element>
            </xs:schema>"#,
            any(r###"namespace="##local" processContents="lax""###),
            any_attribute(r###"namespace="##local" processContents="skip""###)
        ),
    )?;
    Ok(())
}

/// A chameleon document reached through several namespace contexts declares
/// local elements whose namespace is unknowable, so `##definedSibling`
/// cannot exclude them exactly.
#[test]
fn siblings_from_multi_context_chameleon_documents_are_refused() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    directory.write(
        "chameleon.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:group name="Fields"><xs:sequence>
    <xs:element name="Local" type="xs:string"/>
  </xs:sequence></xs:group>
</xs:schema>"#,
    )?;
    directory.write(
        "other.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:ferrule:wx:a">
  <xs:include schemaLocation="chameleon.xsd"/>
</xs:schema>"#,
    )?;
    let root = |contexts: &str| {
        format!(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:wx:t" targetNamespace="urn:ferrule:wx:t">
  <xs:include schemaLocation="chameleon.xsd"/>
  {contexts}
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:group ref="t:Fields"/>
    <xs:any namespace="##any" notQName="##definedSibling" processContents="lax"
            minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###
        )
    };
    let several = directory.write(
        "several.xsd",
        &root(r#"<xs:import namespace="urn:ferrule:wx:a" schemaLocation="other.xsd"/>"#),
    )?;
    match xsd::import_root(&several, Some("{urn:ferrule:wx:t}Root")) {
        Err(error) => assert!(
            refusal(Kind::Element, &error, "no exact expanded name"),
            "{error:?}"
        ),
        Ok(_) => panic!("imported a sibling with an unknowable namespace"),
    }
    // Through one context the sibling takes that context's local form.
    let one = directory.write("one.xsd", &root(""))?;
    let schema = xsd::import_root(&one, Some("{urn:ferrule:wx:t}Root"))?;
    let names = schema
        .child(ir::XML_ELEMENTS_FIELD)
        .and_then(|wildcard| wildcard.xml_wildcard_excluded_names.as_ref())
        .map(|names| {
            names
                .iter()
                .map(|name| {
                    (
                        name.namespace().uri().map(str::to_string),
                        name.local().to_string(),
                    )
                })
                .collect::<Vec<_>>()
        });
    assert_eq!(names, Some(vec![(None, "Local".to_string())]));
    Ok(())
}
