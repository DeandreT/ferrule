use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_db_query_where_{}_{}",
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

fn database(path: &Path) -> Result<(), rusqlite::Error> {
    Connection::open(path)?.execute_batch(
        "CREATE TABLE Person (ForeignKey INTEGER, Title TEXT, First TEXT); \
         INSERT INTO Person VALUES \
           (1, 'Office Manager', 'Ada'), \
           (1, 'office manager', 'Bea'), \
           (2, 'Program Manager', 'Cal'), \
           (1, NULL, 'Dee'), \
           (1, 'Software Engineer', 'Eli');",
    )
}

fn project() -> Project {
    let nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Title".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("%Manager%".into()),
            },
        ),
        (
            2,
            Node::Call {
                function: "sql_like".into(),
                args: vec![0, 1],
            },
        ),
        (
            3,
            Node::Call {
                function: "exists".into(),
                args: vec![0],
            },
        ),
        (
            4,
            Node::Call {
                function: "exists".into(),
                args: vec![1],
            },
        ),
        (
            5,
            Node::Call {
                function: "and".into(),
                args: vec![3, 4],
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            7,
            Node::If {
                condition: 5,
                then: 2,
                else_: 6,
            },
        ),
        (
            8,
            Node::SourceField {
                path: vec!["ForeignKey".into()],
                frame: None,
            },
        ),
        (
            9,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            10,
            Node::Call {
                function: "equal".into(),
                args: vec![8, 9],
            },
        ),
        (
            11,
            Node::Call {
                function: "exists".into(),
                args: vec![8],
            },
        ),
        (
            12,
            Node::Call {
                function: "exists".into(),
                args: vec![9],
            },
        ),
        (
            13,
            Node::Call {
                function: "and".into(),
                args: vec![11, 12],
            },
        ),
        (
            14,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            15,
            Node::If {
                condition: 13,
                then: 10,
                else_: 14,
            },
        ),
        (
            16,
            Node::Call {
                function: "and".into(),
                args: vec![15, 7],
            },
        ),
        (
            17,
            Node::SourceField {
                path: vec!["First".into()],
                frame: None,
            },
        ),
    ]);
    Project {
        source: SchemaNode::group(
            "Person",
            vec![
                SchemaNode::scalar("ForeignKey", ScalarType::Int),
                SchemaNode::scalar("Title", ScalarType::String),
                SchemaNode::scalar("First", ScalarType::String),
            ],
        )
        .repeating(),
        target: SchemaNode::group(
            "Company",
            vec![SchemaNode::group(
                "Employees",
                vec![
                    SchemaNode::group(
                        "Employee",
                        vec![
                            SchemaNode::scalar("FirstName", ScalarType::String),
                            SchemaNode::scalar("Title", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )],
        ),
        source_path: Some("people.sqlite".into()),
        target_path: Some("out.xml".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Employees".into(),
                children: vec![Scope {
                    target_field: "Employee".into(),
                    iteration: ScopeIteration::Source(Vec::new()),
                    filter: Some(16),
                    bindings: vec![
                        Binding {
                            target_field: "FirstName".into(),
                            node: 17,
                        },
                        Binding {
                            target_field: "Title".into(),
                            node: 0,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn xml(project: &Project, database: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    let source = format_db::read_instance(database, &project.source)?;
    let output = engine::run(project, &source)?;
    format_xml::write(path, &project.target, &output)?;
    Ok(std::fs::read_to_string(path)?)
}

#[test]
fn root_query_where_with_residual_key_is_native_and_round_trips() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    database(&dir.0.join("people.sqlite"))?;
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let expected = xml(
        &project,
        &dir.0.join("people.sqlite"),
        &dir.0.join("before.xml"),
    )?;
    assert_eq!(expected.matches("<Employee>").count(), 2);
    assert!(expected.contains("<FirstName>Ada</FirstName>"));
    assert!(expected.contains("<FirstName>Bea</FirstName>"));

    let design = dir.0.join("native.mfd");
    let report = mfd::preflight_export(&project, &design)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &design, mfd::ExportProfile::NativeMfd)?;
    let rendered = std::fs::read_to_string(&design)?;
    assert!(rendered.contains("condition=\"Title LIKE :sqlparam\""));
    assert!(rendered.contains("name=\"Title where\" library=\"db\""));
    assert!(!rendered.contains("library=\"ferrule\""));
    let reimported = mfd::import(&design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let actual = xml(
        &reimported.project,
        &dir.0.join("people.sqlite"),
        &dir.0.join("after.xml"),
    )?;
    assert_eq!(actual, expected);
    Ok(())
}

#[test]
fn optional_title_pattern_preserves_default_override_and_null_in_native_where()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let database_path = dir.0.join("people.sqlite");
    database(&database_path)?;
    let mut project = project();
    project.graph.nodes.insert(
        18,
        Node::Const {
            value: Value::String("%Manager%".into()),
        },
    );
    project.graph.nodes.insert(
        1,
        Node::RuntimeParameterDefault {
            name: "pattern".into(),
            ty: ScalarType::String,
            default: 18,
            preview: None,
        },
    );
    project.graph.nodes.insert(
        19,
        Node::Const {
            value: Value::Float(1.0),
        },
    );
    project.graph.nodes.insert(
        9,
        Node::RuntimeParameterDefault {
            name: "DepartmentID".into(),
            ty: ScalarType::Int,
            default: 19,
            preview: None,
        },
    );
    assert!(engine::validate(&project).is_empty());
    let design = dir.0.join("optional-pattern.mfd");
    let report = mfd::export_with_profile(&project, &design, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible(), "{report}");
    let restored = mfd::import(&design)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    assert!(engine::validate(&restored.project).is_empty());
    let second_design = dir.0.join("optional-pattern-second.mfd");
    mfd::export_with_profile(
        &restored.project,
        &second_design,
        mfd::ExportProfile::NativeMfd,
    )?;
    let second = mfd::import(&second_design)?;
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
    assert_eq!(
        second.project.graph.nodes.len(),
        restored.project.graph.nodes.len()
    );
    let source = format_db::read_instance(&database_path, &project.source)?;
    for (value, count) in [
        (None, 2),
        (Some(Value::String("%Engineer%".into())), 1),
        (Some(Value::String("%".into())), 3),
        (Some(Value::Null), 0),
    ] {
        let mut parameters = engine::RuntimeParameters::new();
        if let Some(value) = value {
            parameters.insert("pattern", value)?;
        }
        let context = engine::ExecutionContext::new(&design).with_parameters(&parameters);
        let before = engine::run_with_context(&project, &source, &context)?;
        let after = engine::run_with_context(&restored.project, &source, &context)?;
        assert_eq!(
            engine::run_with_context(&second.project, &source, &context)?,
            after
        );
        let before_path = dir.0.join("before-optional.xml");
        let after_path = dir.0.join("after-optional.xml");
        format_xml::write(&before_path, &project.target, &before)?;
        format_xml::write(&after_path, &restored.project.target, &after)?;
        let expected = std::fs::read_to_string(&before_path)?;
        assert_eq!(expected.matches("<Employee>").count(), count);
        assert_eq!(std::fs::read_to_string(&after_path)?, expected);
    }
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("DepartmentID", Value::Int(2))?;
    let context = engine::ExecutionContext::new(&design).with_parameters(&parameters);
    let before = engine::run_with_context(&project, &source, &context)?;
    let after = engine::run_with_context(&restored.project, &source, &context)?;
    assert_eq!(after, before);
    assert_eq!(
        after
            .field("Employees")
            .and_then(|employees| employees.field("Employee"))
            .and_then(ir::Instance::as_repeated)
            .unwrap()
            .len(),
        1
    );
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("DepartmentID", Value::Bool(true))?;
    let context = engine::ExecutionContext::new(&design).with_parameters(&parameters);
    assert!(matches!(
        engine::run_with_context(&project, &source, &context),
        Err(engine::EngineError::RuntimeParameterType {
            expected: ScalarType::Int,
            ..
        })
    ));
    assert!(matches!(
        engine::run_with_context(&restored.project, &source, &context),
        Err(engine::EngineError::RuntimeParameterType {
            expected: ScalarType::Int,
            ..
        })
    ));
    project.graph.nodes.insert(
        18,
        Node::Const {
            value: Value::String("%Man_ger%".into()),
        },
    );
    assert_rejected(&project, &dir.0.join("changed-optional-pattern.mfd"))?;
    Ok(())
}

#[test]
fn required_host_title_pattern_survives_two_native_where_cycles() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let database_path = dir.0.join("people.sqlite");
    database(&database_path)?;
    let mut project = project();
    project.graph.nodes.insert(
        1,
        Node::RuntimeParameter {
            name: "pattern".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    assert!(engine::validate(&project).is_empty());
    let mut projects = vec![project];
    for cycle in 0..2 {
        let design = dir.0.join(format!("required-pattern-{cycle}.mfd"));
        let report = mfd::export_with_profile(
            projects.last().unwrap(),
            &design,
            mfd::ExportProfile::NativeMfd,
        )?;
        assert!(report.is_native_compatible(), "{report}");
        let native = std::fs::read_to_string(&design)?;
        assert!(native.contains("condition=\"Title LIKE :sqlparam\""));
        assert!(native.contains("<component name=\"pattern\" library=\"core\""));
        assert!(!native.contains("name=\"pattern\" optional=\"1\""));
        assert!(!native.contains("library=\"ferrule\""));
        let restored = mfd::import(&design)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        projects.push(restored.project);
    }
    for project in &projects {
        assert!(engine::validate(project).is_empty());
        assert_eq!(
            project
                .graph
                .nodes
                .values()
                .filter(|node| matches!(
                    node,
                    Node::RuntimeParameter {
                        name,
                        ty: ScalarType::String,
                        ..
                    } if name == "pattern"
                ))
                .count(),
            1
        );
        let source = format_db::read_instance(&database_path, &project.source)?;
        assert!(matches!(
            engine::run(project, &source),
            Err(engine::EngineError::MissingRuntimeParameter { name, .. })
                if name == "pattern"
        ));
        assert!(engine::run(project, &ir::Instance::Repeated(Vec::new())).is_ok());
        let mut wrong = engine::RuntimeParameters::new();
        wrong.insert("pattern", Value::Float(f64::INFINITY))?;
        let context = engine::ExecutionContext::new(&database_path).with_parameters(&wrong);
        assert!(matches!(
            engine::run_with_context(project, &source, &context),
            Err(engine::EngineError::RuntimeParameterType {
                expected: ScalarType::String,
                ..
            })
        ));
    }
    for (value, count) in [
        (Value::String("%".into()), 3),
        (Value::String("%Engineer%".into()), 1),
        (Value::Null, 0),
    ] {
        let mut expected = None;
        for (index, project) in projects.iter().enumerate() {
            let source = format_db::read_instance(&database_path, &project.source)?;
            let mut parameters = engine::RuntimeParameters::new();
            parameters.insert("pattern", value.clone())?;
            let context =
                engine::ExecutionContext::new(&database_path).with_parameters(&parameters);
            let output = engine::run_with_context(project, &source, &context)?;
            let path = dir.0.join(format!("required-{index}.xml"));
            format_xml::write(&path, &project.target, &output)?;
            let actual = std::fs::read_to_string(path)?;
            assert_eq!(actual.matches("<Employee>").count(), count);
            if let Some(expected) = &expected {
                assert_eq!(&actual, expected);
            } else {
                expected = Some(actual);
            }
        }
    }
    Ok(())
}

#[test]
fn title_where_keeps_nonmatching_graphs_strictly_rejected() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut changed = project();
    if let Node::Const {
        value: Value::String(pattern),
    } = changed.graph.nodes.get_mut(&1).unwrap()
    {
        *pattern = "%Man_ger%".into();
    }
    assert_rejected(&changed, &dir.0.join("wildcard.mfd"))?;

    let mut changed = project();
    changed.root.children[0].children[0].bindings[1].node = 2;
    assert_rejected(&changed, &dir.0.join("shared.mfd"))?;

    let mut changed = project();
    if let Node::Call { args, .. } = changed.graph.nodes.get_mut(&16).unwrap() {
        args.swap(0, 1);
    }
    assert_rejected(&changed, &dir.0.join("changed-guard.mfd"))?;
    Ok(())
}

fn assert_rejected(project: &Project, path: &Path) -> Result<(), Box<dyn Error>> {
    let report = mfd::preflight_export(project, path)?;
    assert!(!report.is_native_compatible(), "{report}");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| { issue.feature == mfd::ExportCompatibilityFeature::FerruleComponent })
    );
    assert!(matches!(
        mfd::export_with_profile(project, path, mfd::ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!path.exists());
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn local_employee_and_manager_queries_export_natively_and_keep_exact_xml()
-> Result<(), Box<dyn Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    for name in [
        "DB_EmployeeListByTitle",
        "DB_ManagerList_AllOffices",
        "DB_ManagerList_SelectedDepartment",
        "DB_ManagerList_SelectedOffice",
    ] {
        let dir = TempDir::new()?;
        let imported = mfd::import(&samples.join(format!("{name}.mfd")))?;
        assert!(
            imported.warnings.is_empty(),
            "{name}: {:?}",
            imported.warnings
        );
        let source =
            format_db::read_instance(&samples.join("Altova.sqlite"), &imported.project.source)?;
        let before = engine::run(&imported.project, &source)?;
        let before_path = dir.0.join("before.xml");
        format_xml::write(&before_path, &imported.project.target, &before)?;
        let design = dir.0.join("native.mfd");
        let report = mfd::preflight_export(&imported.project, &design)?;
        assert!(report.is_native_compatible(), "{name}: {report}");
        mfd::export_with_profile(&imported.project, &design, mfd::ExportProfile::NativeMfd)?;
        let rendered = std::fs::read_to_string(&design)?;
        assert!(rendered.contains("condition=\"Title LIKE :sqlparam\""));
        assert!(!rendered.contains("library=\"ferrule\""));
        let reimported = mfd::import(&design)?;
        assert!(
            reimported.warnings.is_empty(),
            "{name}: {:?}",
            reimported.warnings
        );
        assert!(engine::validate(&reimported.project).is_empty());
        let source =
            format_db::read_instance(&samples.join("Altova.sqlite"), &reimported.project.source)?;
        let after = engine::run(&reimported.project, &source)?;
        let after_path = dir.0.join("after.xml");
        format_xml::write(&after_path, &reimported.project.target, &after)?;
        assert_eq!(
            std::fs::read(&after_path)?,
            std::fs::read(&before_path)?,
            "{name}"
        );
    }
    Ok(())
}
