use std::collections::BTreeMap;
use std::error::Error;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, EdiBoundaryKind, EdiConfigDependency, EdiImpliedDecimal, EdiLexicalFormat,
    EdiLexicalKind, EdiValueConstraint, ExternalHttpMode, ExternalPayloadFormat,
    ExternalSourceOptions, FormatOptions, Graph, HttpTimeoutSeconds, IdocFieldLayout, IdocLayout,
    IdocSegmentLayout, NamedSource, NamedTarget, Node, Project, Scope,
};
use mfd::{
    ExportCompatibility, ExportCompatibilityFeature as Feature, ExportProfile, ExportReport,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_export_compatibility_{}_{}",
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

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn assert_rejected(project: &Project, path: &Path, expected: &ExportReport) {
    let error = mfd::export_with_profile(project, path, ExportProfile::NativeMapForce).unwrap_err();
    let mfd::MfdError::IncompatibleExport(report) = error else {
        panic!("expected compatibility rejection, got {error}");
    };
    assert_eq!(*report, *expected);
}

#[test]
fn native_preflight_is_read_only_and_matches_published_report() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("new-directory/mapping.mfd");
    let project = project();
    let report = mfd::preflight_export(&project, &path)?;
    assert_eq!(report.compatibility, ExportCompatibility::NativeMapForce);
    assert!(report.is_native_compatible());
    assert!(!path.parent().unwrap().exists());
    let published = mfd::export_with_profile(&project, &path, ExportProfile::NativeMapForce)?;
    assert_eq!(report, published);
    let xml = std::fs::read_to_string(&path)?;
    assert!(
        xml.contains("ferrule-primary-source"),
        "ordinary round-trip annotations do not imply a semantic dependency"
    );
    let imported = mfd::import(&path)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(mfd::export(&project, &path)?.is_empty());
    assert_eq!(
        std::fs::read_to_string(&path)?,
        xml,
        "legacy export keeps the same artifact representation"
    );
    assert_eq!(
        serde_json::to_value(&report)?["compatibility"],
        "native_map_force"
    );
    Ok(())
}

#[test]
fn internal_functions_reject_before_replacing_any_existing_artifact() -> Result<(), Box<dyn Error>>
{
    let temp = TempDir::new()?;
    let path = temp.0.join("mapping.mfd");
    let source_schema = temp.0.join("mapping-source.xsd");
    let target_schema = temp.0.join("mapping-target.xsd");
    for file in [&path, &source_schema, &target_schema] {
        std::fs::write(file, "preserve existing content")?;
    }
    let mut project = project();
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "isbn10_to_isbn13".into(),
            args: vec![0],
        },
    );
    project.root.bindings[0].node = 1;
    let report = mfd::preflight_export(&project, &path)?;
    assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
    assert!(report.warnings.is_empty());
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].feature, Feature::FerruleComponent);
    assert_eq!(report.issues[0].component, "isbn10_to_isbn13");
    assert!(report.issues[0].component_uid.is_some());
    assert_rejected(&project, &path, &report);
    for file in [&path, &source_schema, &target_schema] {
        assert_eq!(std::fs::read_to_string(file)?, "preserve existing content");
    }
    assert_eq!(std::fs::read_dir(&temp.0)?.count(), 3);
    assert_eq!(
        mfd::export_with_profile(&project, &path, ExportProfile::default())?,
        report
    );
    assert!(std::fs::read_to_string(path)?.contains("library=\"ferrule\""));
    Ok(())
}

