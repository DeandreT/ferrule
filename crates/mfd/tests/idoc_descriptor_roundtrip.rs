use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mapping::{EdiBoundaryKind, IdocNativeNode, NamedSource, NamedTarget, Project, Scope};
use mfd::{ExportCompatibility, ExportCompatibilityFeature as Feature, ExportProfile};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_idoc_descriptor_{}_{}",
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

fn config() -> &'static str {
    "BEGIN_SEGMENT_SECTION\nBEGIN_IDOC ORDERS01\nBEGIN_GROUP 1\nLEVEL 01\nSTATUS MANDATORY\nLOOPMIN 0000000001\nLOOPMAX 0000000001\nBEGIN_SEGMENT E2ITEM\nSEGMENTTYPE E1ITEM\nQUALIFIED\nLEVEL 02\nSTATUS OPTIONAL\nLOOPMIN 0000000000\nLOOPMAX 9999999999\nBEGIN_FIELDS\nNAME DOCNO\nTEXT Document number\nTYPE CHARACTER\nLENGTH 000005\nFIELD_POS 0001\nBYTE_FIRST 000064\nBYTE_LAST 000068\nVALUE 'A'\nVALUE_TEXT Active\nEND_FIELDS\nEND_SEGMENT\nEND_GROUP\nEND_IDOC\nEND_SEGMENT_SECTION\n"
}

fn write_design(dir: &Path, config_text: &str) -> PathBuf {
    std::fs::write(dir.join("parser.txt"), config_text).unwrap();
    let design = dir.join("original.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="22"><resources/><component name="defaultmap" uid="1">
          <structure><children>
            <component name="idoc" library="text" uid="2" kind="16"><properties/><data>
              <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
                <entry name="SG1"><entry name="E2ITEM"><entry name="DOCNO" outkey="10"/></entry></entry>
              </entry></entry></entry></root>
              <text type="edi" kind="EDIFIXED" config="parser.txt" inputinstance="input.idoc"/>
            </data></component>
            <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data>
              <root><entry name="Outputs"><entry name="Value" inpkey="20"/></entry></root>
            </data></component>
          </children><graph directed="1"><vertices>
            <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
          </vertices></graph></structure>
        </component></mapping>"#,
    )
    .unwrap();
    design
}

fn exported_project(dir: &Path) -> (Project, String) {
    let imported = mfd::import(&write_design(dir, config())).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = imported.project;
    assert_eq!(project.source_options.edi_kind, Some(EdiBoundaryKind::Idoc));
    let descriptor = project.source_options.idoc_native_config.as_ref().unwrap();
    assert_eq!(descriptor.name(), "ORDERS01");
    let IdocNativeNode::Group(group) = &descriptor.nodes()[0] else {
        panic!("expected group");
    };
    let IdocNativeNode::Segment(segment) = &group.children()[0] else {
        panic!("expected segment");
    };
    assert_eq!(segment.record_name(), "E2ITEM");
    assert_eq!(segment.segment_type(), "E1ITEM");
    assert!(segment.qualified());
    assert_eq!(segment.loop_max(), 9_999_999_999);
    assert_eq!(segment.fields()[0].codes()[0].text(), "Active");
    assert_eq!(
        descriptor.project().unwrap(),
        (
            project.source.clone(),
            project.source_options.idoc.clone().unwrap()
        )
    );
    let encoded = serde_json::to_string(&project).unwrap();
    let decoded: Project = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.source_options.idoc_native_config,
        project.source_options.idoc_native_config
    );

    let output = dir.join("ferrule.mfd");
    let warnings = mfd::export(&project, &output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(output).unwrap();
    assert!(text.contains("<ferrule-idoc-native-config kind=\"idoc\" version=\"1\">"));
    assert!(!text.contains("config=\"parser.txt\""));
    (project, text)
}

fn replace_within(xml: &str, tag: &str, before: &str, after: &str) -> String {
    let start = xml.find(&format!("<{tag}")).unwrap();
    let end = start + xml[start..].find(&format!("</{tag}>")).unwrap();
    let section = &xml[start..end];
    assert!(section.contains(before), "missing `{before}` in {tag}");
    format!(
        "{}{}{}",
        &xml[..start],
        section.replacen(before, after, 1),
        &xml[end..]
    )
}

