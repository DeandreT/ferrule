use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{CsvTextRepairCause as Cause, NamedSource, NamedTarget};
use mfd::{ExportProfile, ImportIssueKind, ImportOptions, ImportProfile, MfdError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-encoding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn component(name: &str, source: bool, encoding: Option<&str>, extra_attrs: &str) -> String {
    let port = if source { "outkey" } else { "inpkey" };
    let base = if source { 10 } else { 20 };
    let properties = if source {
        ""
    } else {
        "<properties XSLTDefaultOutput=\"1\"/>"
    };
    let encoding = encoding
        .map(|value| format!(" encoding=\"{value}\""))
        .unwrap_or_default();
    format!(
        r#"<component name="{name}" library="text" kind="16">{properties}<data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" {port}="{base}">
<entry name="Text" {port}="{}"/><entry name="Count" {port}="{}"/>
</entry></entry></entry></root><text type="csv"{encoding}{extra_attrs}>
<settings separator="," firstrownames="false"><names root="{name}" block="Rows">
<field0 name="Text" type="string"/><field1 name="Count" type="integer"/>
</names></settings></text></data></component>"#,
        base + 1,
        base + 2,
    )
}

fn design_with_text_attrs(
    source_encoding: Option<&str>,
    target_encoding: Option<&str>,
    source_attrs: &str,
    target_attrs: &str,
) -> String {
    let source = component("source", true, source_encoding, source_attrs);
    let target = component("target", false, target_encoding, target_attrs);
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>{source}{target}
</children><graph><vertices>
<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
<vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
    )
}

#[test]
fn unsupported_text_settings_persist_and_never_requalify_after_warning_clear()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    for (case, source_encoding, attrs, cause) in [
        ("encoding", Some("52"), "", Cause::Encoding),
        (
            "byteorder",
            Some("1000"),
            " byteorder=\"2\"",
            Cause::ByteOrder,
        ),
        (
            "bom",
            Some("1000"),
            " byteordermark=\"2\"",
            Cause::ByteOrderMark,
        ),
        ("malformed", Some("not-an-encoding"), "", Cause::Encoding),
    ] {
        for source in [true, false] {
            let path = temp.0.join(format!("{case}-{source}.mfd"));
            let (source_encoding, target_encoding, source_attrs, target_attrs) = if source {
                (source_encoding, Some("1000"), attrs, "")
            } else {
                (Some("1000"), source_encoding, "", attrs)
            };
            std::fs::write(
                &path,
                design_with_text_attrs(
                    source_encoding,
                    target_encoding,
                    source_attrs,
                    target_attrs,
                ),
            )?;
            let imported = mfd::import(&path)?;
            assert!(engine::validate(&imported.project).is_empty());
            let host = Instance::Repeated(vec![Instance::Group(vec![
                (
                    "Text".into(),
                    Instance::Scalar(Value::String("host-parsed".into())),
                ),
                ("Count".into(), Instance::Scalar(Value::Int(7))),
            ])]);
            assert!(engine::run(&imported.project, &host).is_ok());
            assert!(
                matches!(mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable),
                Err(MfdError::IncompatibleImport(report)) if report.issues.iter().any(|issue| issue.kind == ImportIssueKind::RuntimeDependency))
            );
            let reopened = mapping::project_file::decode_str(
                &mapping::project_file::encode_pretty(&imported.project)?,
            )?;
            let clean = mfd::Imported {
                project: reopened,
                warnings: Vec::new(),
                mapping_path: path,
            };
            let report = mfd::assess_import(&clean);
            assert!(!report.executable);
            assert_eq!(report.issues.len(), 1);
            assert_eq!(report.issues[0].kind, ImportIssueKind::RuntimeDependency);
            let dependencies = clean.project.csv_runtime_dependencies();
            assert_eq!(dependencies.len(), 1);
            assert_eq!(
                dependencies[0].dependency.causes().collect::<Vec<_>>(),
                vec![cause]
            );
            for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
                let output = temp
                    .0
                    .join(format!("absent-{case}-{source}-{profile:?}/mapping.mfd"));
                assert!(mfd::export_with_profile(&clean.project, &output, profile).is_err());
                assert!(!output.parent().unwrap().exists());
                let existing = temp
                    .0
                    .join(format!("sentinel-{case}-{source}-{profile:?}.mfd"));
                std::fs::write(&existing, b"sentinel")?;
                assert!(mfd::export_with_profile(&clean.project, &existing, profile).is_err());
                assert_eq!(std::fs::read(existing)?, b"sentinel");
            }
        }
    }
    Ok(())
}

