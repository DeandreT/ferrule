//! A target's input instance is a design-time preview: a wildcard there never
//! makes the target a file set, and the output path never names it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-xml-target-preview-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("source.xsd"), SCHEMA).unwrap();
        std::fs::write(path.join("target.xsd"), SCHEMA).unwrap();
        Self(path)
    }
    fn design(&self, name: &str, target_document: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(
            &path,
            format!(
                r#"<mapping version="22"><component name="defaultmap" uid="1"><properties SelectedLanguage="builtin"/><structure><children>
<component name="Source" library="xml" uid="2" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Label" outkey="1"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{{}}Envelope" inputinstance="input.xml"/></data></component>
<component name="Target" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1" AddSchemaLocation="0"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Label" inpkey="2"/></entry></entry></entry></root>{target_document}</data></component>
</children><graph directed="1"><edges/><vertices><vertex vertexkey="1"><edges><edge vertexkey="2"/></edges></vertex></vertices></graph></structure></component></mapping>"#
            ),
        )
        .unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SCHEMA: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;

fn document(input: Option<&str>, output: Option<&str>) -> String {
    let attribute = |name: &str, value: Option<&str>| {
        value
            .map(|value| format!(r#" {name}="{value}""#))
            .unwrap_or_default()
    };
    format!(
        r#"<document schema="target.xsd" instanceroot="{{}}Envelope"{}{}/>"#,
        attribute("inputinstance", input),
        attribute("outputinstance", output)
    )
}

fn executable(path: &Path) -> mapping::Project {
    let outcome = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(
        outcome.imported.warnings.is_empty(),
        "{:?}",
        outcome.imported.warnings
    );
    outcome.imported.project
}

fn label(project: &mapping::Project) -> Option<Value> {
    let source =
        format_xml::from_str("<Envelope><Label>x</Label></Envelope>", &project.source).unwrap();
    engine::run(project, &source)
        .unwrap()
        .field("Label")
        .and_then(Instance::as_scalar)
        .cloned()
}

#[test]
fn wildcard_preview_input_imports_as_a_runnable_target() {
    let directory = Directory::new();
    let project = executable(&directory.design(
        "mapping.mfd",
        &document(Some("prior*.xml"), Some("output.xml")),
    ));
    assert_eq!(
        project.target_options,
        mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        }
    );
    assert_eq!(project.target_path.as_deref(), Some("output.xml"));
    assert_eq!(label(&project), Some(Value::String("x".into())));
}

#[test]
fn a_target_preview_input_does_not_change_the_imported_project() {
    let directory = Directory::new();
    let without = executable(&directory.design("none.mfd", &document(None, Some("output.xml"))));
    for preview in ["prior.xml", "prior*.xml", "prior?.xml"] {
        let project = executable(
            &directory.design("preview.mfd", &document(Some(preview), Some("output.xml"))),
        );
        assert_eq!(
            serde_json::to_value(&project).unwrap(),
            serde_json::to_value(&without).unwrap(),
            "{preview}"
        );
    }
}

#[test]
fn only_a_single_document_preview_names_an_absent_output() {
    let directory = Directory::new();
    let plain = mfd::import(&directory.design("plain.mfd", &document(Some("prior.xml"), None)))
        .unwrap()
        .project;
    assert_eq!(plain.target_path.as_deref(), Some("prior.xml"));
    let wildcard = mfd::import(&directory.design("set.mfd", &document(Some("prior*.xml"), None)))
        .unwrap()
        .project;
    assert_eq!(wildcard.target_path, None);
    assert!(!wildcard.target_options.local_xml_file_set);
    assert!(engine::validate(&wildcard).is_empty());
}

#[test]
fn a_target_preview_input_exports_and_reimports_like_a_plain_preview() {
    let directory = Directory::new();
    let mut exported = Vec::new();
    for (name, preview) in [("plain", "prior.xml"), ("set", "prior*.xml")] {
        let project = executable(&directory.design(
            &format!("{name}.mfd"),
            &document(Some(preview), Some("output.xml")),
        ));
        let out = directory.0.join(format!("export-{name}/roundtrip.mfd"));
        let report =
            mfd::export_with_profile(&project, &out, mfd::ExportProfile::NativeMfd).unwrap();
        assert!(report.is_native_compatible(), "{report}");
        let reimported = executable(&out);
        assert_eq!(reimported.target_options, project.target_options);
        assert_eq!(reimported.target_path.as_deref(), Some("output.xml"));
        assert_eq!(label(&reimported), Some(Value::String("x".into())));
        exported.push(std::fs::read_to_string(&out).unwrap());
    }
    assert_eq!(exported[0], exported[1]);
}

/// A pass-through component reads its input instance as the next stage's
/// source, so a wildcard there stays a file set in that role only.
#[test]
fn pass_through_components_keep_a_wildcard_input_in_their_source_role() {
    let directory = Directory::new();
    std::fs::write(
        directory.0.join("buffer.xsd"),
        SCHEMA.replace("\"Envelope\"", "\"Buffer\""),
    )
    .unwrap();
    let path = directory.0.join("mapping.mfd");
    std::fs::write(
        &path,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Envelope"><entry name="Label" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="input.xml" instanceroot="{}Envelope"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1" AddSchemaLocation="0"/><data><root><entry name="Buffer"><entry name="Label" inpkey="20" outkey="30"/></entry></root><document schema="buffer.xsd" inputinstance="buffer*.xml" outputinstance="buffer.xml" instanceroot="{}Buffer"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1" AddSchemaLocation="0"/><data><root><entry name="Envelope"><entry name="Label" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Envelope"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&path).unwrap();
    let [buffer] = imported.project.extra_targets.as_slice() else {
        panic!("expected the pass-through component as an additional target");
    };
    assert!(!buffer.options.local_xml_file_set);
    assert_eq!(buffer.path.as_deref(), Some("buffer.xml"));
    assert!(engine::validate(&imported.project).is_empty());

    let pipeline = mfd::import_pipeline(&path).unwrap();
    let [first, second] = pipeline.pipeline.stages.as_slice() else {
        panic!("expected two stages");
    };
    assert!(!first.project.target_options.local_xml_file_set);
    assert!(second.project.source_options.local_xml_file_set);
    assert_eq!(second.project.source_path.as_deref(), Some("buffer*.xml"));
}
