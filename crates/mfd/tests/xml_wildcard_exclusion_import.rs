//! `.mfd` import keeps treating an XSD whose wildcards use XSD 1.1 exclusions
//! like any other unsupported wildcard: the component falls back to its
//! entry tree, although the native XSD importer accepts the schema.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-wildcard-exclusion-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema(wildcards: &str, attribute_wildcard: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Amount" type="xs:long"/><xs:element name="Label" type="xs:string"/>{wildcards}</xs:sequence>{attribute_wildcard}</xs:complexType></xs:element></xs:schema>"#
    )
}

/// One unqualified source copied into one target.
fn design(source_schema: &str) -> String {
    format!(
        r#"<mapping version="22"><component name="defaultmap" uid="1"><properties SelectedLanguage="builtin"/><structure><children>
<component name="Source" library="xml" uid="2" kind="14"><data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1"><entry name="document" ns="1"><entry name="Envelope"><entry name="Amount" outkey="1"/><entry name="Label" outkey="2"/></entry></entry></entry></root><document schema="{source_schema}" instanceroot="{{}}Envelope" inputinstance="input.xml"/></data></component>
<component name="Target" library="xml" uid="3" kind="14"><data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1"><entry name="document" ns="1"><entry name="Envelope"><entry name="Amount" inpkey="3"/><entry name="Label" inpkey="4"/></entry></entry></entry></root><document schema="target.xsd" instanceroot="{{}}Envelope" outputinstance="output.xml"/></data></component>
</children><graph directed="1"><edges/><vertices><vertex vertexkey="1"><edges><edge vertexkey="3"/></edges></vertex><vertex vertexkey="2"><edges><edge vertexkey="4"/></edges></vertex></vertices></graph></structure></component></mapping>"#
    )
}

#[test]
fn exclusion_bearing_component_schemas_fall_back_to_the_entry_tree() {
    let directory = Directory::new();
    directory.write("target.xsd", &schema("", ""));
    // Without the schema file, import falls back to the same entry tree.
    let missing = mfd::import(&directory.write("missing.mfd", &design("missing.xsd"))).unwrap();

    for (label, wildcards, attribute_wildcard, refusal) in [
        (
            "element",
            r###"<xs:any notNamespace="##local" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>"###,
            "",
            "xs:any wildcard cannot be represented",
        ),
        (
            "attribute",
            "",
            r#"<xs:anyAttribute xmlns:x="urn:x" notQName="x:blocked" processContents="skip"/>"#,
            "xs:anyAttribute wildcard cannot be represented",
        ),
    ] {
        let source = directory.write(
            &format!("{label}.xsd"),
            &schema(wildcards, attribute_wildcard),
        );
        // The native importer accepts the schema and keeps the exclusions.
        let native = format_xml::xsd::import_root(&source, Some("Envelope")).unwrap();
        assert!(native.uses_xml_wildcard_exclusions(), "{label}");

        let imported = mfd::import(
            &directory.write(&format!("{label}.mfd"), &design(&format!("{label}.xsd"))),
        )
        .unwrap();
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains(&format!("could not read schema `{label}.xsd`"))
                    && warning.contains(refusal)
                    && warning.contains("XSD 1.1 wildcard exclusions are not supported")
                    && warning.contains("falling back to the entry tree")
            }),
            "{label}: {:?}",
            imported.warnings
        );
        assert_eq!(imported.project.source, missing.project.source, "{label}");
        assert!(!imported.project.source.uses_xml_wildcard_exclusions());
    }
}
