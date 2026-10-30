//! Imported XML targets write the reference default schema-location hint.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const NAMESPACE: &str = "urn:ferrule:schema-location";

struct Dir(PathBuf);
impl Dir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-schema-location-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema(namespace: Option<&str>) -> String {
    let target = namespace
        .map(|uri| format!(" targetNamespace=\"{uri}\" elementFormDefault=\"qualified\""))
        .unwrap_or_default();
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"{target}><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Amount" type="xs:long"/><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#
    )
}

/// One unqualified source copied into an unqualified or namespaced target.
fn design(directory: &Path, setting: Option<&str>, namespace: Option<&str>, schema_file: &str) {
    std::fs::write(directory.join("source.xsd"), schema(None)).unwrap();
    let schema_path = directory.join(schema_file.replace('\\', "/"));
    std::fs::create_dir_all(schema_path.parent().unwrap()).unwrap();
    std::fs::write(schema_path, schema(namespace)).unwrap();
    let properties = match setting {
        Some(value) => {
            format!(r#"<properties XSLTDefaultOutput="1" AddSchemaLocation="{value}"/>"#)
        }
        None => r#"<properties XSLTDefaultOutput="1"/>"#.into(),
    };
    let (slots, entry_slot, root) = match namespace {
        Some(uri) => (
            format!(
                r#"<namespace/><namespace uid="{uri}"/><namespace uid="http://www.altova.com/mapforce"/>"#
            ),
            (" ns=\"1\"", "2"),
            format!("{{{uri}}}Envelope"),
        ),
        None => (
            r#"<namespace/><namespace uid="http://www.altova.com/mapforce"/>"#.into(),
            ("", "1"),
            "{}Envelope".into(),
        ),
    };
    let (ns, wrapper) = entry_slot;
    std::fs::write(
        directory.join("mapping.mfd"),
        format!(
            r#"<mapping version="22"><component name="defaultmap" uid="1"><properties SelectedLanguage="builtin"/><structure><children>
<component name="Source" library="xml" uid="2" kind="14"><data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1"><entry name="document" ns="1"><entry name="Envelope"><entry name="Amount" outkey="1"/><entry name="Label" outkey="2"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{{}}Envelope" inputinstance="input.xml"/></data></component>
<component name="Target" library="xml" uid="3" kind="14">{properties}<data><root><header><namespaces>{slots}</namespaces></header><entry name="FileInstance" ns="{wrapper}"><entry name="document" ns="{wrapper}"><entry name="Envelope"{ns}><entry name="Amount"{ns} inpkey="3"/><entry name="Label"{ns} inpkey="4"/></entry></entry></entry></root><document schema="{schema_file}" instanceroot="{root}" outputinstance="output.xml"/></data></component>
</children><graph directed="1"><edges/><vertices><vertex vertexkey="1"><edges><edge vertexkey="3"/></edges></vertex><vertex vertexkey="2"><edges><edge vertexkey="4"/></edges></vertex></vertices></graph></structure></component></mapping>"#
        ),
    )
    .unwrap();
    std::fs::write(
        directory.join("input.xml"),
        "<Envelope><Amount>-9223372036854775808</Amount><Label>kept &amp; complete</Label></Envelope>",
    )
    .unwrap();
}

/// Imports executably, runs, and returns the output root's schema attributes.
fn run(directory: &Path) -> (Option<String>, Option<String>, Vec<(String, String)>) {
    let project = directory.join("project.json");
    let warnings = cli::import_mfd_with_profile(
        &directory.join("mapping.mfd"),
        &project,
        None,
        None,
        &[],
        &[],
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let output = directory.join("output.xml");
    cli::run_project(&project, &directory.join("input.xml"), &output).unwrap();
    let text = std::fs::read_to_string(&output).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let root = document.root_element();
    let children = root
        .children()
        .filter(|node| node.is_element())
        .map(|node| {
            (
                node.tag_name().name().to_owned(),
                node.text().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    (
        root.attribute((XSI, "noNamespaceSchemaLocation"))
            .map(str::to_owned),
        root.attribute((XSI, "schemaLocation")).map(str::to_owned),
        children,
    )
}

fn values() -> Vec<(String, String)> {
    vec![
        ("Amount".into(), "-9223372036854775808".into()),
        ("Label".into(), "kept & complete".into()),
    ]
}

#[test]
fn absent_and_enabled_settings_write_the_stored_schema_reference() {
    for setting in [None, Some("1")] {
        let directory = Dir::new("unqualified");
        design(&directory.0, setting, None, "target.xsd");
        assert_eq!(
            run(&directory.0),
            (Some("target.xsd".into()), None, values()),
            "{setting:?}"
        );
    }
}

#[test]
fn disabled_setting_writes_no_hint() {
    let directory = Dir::new("disabled");
    design(&directory.0, Some("0"), None, "target.xsd");
    assert_eq!(run(&directory.0), (None, None, values()));
}

#[test]
fn namespaced_roots_write_one_namespace_location_pair() {
    for setting in [None, Some("1")] {
        let directory = Dir::new("namespaced");
        design(&directory.0, setting, Some(NAMESPACE), "target.xsd");
        assert_eq!(
            run(&directory.0),
            (None, Some(format!("{NAMESPACE} target.xsd")), values()),
            "{setting:?}"
        );
    }
    let directory = Dir::new("namespaced-disabled");
    design(&directory.0, Some("0"), Some(NAMESPACE), "target.xsd");
    assert_eq!(run(&directory.0), (None, None, values()));
}

#[test]
fn stored_windows_paths_become_portable_uri_references() {
    let directory = Dir::new("windows-path");
    design(&directory.0, None, None, r"schemas\Target Schema.xsd");
    assert_eq!(
        run(&directory.0).0.as_deref(),
        Some("schemas/Target%20Schema.xsd")
    );
}

#[test]
fn non_xml_sources_feed_a_hinted_xml_target() {
    let directory = Dir::new("csv");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/present-string-group-keys");
    for name in ["legacy-csv-sparse-string.mfd", "legacy-csv-target.xsd"] {
        std::fs::copy(fixtures.join(name), directory.0.join(name)).unwrap();
    }
    let project = directory.0.join("project.json");
    let warnings = cli::import_mfd_with_profile(
        &directory.0.join("legacy-csv-sparse-string.mfd"),
        &project,
        None,
        None,
        &[],
        &[],
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    std::fs::write(directory.0.join("input.csv"), "Key\nsapling\n").unwrap();
    let output = directory.0.join("output.xml");
    cli::run_project(&project, &directory.0.join("input.csv"), &output).unwrap();
    let text = std::fs::read_to_string(&output).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    assert_eq!(
        document
            .root_element()
            .attribute((XSI, "noNamespaceSchemaLocation")),
        Some("legacy-csv-target.xsd"),
        "{text}"
    );
}