#[test]
fn edi_reports_schema_layout_and_each_retained_constraint_on_named_boundaries()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("unpublished/mapping.mfd");
    let mut project = project();
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Idoc),
        idoc: Some(IdocLayout::new(vec![IdocSegmentLayout::new(
            "Record",
            vec![IdocFieldLayout::new(
                "Value",
                NonZeroU32::new(1).unwrap(),
                NonZeroU32::new(8).unwrap(),
            )?],
        )?])?),
        edi_lexical_formats: vec![
            EdiLexicalFormat::new(
                vec!["Value".into()],
                EdiLexicalKind::Decimal { max_chars: 8 },
            )
            .unwrap(),
        ],
        edi_implied_decimals: vec![EdiImpliedDecimal::new(vec!["Value".into()], 2).unwrap()],
        edi_value_constraints: vec![
            EdiValueConstraint::new(vec!["Value".into()], 1, 8, Vec::new()).unwrap(),
        ],
        ..FormatOptions::default()
    };
    project.extra_sources.push(NamedSource {
        name: "Additional input".into(),
        path: "input.idoc".into(),
        schema: project.source.clone(),
        options: options.clone(),
        dynamic_path: None,
    });
    project.extra_targets.push(NamedTarget {
        name: "Additional output".into(),
        path: Some("output.idoc".into()),
        schema: project.target.clone(),
        options,
        root: project.root.clone(),
    });
    let report = mfd::preflight_export(&project, &path)?;
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for component in ["Additional input", "Additional output"] {
        let features = report
            .issues
            .iter()
            .filter(|issue| issue.component == component)
            .map(|issue| issue.feature)
            .collect::<Vec<_>>();
        assert_eq!(
            features,
            [
                Feature::EdiSchema,
                Feature::EdiLayout,
                Feature::EdiLexicalFormats,
                Feature::EdiImpliedDecimals,
                Feature::EdiValueConstraints
            ]
        );
    }
    assert_rejected(&project, &path, &report);
    assert!(!path.parent().unwrap().exists());
    Ok(())
}

#[test]
fn unresolved_edi_configuration_is_a_typed_blocker() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let mut project = project();
    project.source_options.edi_kind = Some(EdiBoundaryKind::Edifact);
    project.source_options.edi_config_reference = Some(EdiConfigDependency::MissingConfiguration);
    let report = mfd::preflight_export(&project, &temp.0.join("mapping.mfd"))?;
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == Feature::UnresolvedEdiConfiguration)
    );
    assert!(!report.is_native_compatible());
    Ok(())
}

#[test]
fn captured_sources_report_behavior_differences_without_changing_legacy_warnings()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("mapping.mfd");
    let mut project = project();
    project.source_path = Some("https://example.test/query".into());
    project.source_options.external_source = Some(ExternalSourceOptions::http_post(
        ExternalHttpMode::Manual,
        HttpTimeoutSeconds::new(20).unwrap(),
        None,
        None,
        ExternalPayloadFormat::Json,
        Vec::new(),
    )?);
    let report = mfd::preflight_export(&project, &path)?;
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].feature, Feature::CapturedHttpPost);
    assert_rejected(&project, &path, &report);
    assert!(mfd::export(&project, &path)?.is_empty());
    assert!(std::fs::read_to_string(&path)?.contains("httpmethod=\"POST\""));

    project.source_path = Some("capture.json".into());
    project.source_options.external_source = Some(ExternalSourceOptions::user_function(
        "Opaque",
        "external implementation",
        ExternalPayloadFormat::Json,
    )?);
    let report = mfd::preflight_export(&project, &path)?;
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].feature, Feature::CapturedUserFunction);
    assert_rejected(&project, &path, &report);
    assert!(mfd::export(&project, &path)?.is_empty());
    Ok(())
}

#[test]
fn lossy_warnings_cannot_be_mistaken_for_a_native_export() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("mapping.mfd");
    let mut project = project();
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Missing".into()],
            frame: None,
        },
    );
    let report = mfd::preflight_export(&project, &path)?;
    assert_eq!(report.compatibility, ExportCompatibility::Incomplete);
    assert!(!report.warnings.is_empty());
    assert_rejected(&project, &path, &report);
    assert!(!path.exists());
    assert_eq!(mfd::export(&project, &path)?, report.warnings);
    Ok(())
}

#[test]
fn ordinary_constants_stay_in_the_native_profile() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let mut project = project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("literal".into()),
        },
    );
    let report = mfd::preflight_export(&project, &temp.0.join("mapping.mfd"))?;
    assert!(report.is_native_compatible(), "{report}");
    Ok(())
}
