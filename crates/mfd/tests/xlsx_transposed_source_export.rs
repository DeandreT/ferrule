use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{AggregateOp, Binding, Graph, Node, Project, Scope};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-transposed-source-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(indexed: bool, integer: bool) -> Project {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx-transposed-rows.mfd");
    let mut text = std::fs::read_to_string(fixture).unwrap();
    if !integer {
        text = text
            .replace(
                r#"annotation="Amount" datatype="integer""#,
                r#"annotation="Amount" datatype="double""#,
            )
            .replace(
                r#"<field1 name="Amount" type="integer"/>"#,
                r#"<field1 name="Amount" type="double"/>"#,
            );
    }
    if !indexed {
        text = text
            .replace(r#"<entry name="n" type="attribute" outkey="104"/>"#, "")
            .replace(
                r#"<vertex vertexkey="104"><edges><edge vertexkey="204"/></edges></vertex>"#,
                "",
            );
    }
    let directory = Scratch::new();
    let design = directory.0.join("source.mfd");
    std::fs::write(&design, text).unwrap();
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    imported.project
}

fn workbook(driver: &[(u16, &str)], amounts: &[(u16, &str)]) -> Vec<u8> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Quarterly").unwrap();
    for &(column, value) in driver {
        sheet.write_string(2, column, value).unwrap();
        sheet.write_number(8, column, 1.25).unwrap();
    }
    // Text-backed numbers exercise exact integer parsing rather than a
    // rounded XLSX numeric cell supplied by the test itself.
    for &(column, value) in amounts {
        sheet.write_string(6, column, value).unwrap();
    }
    workbook.save_to_buffer().unwrap()
}

fn read(project: &Project, bytes: &[u8]) -> Instance {
    Instance::Repeated(
        format_xlsx::from_bytes_transposed(
            bytes,
            &project.source,
            project.source_options.xlsx_sheet.as_deref(),
            &project.source_options.xlsx_rows,
        )
        .unwrap(),
    )
}

fn aggregate(mut project: Project, function: AggregateOp, field: &str, computed: bool) -> Project {
    let ty = match function {
        AggregateOp::Count => ScalarType::Int,
        AggregateOp::Join => ScalarType::String,
        _ => ScalarType::Float,
    };
    project.target = SchemaNode::group("Report", vec![SchemaNode::scalar("Value", ty)]);
    project.target_path = Some("report.xml".into());
    project.target_options = Default::default();
    project.graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::Aggregate {
                    function,
                    collection: Vec::new(),
                    value: if computed {
                        Vec::new()
                    } else {
                        vec![field.into()]
                    },
                    expression: computed.then_some(1),
                    arg: matches!(function, AggregateOp::ItemAt | AggregateOp::Join).then_some(2),
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec![field.into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Const {
                    value: if function == AggregateOp::Join {
                        Value::String(",".into())
                    } else {
                        Value::Int(2)
                    },
                },
            ),
        ]),
    };
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Default::default()
    };
    let errors = engine::validate(&project);
    assert!(errors.is_empty(), "{errors:?}");
    project
}

fn assert_refused(project: &Project, reason: &str) {
    let directory = Scratch::new();
    let design = directory.0.join("mapping.mfd");
    let sibling = directory.0.join("mapping-target.xsd");
    std::fs::write(&design, "existing design").unwrap();
    std::fs::write(&sibling, "existing schema").unwrap();
    let check = |result| match result {
        Err(mfd::MfdError::Unsupported(message)) => assert!(message.contains(reason), "{message}"),
        other => panic!("expected {reason}: {other:?}"),
    };
    check(mfd::preflight_export(project, &design));
    for profile in [
        mfd::ExportProfile::FerruleExtensions,
        mfd::ExportProfile::NativeMfd,
    ] {
        check(mfd::export_with_profile(project, &design, profile));
        let absent = directory.0.join("absent/mapping.mfd");
        check(mfd::export_with_profile(project, &absent, profile));
        assert!(!absent.parent().unwrap().exists());
    }
    assert_eq!(std::fs::read_to_string(design).unwrap(), "existing design");
    assert_eq!(std::fs::read_to_string(sibling).unwrap(), "existing schema");
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 2);
}