fn idoc_config_references(xml: &str) -> Vec<String> {
    let document = roxmltree::Document::parse(xml).unwrap();
    document
        .descendants()
        .filter(|node| {
            node.has_tag_name("text")
                && node.attribute("type") == Some("edi")
                && node.attribute("kind") == Some("EDIFIXED")
        })
        .map(|node| node.attribute("config").unwrap().to_string())
        .collect()
}

fn without_idoc_config_reference(xml: &str) -> String {
    let references = idoc_config_references(xml);
    assert_eq!(references.len(), 1);
    xml.replacen(&format!(" config=\"{}\"", references[0]), "", 1)
}

fn assert_portable_config_reference(dir: &Path, reference: &str) -> String {
    let relative = Path::new(reference);
    assert!(!relative.is_absolute(), "{reference}");
    assert!(
        relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))),
        "{reference}"
    );
    assert!(reference.ends_with(".txt"), "{reference}");
    assert_ne!(reference, "parser.txt");
    let sibling = dir.join(relative);
    assert!(sibling.is_file(), "{}", sibling.display());
    std::fs::read_to_string(sibling).unwrap()
}

#[test]
fn exported_idoc_config_sibling_survives_relocation_and_keeps_native_blockers() {
    let dir = TempDir::new();
    let (project, _) = exported_project(&dir.0);
    let original_config = dir.0.join("parser.txt");
    let bundle = dir.0.join("exported");
    std::fs::create_dir_all(&bundle).unwrap();
    let output = bundle.join("portable.mfd");
    mfd::export(&project, &output).unwrap();
    let xml = std::fs::read_to_string(&output).unwrap();
    let references = idoc_config_references(&xml);
    assert_eq!(references.len(), 1);
    let rendered = assert_portable_config_reference(&bundle, &references[0]);
    let descriptor = project.source_options.idoc_native_config.as_ref().unwrap();
    assert_eq!(
        format_edi::config::idoc::parse_native_config(&rendered).unwrap(),
        *descriptor
    );
    let compiled = format_edi::config::idoc::import_config(&bundle.join(&references[0])).unwrap();
    assert_eq!(compiled.native.as_ref(), Some(descriptor));
    assert_eq!(compiled.schema, project.source);
    assert_eq!(
        compiled.layout,
        project.source_options.idoc.clone().unwrap()
    );

    let relocated = dir.0.join("relocated");
    std::fs::rename(&bundle, &relocated).unwrap();
    std::fs::remove_file(original_config).unwrap();
    let reimported = mfd::import(&relocated.join("portable.mfd")).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(reimported.project.source_options, project.source_options);
    assert!(engine::validate(&reimported.project).is_empty());
    let report = mfd::preflight_export(&reimported.project, &relocated.join("native.mfd")).unwrap();
    assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
    for feature in [
        Feature::EdiSchema,
        Feature::EdiLayout,
        Feature::EdiConfigDescriptor,
    ] {
        assert!(report.issues.iter().any(|issue| issue.feature == feature));
    }
}

#[test]
fn multiple_idoc_boundaries_receive_distinct_config_siblings() {
    let dir = TempDir::new();
    let (mut project, _) = exported_project(&dir.0);
    project.extra_sources.push(NamedSource {
        name: "secondary".into(),
        path: "secondary.idoc".into(),
        schema: project.source.clone(),
        options: project.source_options.clone(),
        dynamic_path: None,
    });
    let bundle = dir.0.join("multiple");
    let output = bundle.join("portable.mfd");
    mfd::export(&project, &output).unwrap();
    let xml = std::fs::read_to_string(output).unwrap();
    let references = idoc_config_references(&xml);
    assert_eq!(references.len(), 2);
    assert_ne!(references[0], references[1]);
    for reference in references {
        let rendered = assert_portable_config_reference(&bundle, &reference);
        assert_eq!(
            format_edi::config::idoc::parse_native_config(&rendered).unwrap(),
            *project.source_options.idoc_native_config.as_ref().unwrap()
        );
    }
}

