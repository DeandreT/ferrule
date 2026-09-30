use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_preview_filter_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_design(dir: &Path, optional: bool) -> PathBuf {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("source.xml"),
        "<Source><Row><Name>Ada</Name></Row><Row><Name>Grace</Name></Row></Source>",
    )
    .unwrap();
    let optional_attr = if optional { " optional=\"1\"" } else { "" };
    let design = dir.join("preview-filter.mfd");
    std::fs::write(
        &design,
        format!(
            r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Name" outkey="11"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Source"/></data></component>
  <component name="KeepRows" library="core" kind="6"><sources><datapoint pos="0" key="19"/></sources><targets><datapoint pos="0" key="20"/></targets><data><input datatype="boolean" previewvalue="true" usepreviewvalue="1"/><parameter usageKind="input" name="KeepRows"{optional_attr}/></data></component>
  <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources><targets><datapoint pos="0" key="32"/><datapoint/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="40"><entry name="Name" inpkey="41"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
        ),
    )
    .unwrap();
    design
}

fn input(project: &mapping::Project, dir: &Path) -> Instance {
    format_xml::read(&dir.join("source.xml"), &project.source).unwrap()
}

fn row_count(output: &Instance) -> usize {
    output
        .as_repeated()
        .or_else(|| output.field("Row").and_then(Instance::as_repeated))
        .map_or(0, <[Instance]>::len)
}

#[test]
fn optional_preview_only_filter_skips_iteration_instead_of_unfiltering() {
    let dir = TempDir::new();
    let design = write_design(&dir.0, true);
    let imported = mfd::import(&design).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("optional input parameter `KeepRows`")
                && warning.contains("dependent value skipped")
        }),
        "{:?}",
        imported.warnings
    );
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("filter feeding") && warning.contains("iteration skipped")
        }),
        "{:?}",
        imported.warnings
    );
    assert!(engine::validate(&imported.project).is_empty());
    let preview =
        engine::ExecutionContext::new(&design).with_purpose(engine::ExecutionPurpose::Preview);
    let output = engine::run_with_context(
        &imported.project,
        &input(&imported.project, &dir.0),
        &preview,
    )
    .unwrap();
    assert!(
        matches!(&output, Instance::Group(fields) if fields.is_empty())
            || matches!(&output, Instance::Repeated(rows) if rows.is_empty()),
        "unsupported preview filter must not pass all source rows: {output:?}"
    );
}

#[test]
fn disconnected_filter_predicate_skips_iteration_instead_of_unfiltering() {
    let dir = TempDir::new();
    let design = write_design(&dir.0, false);
    let text = std::fs::read_to_string(&design).unwrap().replace(
        "<vertex vertexkey=\"20\"><edges><edge vertexkey=\"31\"/></edges></vertex>",
        "<vertex vertexkey=\"20\"><edges/></vertex>",
    );
    std::fs::write(&design, text).unwrap();
    let imported = mfd::import(&design).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("filter feeding")
                && warning.contains("missing or unsupported predicate")
                && warning.contains("iteration skipped")
        }),
        "{:?}",
        imported.warnings
    );
    let output = engine::run(&imported.project, &input(&imported.project, &dir.0)).unwrap();
    assert!(
        matches!(&output, Instance::Group(fields) if fields.is_empty())
            || matches!(&output, Instance::Repeated(rows) if rows.is_empty()),
        "a disconnected filter must not pass all source rows: {output:?}"
    );
}

#[test]
fn required_preview_filter_uses_preview_only_for_preview_purpose() {
    let dir = TempDir::new();
    let design = write_design(&dir.0, false);
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = input(&imported.project, &dir.0);
    assert!(matches!(
        engine::run(&imported.project, &source),
        Err(engine::EngineError::MissingRuntimeParameter { name, .. }) if name == "KeepRows"
    ));
    let preview =
        engine::ExecutionContext::new(&design).with_purpose(engine::ExecutionPurpose::Preview);
    let output = engine::run_with_context(&imported.project, &source, &preview).unwrap();
    assert_eq!(row_count(&output), 2, "{output:?}");
    let mut hosts = engine::RuntimeParameters::new();
    hosts.insert("KeepRows", Value::Bool(false)).unwrap();
    let overridden = preview.with_parameters(&hosts);
    let output = engine::run_with_context(&imported.project, &source, &overridden).unwrap();
    assert_eq!(row_count(&output), 0, "{output:?}");
}
