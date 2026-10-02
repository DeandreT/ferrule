use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, NamedTarget, Node, Pipeline,
    PipelineInput, PipelineNamedInput, PipelineStage, Project, Scope, ScopeIteration,
};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_export_paths_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> Result<PathBuf> {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, contents)?;
        Ok(path)
    }

    fn project(&self, project: &Project) -> Result<PathBuf> {
        self.write(
            "maps/project.json",
            mapping::project_file::encode_pretty(project)?,
        )
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ferrule"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot(path: &Path) -> Result<(Vec<u8>, SystemTime)> {
    Ok((std::fs::read(path)?, std::fs::metadata(path)?.modified()?))
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn document(root: &str) -> SchemaNode {
    SchemaNode::group(root, vec![SchemaNode::scalar("Value", ScalarType::String)])
}

fn scope(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}

fn project() -> Project {
    Project {
        source: document("Source"),
        target: document("Target"),
        source_path: Some(r"..\data\source.xml".into()),
        target_path: Some("../results/target.xml".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        extra_sources: vec![NamedSource {
            name: "Catalog".into(),
            path: r"..\data\catalog.xml".into(),
            schema: document("Catalog"),
            options: FormatOptions::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "Audit".into(),
            path: Some(r"..\results\audit.xml".into()),
            schema: document("Audit"),
            options: FormatOptions::default(),
            root: scope(1),
        }],
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Catalog".into(), "Value".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 1],
                    },
                ),
            ]),
        },
        root: scope(2),
    }
}

#[test]
fn relocated_export_roundtrips_and_executes_all_static_boundaries() -> Result {
    let temp = TempDir::new()?;
    let project_path = temp.project(&project())?;
    let source = temp.write("data/source.xml", "<Source><Value>first:</Value></Source>")?;
    let catalog = temp.write(
        "data/catalog.xml",
        "<Catalog><Value>second</Value></Catalog>",
    )?;
    std::fs::create_dir_all(temp.0.join("results"))?;
    let before = [
        snapshot(&project_path)?,
        snapshot(&source)?,
        snapshot(&catalog)?,
    ];
    let design = temp.0.join("exports/deep/mapping.mfd");
    let checked = cli::preflight_mfd_export(&project_path, &design)?;
    assert!(checked.is_native_compatible());
    let check = temp.cli(&[
        "export-mfd",
        "--project",
        "maps/project.json",
        "--out",
        "exports/deep/mapping.mfd",
        "--profile",
        "native-mfd",
        "--check",
        "--report-json",
    ]);
    succeeded(&check);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&check.stdout)?["report"],
        serde_json::to_value(&checked)?
    );
    assert!(!design.parent().unwrap().exists());
    succeeded(&temp.cli(&[
        "export-mfd",
        "--project",
        "maps/project.json",
        "--out",
        "exports/deep/mapping.mfd",
        "--profile",
        "native-mfd",
    ]));
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("../../data/source.xml")
    );
    assert_eq!(
        imported.project.target_path.as_deref(),
        Some("../../results/target.xml")
    );
    assert_eq!(
        imported.project.extra_sources[0].path,
        "../../data/catalog.xml"
    );
    assert_eq!(
        imported.project.extra_targets[0].path.as_deref(),
        Some("../../results/audit.xml")
    );
    std::fs::create_dir_all(temp.0.join("roundtrip/nested"))?;
    succeeded(&temp.cli(&[
        "import-mfd",
        "--mfd",
        "exports/deep/mapping.mfd",
        "--out",
        "roundtrip/nested/project.json",
    ]));
    succeeded(&temp.cli(&["run", "--project", "roundtrip/nested/project.json"]));
    for (root, name, value) in [
        ("Target", "target.xml", "first:second"),
        ("Audit", "audit.xml", "second"),
    ] {
        assert_eq!(
            format_xml::read(&temp.0.join("results").join(name), &document(root))?,
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into()))
            )])
        );
    }
    assert_eq!(
        [
            snapshot(&project_path)?,
            snapshot(&source)?,
            snapshot(&catalog)?
        ],
        before
    );
    Ok(())
}