#[test]
fn certified_idoc_provenance_survives_project_and_ferrule_mfd_roundtrips() {
    let dir = TempDir::new();
    let (project, _) = exported_project(&dir.0);
    let reimported = mfd::import(&dir.0.join("ferrule.mfd")).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(reimported.project.source_options, project.source_options);

    let strict_path = dir.0.join("native-not-published.mfd");
    let report = mfd::preflight_export(&project, &strict_path).unwrap();
    assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
    for feature in [
        Feature::EdiSchema,
        Feature::EdiLayout,
        Feature::EdiConfigDescriptor,
    ] {
        assert!(
            report.issues.iter().any(|issue| issue.feature == feature),
            "{report:?}"
        );
    }
    assert!(matches!(
        mfd::export_with_profile(&project, &strict_path, ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!strict_path.exists());
    assert!(!std::fs::read_dir(&dir.0).unwrap().any(|entry| {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        name.starts_with("native-not-published-") && name.ends_with(".idoc-config.txt")
    }));
}

#[test]
fn edited_or_conflicting_embedded_metadata_cannot_certify_provenance() {
    let dir = TempDir::new();
    let (project, xml) = exported_project(&dir.0);
    let embedded_only = without_idoc_config_reference(&xml);
    let altered = [
        replace_within(
            &embedded_only,
            "ferrule-idoc-native-config",
            "version=\"1\"",
            "version=\"2\"",
        ),
        replace_within(
            &embedded_only,
            "ferrule-idoc-native-config",
            "kind=\"idoc\"",
            "kind=\"x12\"",
        ),
        replace_within(&embedded_only, "ferrule-layout", "DOCNO", "OTHER"),
        embedded_only.replacen("<entry name=\"DOCNO\"", "<entry name=\"ALTERED\"", 1),
        embedded_only.replacen("kind=\"EDIFIXED\"", "kind=\"EDIX12\"", 1),
    ];
    for (index, modified) in altered.into_iter().enumerate() {
        assert_ne!(modified, xml);
        let path = dir.0.join(format!("altered-{index}.mfd"));
        std::fs::write(&path, modified).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(imported.project.source_options.idoc_native_config.is_none());
        assert!(!imported.warnings.is_empty(), "case {index}");
    }

    let start = embedded_only.find("<ferrule-idoc-native-config").unwrap();
    let end = start
        + embedded_only[start..]
            .find("</ferrule-idoc-native-config>")
            .unwrap()
        + "</ferrule-idoc-native-config>".len();
    let duplicate = format!(
        "{}{}{}",
        &embedded_only[..start],
        &embedded_only[start..end],
        &embedded_only[start..]
    );
    let duplicate_path = dir.0.join("duplicate.mfd");
    std::fs::write(&duplicate_path, duplicate).unwrap();
    let imported = mfd::import(&duplicate_path).unwrap();
    assert!(imported.project.source_options.idoc_native_config.is_none());
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("duplicate IDoc"))
    );

    let layout_start = embedded_only.find("<ferrule-layout").unwrap();
    let layout_end = layout_start
        + embedded_only[layout_start..]
            .find("</ferrule-layout>")
            .unwrap()
        + "</ferrule-layout>".len();
    let without_layout = format!(
        "{}{}",
        &embedded_only[..layout_start],
        &embedded_only[layout_end..]
    );
    let no_layout_path = dir.0.join("missing-layout.mfd");
    std::fs::write(&no_layout_path, without_layout).unwrap();
    let imported = mfd::import(&no_layout_path).unwrap();
    assert!(imported.project.source_options.idoc_native_config.is_none());
    assert!(imported.project.source_options.idoc.is_none());
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("independently typed"))
    );

    let tampered = replace_within(&xml, "ferrule-idoc-native-config", "Active", "Tampered");
    let path = dir.0.join("external-authoritative.mfd");
    std::fs::write(&path, tampered).unwrap();
    let imported = mfd::import(&path).unwrap();
    assert_eq!(
        imported.project.source_options.idoc_native_config,
        project.source_options.idoc_native_config
    );
}

#[test]
fn unsupported_external_config_remains_executable_without_certificate() {
    let dir = TempDir::new();
    let text = config().replace("QUALIFIED", "UNKNOWN_DIRECTIVE x");
    let imported = mfd::import(&write_design(&dir.0, &text)).unwrap();
    assert!(imported.project.source_options.idoc.is_some());
    assert!(imported.project.source_options.idoc_native_config.is_none());
    assert!(engine::validate(&imported.project).is_empty());
}

