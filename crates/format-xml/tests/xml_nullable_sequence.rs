use format_xml::{XmlFormatError, xsd};
use ir::SchemaNode;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_nullable_sequence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn import(&self, decls: &str, particle: &str) -> Result<SchemaNode, XmlFormatError> {
        let text = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{decls}<xs:element name="Root"><xs:complexType>{particle}</xs:complexType></xs:element></xs:schema>"#
        );
        let path = self.0.join("source.xsd");
        std::fs::write(&path, text).unwrap();
        xsd::import_root(&path, Some("Root"))
    }
    fn roundtrip(&self, schema: &SchemaNode) {
        let set = xsd::export_set(schema, "export.xsd").unwrap();
        std::fs::write(self.0.join("export.xsd"), set.root).unwrap();
        for item in set.dependencies {
            std::fs::write(self.0.join(item.filename), item.contents).unwrap();
        }
        assert_eq!(
            xsd::import_root(&self.0.join("export.xsd"), Some("Root")).unwrap(),
            *schema
        );
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
const OPTIONAL: &str = r#"<xs:element name="Before" type="xs:string" minOccurs="0"/>"#;
const BRANCHES: &str =
    r#"<xs:element name="Left" type="xs:string"/><xs:element name="Right" type="xs:string"/>"#;
#[test]
fn nullable_element_members_make_only_the_outer_sequence_redundant() {
    let decls = r#"<xs:element name="After" type="xs:string"/>"#;
    for zero in ["0", "00", "000"] {
        let d = Dir::new();
        let content = format!(r#"{OPTIONAL}<xs:element ref="After" minOccurs="{zero}"/>"#);
        let optional = d
            .import(
                decls,
                &format!(r#"<xs:sequence minOccurs="{zero}">{content}</xs:sequence>"#),
            )
            .unwrap();
        assert!(optional.child("Before").unwrap().xml_optional);
        assert!(optional.child("After").unwrap().xml_optional);
        d.roundtrip(&optional);
        let required = d
            .import(decls, &format!("<xs:sequence>{content}</xs:sequence>"))
            .unwrap();
        assert_eq!(optional, required);
    }
}
#[test]
fn optional_ordered_choice_keeps_its_collective_absence_and_member_order() {
    for max in ["1", "unbounded"] {
        let d = Dir::new();
        let content = format!(
            r#"{OPTIONAL}<xs:choice minOccurs="0" maxOccurs="{max}">{BRANCHES}</xs:choice>"#
        );
        let schema = d
            .import(
                "",
                &format!(r#"<xs:sequence minOccurs="0">{content}</xs:sequence>"#),
            )
            .unwrap();
        assert_eq!(schema.xml_repeating_choices.len(), 1);
        let choice = &schema.xml_repeating_choices[0];
        assert!(!choice.required);
        assert_eq!(choice.repeating, max == "unbounded");
        assert_eq!(choice.members, ["Left", "Right"]);
        d.roundtrip(&schema);
        assert_eq!(
            schema,
            d.import("", &format!("<xs:sequence>{content}</xs:sequence>"))
                .unwrap()
        );
    }
}
#[test]
fn epsilon_branches_in_an_unbounded_choice_retain_exact_ordered_nonempty_members() {
    for minimum in ["0", "1"] {
        let d = Dir::new();
        let content = format!(
            r#"{OPTIONAL}<xs:choice minOccurs="{minimum}" maxOccurs="unbounded"><xs:element name="Left" type="xs:string" minOccurs="0"/><xs:element name="Right" type="xs:string"/></xs:choice>"#
        );
        let schema = d
            .import(
                "",
                &format!(r#"<xs:sequence minOccurs="0">{content}</xs:sequence>"#),
            )
            .unwrap();
        assert_eq!(schema.xml_repeating_choices.len(), 1);
        assert!(!schema.xml_repeating_choices[0].required);
        assert!(schema.xml_repeating_choices[0].repeating);
        d.roundtrip(&schema);
        let xml = "<Root><Right>b</Right><Left>a</Left><Right>c</Right></Root>";
        let input = format_xml::from_str(xml, &schema).unwrap();
        let output = format_xml::to_string(&schema, &input).unwrap();
        let doc = roxmltree::Document::parse(&output).unwrap();
        let names = doc
            .root_element()
            .children()
            .filter(|node| node.is_element())
            .map(|node| node.tag_name().name())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Right", "Left", "Right"]);
        assert_eq!(format_xml::from_str(&output, &schema).unwrap(), input);
        let empty = format_xml::from_str("<Root/>", &schema).unwrap();
        assert!(
            format_xml::to_string(&schema, &empty)
                .unwrap()
                .contains("Root")
        );
    }
}
#[test]
fn nested_required_sequences_with_nullable_contents_keep_all_occurrences() {
    let d = Dir::new();
    let schema=d.import("",&format!(r#"<xs:sequence minOccurs="0"><xs:sequence>{OPTIONAL}<xs:element name="After" type="xs:string" minOccurs="0"/></xs:sequence></xs:sequence>"#)).unwrap();
    let ir::SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("group expected")
    };
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|child| child.xml_optional));
    d.roundtrip(&schema);
}
#[test]
fn correlated_required_members_and_unordered_compositors_remain_rejected() {
    for content in [
        BRANCHES.to_string(),
        format!(r#"{OPTIONAL}<xs:element name="After" type="xs:string"/>"#),
        format!(r#"{OPTIONAL}<xs:choice>{BRANCHES}</xs:choice>"#),
        format!(
            r#"{OPTIONAL}<xs:sequence><xs:element name="A" type="xs:string"/><xs:element name="B" type="xs:string"/></xs:sequence>"#
        ),
    ] {
        let d = Dir::new();
        assert!(matches!(
            d.import(
                "",
                &format!(r#"<xs:sequence minOccurs="0">{content}</xs:sequence>"#)
            ),
            Err(XmlFormatError::UnsupportedOptionalSequence { .. })
        ));
    }
    let d = Dir::new();
    assert!(matches!(d.import("",&format!(r#"<xs:all minOccurs="0">{OPTIONAL}<xs:element name="After" type="xs:string" minOccurs="0"/></xs:all>"#)),Err(XmlFormatError::UnsupportedOptionalCompositor {..})));
}
#[test]
fn optional_branch_normalization_does_not_admit_bounded_multi_choice_or_singular_roles() {
    for max in ["1", "2"] {
        let d = Dir::new();
        let choice = format!(
            r#"<xs:choice minOccurs="0" maxOccurs="{max}"><xs:element name="Left" type="xs:string" minOccurs="0"/><xs:element name="Right" type="xs:string"/></xs:choice>"#
        );
        assert!(matches!(
            d.import(
                "",
                &format!(r#"<xs:sequence minOccurs="0">{OPTIONAL}{choice}</xs:sequence>"#)
            ),
            Err(XmlFormatError::UnsupportedOptionalSequence { .. }
                | XmlFormatError::UnsupportedOptionalCompositor { .. })
        ));
    }
}

#[test]
fn only_a_nullable_repeating_element_can_remove_the_outer_sequence_requirement() {
    let d = Dir::new();
    let content = format!(
        r#"{OPTIONAL}<xs:element name="After" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>"#
    );
    let schema = d
        .import(
            "",
            &format!(r#"<xs:sequence minOccurs="0">{content}</xs:sequence>"#),
        )
        .unwrap();
    assert!(schema.child("After").unwrap().repeating);
    d.roundtrip(&schema);
    let required = format!(
        r#"{OPTIONAL}<xs:element name="After" type="xs:string" minOccurs="1" maxOccurs="unbounded"/>"#
    );
    assert!(matches!(
        d.import(
            "",
            &format!(r#"<xs:sequence minOccurs="0">{required}</xs:sequence>"#)
        ),
        Err(XmlFormatError::UnsupportedOptionalSequence { .. })
    ));
    let nested = format!(
        r#"{OPTIONAL}<xs:sequence maxOccurs="unbounded"><xs:element name="A" type="xs:string" minOccurs="0"/><xs:element name="B" type="xs:string" minOccurs="0"/></xs:sequence>"#
    );
    assert!(matches!(
        d.import(
            "",
            &format!(r#"<xs:sequence minOccurs="0">{nested}</xs:sequence>"#)
        ),
        Err(XmlFormatError::UnsupportedOptionalSequence { .. })
    ));
}

#[test]
fn nullable_bounded_repetition_does_not_claim_the_unbounded_projection() {
    for particle in [
        format!(
            r#"{OPTIONAL}<xs:element name="After" type="xs:string" minOccurs="0" maxOccurs="2"/>"#
        ),
        format!(r#"{OPTIONAL}<xs:choice minOccurs="0" maxOccurs="2">{BRANCHES}</xs:choice>"#),
    ] {
        let d = Dir::new();
        assert!(matches!(
            d.import(
                "",
                &format!(r#"<xs:sequence minOccurs="0">{particle}</xs:sequence>"#)
            ),
            Err(XmlFormatError::UnsupportedOptionalSequence { .. })
        ));
    }
}