#[test]
fn later_row_aggregates_refuse_extra_non_driver_cells_and_missing_cells() {
    let driver_hole = workbook(&[(0, "A"), (2, "C")], &[(0, "10"), (1, "99"), (2, "20")]);
    let missing_amount = workbook(&[(0, "A"), (1, "B"), (2, "C")], &[(0, "10"), (2, "20")]);
    for (bytes, count, selected) in [
        (&driver_hole, 2, Some(Instance::Scalar(Value::Float(20.0)))),
        (&missing_amount, 3, Some(Instance::Scalar(Value::Null))),
    ] {
        for function in [
            AggregateOp::Sum,
            AggregateOp::Count,
            AggregateOp::ItemAt,
            AggregateOp::Avg,
            AggregateOp::Min,
            AggregateOp::Max,
            AggregateOp::Join,
        ] {
            let project = aggregate(project(true, false), function, "Amount", false);
            let source = read(&project, bytes);
            let output = engine::run(&project, &source).unwrap();
            match function {
                AggregateOp::Sum => assert_eq!(
                    output.field("Value"),
                    Some(&Instance::Scalar(Value::Float(30.0)))
                ),
                AggregateOp::Count => assert_eq!(
                    output.field("Value"),
                    Some(&Instance::Scalar(Value::Int(count)))
                ),
                AggregateOp::ItemAt => assert_eq!(output.field("Value"), selected.as_ref()),
                _ => {}
            }
            assert_refused(&project, "aggregates over later");
        }
    }
}

#[test]
fn computed_transposed_aggregates_refuse_even_with_a_dense_workbook() {
    let bytes = workbook(&[(0, "A"), (1, "B")], &[(0, "10"), (1, "20")]);
    for function in [AggregateOp::Sum, AggregateOp::Count, AggregateOp::ItemAt] {
        let project = aggregate(project(true, false), function, "Amount", true);
        engine::run(&project, &read(&project, &bytes)).unwrap();
        assert_refused(&project, "computed aggregates");
    }
}

#[test]
fn integer_rows_refuse_without_losing_text_backed_i64_values() {
    for value in [
        9_007_199_254_740_993_i64,
        -9_007_199_254_740_993,
        i64::MIN,
        i64::MAX,
    ] {
        let project = project(true, true);
        let bytes = workbook(&[(0, "A")], &[(0, &value.to_string())]);
        let source = read(&project, &bytes);
        let Instance::Repeated(rows) = &source else {
            panic!("expected records")
        };
        assert_eq!(
            rows[0].field("Amount"),
            Some(&Instance::Scalar(Value::Int(value)))
        );
        engine::run(&project, &source).unwrap();
        assert_refused(&project, "integer row");
    }
}

#[test]
fn direct_reads_roundtrip_sparse_workbooks_with_and_without_column_index() {
    let bytes = workbook(
        &[(0, "A"), (2, "C"), (3, "D")],
        &[(0, "10"), (1, "99"), (3, "20")],
    );
    for indexed in [false, true] {
        let project = project(indexed, false);
        let source = read(&project, &bytes);
        let Instance::Repeated(rows) = &source else {
            panic!("expected records")
        };
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[1].field("Category"),
            Some(&Instance::Scalar(Value::String("C".into())))
        );
        assert_eq!(
            rows[1].field("Amount"),
            Some(&Instance::Scalar(Value::Null))
        );
        if indexed {
            assert_eq!(rows[1].field("n"), Some(&Instance::Scalar(Value::Int(3))));
        }
        let expected = engine::run(&project, &source).unwrap();
        let directory = Scratch::new();
        let design = directory.0.join("mapping.mfd");
        assert!(mfd::export(&project, &design).unwrap().is_empty());
        let text = std::fs::read_to_string(&design).unwrap();
        assert_eq!(
            text.matches(r#"displayselectionmode="selection""#).count(),
            2
        );
        assert_eq!(text.matches(r#"<component name="filter""#).count(), 2);
        assert!(!text.contains("ferrule-datatype"));
        let imported = mfd::import(&design).unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(imported.project.source, project.source);
        assert_eq!(imported.project.source_options, project.source_options);
        let reloaded = read(&imported.project, &bytes);
        assert_eq!(reloaded, source);
        assert_eq!(engine::run(&imported.project, &reloaded).unwrap(), expected);
    }
}

#[test]
fn driver_row_aggregates_remain_exportable_with_gaps() {
    let bytes = workbook(&[(0, "10"), (2, "20")], &[(0, "1"), (1, "99")]);
    for (function, expected) in [
        (AggregateOp::Sum, Value::Float(30.0)),
        (AggregateOp::Count, Value::Int(2)),
        (AggregateOp::ItemAt, Value::Float(20.0)),
    ] {
        let mut base = project(true, false);
        let SchemaKind::Group { children, .. } = &mut base.source.kind else {
            unreachable!()
        };
        children[0].kind = SchemaKind::Scalar {
            ty: ScalarType::Float,
        };
        let project = aggregate(base, function, "Category", false);
        let source = read(&project, &bytes);
        let expected_output = engine::run(&project, &source).unwrap();
        assert_eq!(
            expected_output.field("Value"),
            Some(&Instance::Scalar(expected))
        );
        let directory = Scratch::new();
        let design = directory.0.join("mapping.mfd");
        assert!(mfd::export(&project, &design).unwrap().is_empty());
        let imported = mfd::import(&design).unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(
            engine::run(&imported.project, &read(&imported.project, &bytes)).unwrap(),
            expected_output
        );
    }
}