#[test]
fn named_boundary_repairs_survive_reload_and_block_both_export_profiles()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("mapping.mfd");
    std::fs::write(
        &path,
        design_with_text_attrs(Some("1000"), Some("1000"), "", ""),
    )?;
    let mut imported = mfd::import(&path)?;
    let mut options = imported.project.source_options.clone();
    options.csv_text_repair_dependency =
        Some(mapping::CsvTextRepairDependency::new(Cause::ByteOrder));
    imported.project.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "missing.csv".into(),
        schema: imported.project.source.clone(),
        options: options.clone(),
        dynamic_path: None,
    });
    imported.project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: Some("audit.csv".into()),
        schema: imported.project.target.clone(),
        options,
        root: imported.project.root.clone(),
    });
    imported.project = mapping::project_file::decode_str(&mapping::project_file::encode_pretty(
        &imported.project,
    )?)?;
    imported.warnings.clear();
    let report = mfd::assess_import(&imported);
    assert!(!report.executable);
    assert_eq!(
        report
            .issues
            .iter()
            .filter(|issue| issue.kind == ImportIssueKind::RuntimeDependency)
            .count(),
        2
    );
    for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
        let output = temp
            .0
            .join(format!("named-blocked-{profile:?}/mapping.mfd"));
        assert!(mfd::export_with_profile(&imported.project, &output, profile).is_err());
        assert!(!output.parent().unwrap().exists());
    }
    Ok(())
}

#[test]
fn independent_unsupported_text_settings_keep_all_three_causes() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("all-causes.mfd");
    std::fs::write(
        &path,
        design_with_text_attrs(
            Some("52"),
            Some("1000"),
            " byteorder=\"2\" byteordermark=\"unknown\"",
            "",
        ),
    )?;
    let imported = mfd::import(&path)?;
    assert_eq!(imported.warnings.len(), 3);
    let dependency = imported
        .project
        .source_options
        .csv_text_repair_dependency
        .ok_or("missing CSV repair marker")?;
    assert_eq!(
        dependency.causes().collect::<Vec<_>>(),
        vec![Cause::Encoding, Cause::ByteOrder, Cause::ByteOrderMark]
    );
    let saved = mapping::project_file::encode_pretty(&imported.project)?;
    assert!(!saved.contains("unknown"));
    let reopened = mapping::project_file::decode_str(&saved)?;
    assert_eq!(
        reopened.source_options.csv_text_repair_dependency,
        Some(dependency)
    );
    Ok(())
}

fn supported_xml_to_csv_pipeline(directory: &Path) -> Result<mapping::Pipeline, Box<dyn Error>> {
    for (file, root, field) in [
        ("source.xsd", "Source", "Start"),
        ("buffer.xsd", "Buffer", "Value"),
    ] {
        std::fs::write(
            directory.join(file),
            format!(
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{field}\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
            ),
        )?;
    }
    let design = directory.join("supported-pipeline.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
<component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Buffer"><entry name="Value" inpkey="20" outkey="30"/></entry></root><document schema="buffer.xsd" instanceroot="{}Buffer"/></data></component>
<component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows"><entry name="Result" inpkey="40"/></entry></entry></entry></root><text type="csv" outputinstance="target.csv"><settings separator=";" quote="&quot;" firstrownames="false"><names root="Target" block="Rows"><field0 name="Result" type="string"/></names></settings></text></data></component>
</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;
    let imported = mfd::import_pipeline(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate_pipeline(&imported.pipeline).is_empty());
    assert_eq!(imported.pipeline.stages.len(), 2);
    Ok(imported.pipeline)
}

fn sentinel_snapshot(design: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, Box<dyn Error>> {
    let parent = design.parent().ok_or("design has no parent")?;
    let siblings = std::fs::read_dir(parent)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "xsd"))
        .collect::<Vec<_>>();
    assert!(
        !siblings.is_empty(),
        "control export produced no schema sibling"
    );
    std::fs::write(design, b"existing mapping sentinel")?;
    for (index, sibling) in siblings.iter().enumerate() {
        std::fs::write(sibling, format!("existing schema sentinel {index}"))?;
    }
    let mut snapshot = BTreeMap::new();
    for entry in std::fs::read_dir(parent)? {
        let path = entry?.path();
        if path.is_file() {
            snapshot.insert(path.clone(), std::fs::read(path)?);
        }
    }
    Ok(snapshot)
}