#[test]
fn relocated_export_rebases_wildcard_hints_without_reading_members() -> Result {
    let temp = TempDir::new()?;
    let mut project = project();
    project.source_path = Some(r"..\data\*.xml".into());
    project.extra_sources[0].path = r"..\catalogs\part?.xml".into();
    let saved = temp.project(&project)?;
    let source = temp.write("data/one.xml", "source member bytes")?;
    let before = [snapshot(&saved)?, snapshot(&source)?];
    let design = temp.0.join("exports/deep/wildcard.mfd");
    cli::export_mfd_with_profile(&saved, &design, mfd::ExportProfile::NativeMfd)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("../../data/*.xml")
    );
    assert_eq!(
        imported.project.extra_sources[0].path,
        "../../catalogs/part?.xml"
    );
    assert_eq!([snapshot(&saved)?, snapshot(&source)?], before);
    Ok(())
}

#[test]
fn relocated_export_preserves_url_and_absolute_hints() -> Result {
    let temp = TempDir::new()?;
    for (index, source) in [
        "https://example.test/source.xml?x=1&y=2",
        "/data/source.xml",
        r"C:\Data\source.xml",
        r"\\server\share\source.xml",
    ]
    .into_iter()
    .enumerate()
    {
        let mut project = project();
        project.source_path = Some(source.into());
        project.target_path = Some(r"D:\Results\target.xml".into());
        project.extra_sources[0].path = "https://example.test/catalog.xml".into();
        project.extra_targets[0].path = Some("/results/audit.xml".into());
        let saved = temp.project(&project)?;
        let before = snapshot(&saved)?;
        let design = temp.0.join(format!("exports/deep/preserved-{index}.mfd"));
        cli::export_mfd_with_profile(&saved, &design, mfd::ExportProfile::NativeMfd)?;
        let imported = mfd::import(&design)?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(imported.project.source_path.as_deref(), Some(source));
        assert_eq!(
            imported.project.target_path.as_deref(),
            Some(r"D:\Results\target.xml")
        );
        assert_eq!(
            imported.project.extra_sources[0].path,
            "https://example.test/catalog.xml"
        );
        assert_eq!(
            imported.project.extra_targets[0].path.as_deref(),
            Some("/results/audit.xml")
        );
        assert_eq!(snapshot(&saved)?, before);
    }
    Ok(())
}

#[test]
fn relocated_export_keeps_dynamic_path_expressions_and_preview_hints() -> Result {
    let temp = TempDir::new()?;
    let mut project = project();
    project.source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    project.extra_targets.clear();
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: Vec::new(),
                frame: Some(vec!["File".into()]),
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Catalog".into(), "Value".into()],
                frame: None,
            },
        ),
    ]);
    project.extra_sources[0].path = r"..\previews\catalog.xml".into();
    project.extra_sources[0].dynamic_path = Some(DynamicSourcePath {
        node: 0,
        iteration: vec!["File".into()],
    });
    project.target = SchemaNode::group("Target", vec![document("Row").repeating()]);
    project.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Source(vec!["File".into()]),
            ..scope(1)
        }],
        ..Scope::default()
    };
    let saved = temp.project(&project)?;
    let before = snapshot(&saved)?;
    let design = temp.0.join("exports/deep/dynamic.mfd");
    cli::export_mfd(&saved, &design)?;
    let xml = std::fs::read_to_string(&design)?;
    assert!(xml.contains(r"..\previews\catalog.xml"));
    assert!(!xml.contains("../../previews/catalog.xml"));
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.extra_sources[0].path,
        r"..\previews\catalog.xml"
    );
    let dynamic = imported.project.extra_sources[0]
        .dynamic_path
        .as_ref()
        .unwrap();
    assert_eq!(dynamic.iteration, ["File"]);
    assert!(
        matches!(&imported.project.graph.nodes[&dynamic.node], Node::SourceField { path, frame: Some(frame) } if path.is_empty() && frame == &["File"])
    );
    assert_eq!(snapshot(&saved)?, before);
    Ok(())
}