#[test]
fn export_preflight_pairs_descriptor_with_every_source_and_target_boundary() {
    let dir = TempDir::new();
    let (base, _) = exported_project(&dir.0);
    let output = dir.0.join("should-not-exist.mfd");

    let mut primary_source = base.clone();
    primary_source.source.name = "Altered".into();
    let error = mfd::preflight_export(&primary_source, &output)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("source IDoc native configuration descriptor does not match"),
        "{error}"
    );

    let mut named_source = base.clone();
    let mut altered = base.source.clone();
    altered.name = "Altered".into();
    named_source.extra_sources.push(NamedSource {
        name: "secondary".into(),
        path: "secondary.idoc".into(),
        schema: altered,
        options: base.source_options.clone(),
        dynamic_path: None,
    });
    let error = mfd::preflight_export(&named_source, &output)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("additional source IDoc native configuration descriptor does not match"),
        "{error}"
    );

    let mut primary_target = base.clone();
    primary_target.target = base.source.clone();
    primary_target.target.name = "Altered".into();
    primary_target.target_path = Some("target.idoc".into());
    primary_target.target_options = base.source_options.clone();
    primary_target.root = Scope::default();
    let error = mfd::preflight_export(&primary_target, &output)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("target IDoc native configuration descriptor does not match"),
        "{error}"
    );

    let mut named_target = base.clone();
    let mut altered = base.source.clone();
    altered.name = "Altered".into();
    named_target.extra_targets.push(NamedTarget {
        name: "secondary output".into(),
        path: Some("secondary.idoc".into()),
        schema: altered,
        options: base.source_options.clone(),
        root: Scope::default(),
    });
    let error = mfd::preflight_export(&named_target, &output)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("additional target IDoc native configuration descriptor does not match"),
        "{error}"
    );

    for mutate in [
        |project: &mut Project| project.source_options.edi_kind = None,
        |project: &mut Project| project.source_options.edi_kind = Some(EdiBoundaryKind::X12),
        |project: &mut Project| project.source_options.idoc = None,
    ] {
        let mut project = base.clone();
        mutate(&mut project);
        let error = mfd::preflight_export(&project, &output)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("IDoc native configuration descriptor"),
            "{error}"
        );
    }
    assert!(!output.exists());
}

#[test]
fn invalid_stale_and_unresolved_descriptors_publish_no_artifacts() {
    let dir = TempDir::new();
    let (base, _) = exported_project(&dir.0);
    type ProjectMutation = fn(&mut Project);
    let mutations: [(&str, ProjectMutation); 4] = [
        ("wrong-dialect", |project| {
            project.source_options.edi_kind = Some(EdiBoundaryKind::X12);
        }),
        ("missing-layout", |project| {
            project.source_options.idoc = None;
        }),
        ("stale-schema", |project| {
            project.source.name = "Altered".into();
        }),
        ("unresolved-config", |project| {
            project.source_options.edi_config_reference = Some("missing/parser.txt".into());
        }),
    ];
    for (name, mutate) in mutations {
        let mut project = base.clone();
        mutate(&mut project);
        let output_dir = dir.0.join(name);
        let output = output_dir.join("portable.mfd");
        assert!(mfd::export(&project, &output).is_err(), "{name}");
        assert!(!output_dir.exists(), "{name} published artifacts");
    }
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus; informational only"]
fn local_idoc_order_retains_descriptor_without_claiming_native_compatibility() {
    let sample =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples/IDoc_Order.mfd");
    if !sample.is_file() {
        return;
    }
    let imported = mfd::import(&sample).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let descriptor = imported
        .project
        .source_options
        .idoc_native_config
        .as_ref()
        .unwrap();
    assert_eq!(
        descriptor.project().unwrap(),
        (
            imported.project.source.clone(),
            imported.project.source_options.idoc.clone().unwrap()
        )
    );
    let dir = TempDir::new();
    let output = dir.0.join("idoc-ferrule.mfd");
    mfd::export(&imported.project, &output).unwrap();
    let reimported = mfd::import(&output).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported.project.source_options.idoc_native_config,
        Some(descriptor.clone())
    );
    let report = mfd::preflight_export(&reimported.project, &dir.0.join("native.mfd")).unwrap();
    assert!(!report.is_native_compatible());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == Feature::EdiSchema)
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == Feature::EdiLayout)
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == Feature::EdiConfigDescriptor)
    );
}