fn assert_snapshot_unchanged(
    design: &Path,
    before: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), Box<dyn Error>> {
    let parent = design.parent().ok_or("design has no parent")?;
    let mut after = BTreeMap::new();
    for entry in std::fs::read_dir(parent)? {
        let path = entry?.path();
        if path.is_file() {
            after.insert(path.clone(), std::fs::read(path)?);
        }
    }
    assert_eq!(
        &after, before,
        "rejected export changed published artifacts"
    );
    Ok(())
}

fn assert_repair_rejection(result: Result<mfd::ExportReport, MfdError>) {
    match result {
        Err(MfdError::Unsupported(message)) => {
            assert!(
                message.contains("unsupported native CSV text settings"),
                "{message}"
            );
        }
        Err(error) => panic!("expected a CSV repair rejection, got {error}"),
        Ok(_) => panic!("CSV repair export unexpectedly succeeded"),
    }
}

#[test]
fn csv_repair_rejects_single_and_pipeline_exports_before_existing_artifacts_change()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let pipeline = supported_xml_to_csv_pipeline(&temp.0)?;
    let last = pipeline.stages.last().ok_or("missing CSV final stage")?;
    assert_eq!(last.project.target_path.as_deref(), Some("target.csv"));
    assert!(
        last.project
            .target_options
            .csv_text_repair_dependency
            .is_none()
    );
    for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
        let single_dir = temp.0.join(format!("single-{profile:?}"));
        std::fs::create_dir_all(&single_dir)?;
        let single_design = single_dir.join("mapping.mfd");
        mfd::export_with_profile(&last.project, &single_design, profile)?;
        let single_before = sentinel_snapshot(&single_design)?;
        let mut marked_project = last.project.clone();
        marked_project.target_options.csv_text_repair_dependency =
            Some(mapping::CsvTextRepairDependency::new(Cause::Encoding));
        assert_repair_rejection(mfd::preflight_export(&marked_project, &single_design));
        assert_repair_rejection(mfd::export_with_profile(
            &marked_project,
            &single_design,
            profile,
        ));
        assert_snapshot_unchanged(&single_design, &single_before)?;

        let pipeline_dir = temp.0.join(format!("pipeline-{profile:?}"));
        std::fs::create_dir_all(&pipeline_dir)?;
        let pipeline_design = pipeline_dir.join("pipeline.mfd");
        mfd::export_pipeline_with_profile(&pipeline, &pipeline_design, profile)?;
        let pipeline_before = sentinel_snapshot(&pipeline_design)?;
        let mut marked_pipeline = pipeline.clone();
        marked_pipeline
            .stages
            .last_mut()
            .ok_or("missing CSV final stage")?
            .project
            .target_options
            .csv_text_repair_dependency =
            Some(mapping::CsvTextRepairDependency::new(Cause::Encoding));
        assert_repair_rejection(mfd::preflight_pipeline_export(
            &marked_pipeline,
            &pipeline_design,
        ));
        assert_repair_rejection(mfd::export_pipeline_with_profile(
            &marked_pipeline,
            &pipeline_design,
            profile,
        ));
        assert_snapshot_unchanged(&pipeline_design, &pipeline_before)?;
    }
    Ok(())
}