fn pipeline() -> Pipeline {
    let mut first = project();
    first.extra_targets.clear();
    first.target = document("Buffer");
    first.target_path = Some(r"..\previews\buffer.xml".into());
    let mut final_stage = project();
    final_stage.source = document("Buffer");
    final_stage.source_path = Some(r"..\previews\buffer.xml".into());
    final_stage.extra_sources.clear();
    final_stage.root = scope(0);
    final_stage.extra_targets[0].root = scope(0);
    final_stage.graph.nodes = BTreeMap::from([(
        0,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    )]);
    Pipeline {
        main_mapping_path: Some("../identities/main.mfd".into()),
        stages: vec![
            PipelineStage {
                id: "first".into(),
                mapping_path: Some("../identities/first.mfd".into()),
                project: first,
                source: PipelineInput::Host {
                    name: "source".into(),
                },
                extra_sources: vec![PipelineNamedInput {
                    name: "Catalog".into(),
                    from: PipelineInput::Host {
                        name: "catalog".into(),
                    },
                }],
            },
            PipelineStage {
                id: "last".into(),
                mapping_path: Some("../identities/last.mfd".into()),
                project: final_stage,
                source: PipelineInput::StageTarget {
                    stage: "first".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    }
}

#[test]
fn relocated_pipeline_export_preserves_connected_previews_and_static_resources() -> Result {
    let temp = TempDir::new()?;
    let pipeline = pipeline();
    let saved = temp.write(
        "maps/pipeline.json",
        mapping::pipeline_file::encode_pretty(&pipeline)?,
    )?;
    let source = temp.write("data/source.xml", "<Source><Value>first:</Value></Source>")?;
    let catalog = temp.write(
        "data/catalog.xml",
        "<Catalog><Value>second</Value></Catalog>",
    )?;
    std::fs::create_dir_all(temp.0.join("results"))?;
    let before = [snapshot(&saved)?, snapshot(&source)?, snapshot(&catalog)?];
    let design = temp.0.join("exports/deep/pipeline.mfd");
    let checked = cli::preflight_mfd_pipeline_export(&saved, &design)?;
    assert!(checked.is_native_compatible());
    succeeded(&temp.cli(&[
        "export-mfd",
        "--project",
        "maps/pipeline.json",
        "--out",
        "exports/deep/pipeline.mfd",
        "--pipeline",
        "--profile",
        "native-mfd",
        "--check",
    ]));
    assert!(!design.parent().unwrap().exists());
    succeeded(&temp.cli(&[
        "export-mfd",
        "--project",
        "maps/pipeline.json",
        "--out",
        "exports/deep/pipeline.mfd",
        "--pipeline",
        "--profile",
        "native-mfd",
    ]));
    let xml = std::fs::read_to_string(&design)?;
    assert!(xml.contains("../../previews/buffer.xml"));
    let imported = mfd::import_pipeline(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.pipeline.stages[0].project.source_path.as_deref(),
        Some("../../data/source.xml")
    );
    assert_eq!(
        imported.pipeline.stages[0].project.extra_sources[0].path,
        "../../data/catalog.xml"
    );
    let final_stage = &imported.pipeline.stages.last().unwrap().project;
    assert_eq!(
        final_stage.target_path.as_deref(),
        Some("../../results/target.xml")
    );
    assert_eq!(
        final_stage.extra_targets[0].path.as_deref(),
        Some("../../results/audit.xml")
    );
    std::fs::create_dir_all(temp.0.join("roundtrip/nested"))?;
    succeeded(&temp.cli(&[
        "import-mfd",
        "--mfd",
        "exports/deep/pipeline.mfd",
        "--out",
        "roundtrip/nested/pipeline.json",
        "--pipeline",
    ]));
    let imported_path = temp.0.join("roundtrip/nested/pipeline.json");
    let reimported = mapping::pipeline_file::decode_str(&std::fs::read_to_string(&imported_path)?)?;
    let mut args = vec![
        "run-pipeline".to_string(),
        "--pipeline".into(),
        "roundtrip/nested/pipeline.json".into(),
    ];
    let mut hosts = BTreeMap::new();
    for stage in &reimported.stages {
        if let PipelineInput::Host { name } = &stage.source {
            hosts.insert(
                name,
                imported_path
                    .parent()
                    .unwrap()
                    .join(stage.project.source_path.as_ref().unwrap())
                    .canonicalize()?,
            );
        }
        for binding in &stage.extra_sources {
            if let PipelineInput::Host { name } = &binding.from {
                let source = stage
                    .project
                    .extra_sources
                    .iter()
                    .find(|source| source.name == binding.name)
                    .unwrap();
                let path = imported_path
                    .parent()
                    .unwrap()
                    .join(&source.path)
                    .canonicalize()?;
                if let Some(previous) = hosts.insert(name, path.clone()) {
                    assert_eq!(previous, path);
                }
            }
        }
    }
    for (name, path) in hosts {
        args.extend([
            "--input".into(),
            name.clone(),
            path.to_str().unwrap().into(),
        ]);
    }
    let last_id = &reimported.stages.last().unwrap().id;
    args.extend([
        "--output".into(),
        last_id.clone(),
        "results/target.xml".into(),
    ]);
    args.extend([
        "--named-output".into(),
        last_id.clone(),
        final_stage.extra_targets[0].name.clone(),
        "results/audit.xml".into(),
    ]);
    succeeded(&temp.cli(&args.iter().map(String::as_str).collect::<Vec<_>>()));
    for (root, name) in [("Target", "target.xml"), ("Audit", "audit.xml")] {
        assert_eq!(
            format_xml::read(&temp.0.join("results").join(name), &document(root))?,
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String("first:second".into()))
            )])
        );
    }
    assert_eq!(
        [snapshot(&saved)?, snapshot(&source)?, snapshot(&catalog)?],
        before
    );
    Ok(())
}

#[test]
fn relocated_native_rejection_leaves_saved_projects_and_existing_artifacts_untouched() -> Result {
    let temp = TempDir::new()?;
    let mut project = project();
    project.graph.nodes.insert(
        3,
        Node::Call {
            function: "isbn10_to_isbn13".into(),
            args: vec![0],
        },
    );
    project.root = scope(3);
    let saved = temp.project(&project)?;
    let design = temp.write("exports/deep/mapping.mfd", "existing design")?;
    let schema = temp.write("exports/deep/mapping-source.xsd", "existing schema")?;
    let before = [snapshot(&saved)?, snapshot(&design)?, snapshot(&schema)?];
    let checked = cli::preflight_mfd_export(&saved, &design)?;
    assert!(!checked.is_native_compatible());
    assert!(cli::export_mfd_with_profile(&saved, &design, mfd::ExportProfile::NativeMfd).is_err());
    assert_eq!(
        [snapshot(&saved)?, snapshot(&design)?, snapshot(&schema)?],
        before
    );
    let missing = temp.0.join("not-created/deep/mapping.mfd");
    assert!(cli::export_mfd_with_profile(&saved, &missing, mfd::ExportProfile::NativeMfd).is_err());
    assert!(!missing.parent().unwrap().exists());
    let mut unsupported = pipeline();
    unsupported.stages[0].source = PipelineInput::StageTarget {
        stage: "last".into(),
        target: None,
    };
    let pipeline_path = temp.write(
        "maps/unsupported.json",
        mapping::pipeline_file::encode_pretty(&unsupported)?,
    )?;
    let pipeline_before = snapshot(&pipeline_path)?;
    assert!(cli::preflight_mfd_pipeline_export(&pipeline_path, &missing).is_err());
    assert!(
        cli::export_mfd_pipeline_with_profile(
            &pipeline_path,
            &missing,
            mfd::ExportProfile::NativeMfd
        )
        .is_err()
    );
    assert!(!missing.parent().unwrap().exists());
    assert_eq!(snapshot(&pipeline_path)?, pipeline_before);
    Ok(())
}
