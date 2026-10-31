//! Transposed XLSX targets: import, execution, and export round trips of a
//! self-authored design in the shape the reference application saves.

use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaNode, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-xlsx-transposed-target-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// A copy of the fixture design, edited by `edit`, beside its source
    /// schema and input.
    fn design(&self, edit: impl FnOnce(String) -> String) -> PathBuf {
        for name in [
            "xlsx-transposed-target-source.xsd",
            "xlsx-transposed-target-source.xml",
        ] {
            std::fs::copy(fixture(name), self.0.join(name)).unwrap();
        }
        let original = std::fs::read_to_string(fixture("xlsx-transposed-target.mfd")).unwrap();
        let path = self.0.join("design.mfd");
        std::fs::write(&path, edit(original)).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source(project: &mapping::Project) -> Instance {
    format_xml::read(
        &fixture("xlsx-transposed-target-source.xml"),
        &project.source,
    )
    .unwrap()
}

fn record(name: &str, amount: Value, column: i64) -> Instance {
    Instance::Group(
        (vec![
            (
                "Range1".to_string(),
                Instance::Scalar(Value::String(name.into())),
            ),
            ("Range2".to_string(), Instance::Scalar(amount)),
            ("n".to_string(), Instance::Scalar(Value::Int(column))),
        ])
        .into(),
    )
}

#[test]
fn imported_transposed_target_runs_like_the_reference_layout() {
    let imported = mfd::import(&fixture("xlsx-transposed-target.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = imported.project;
    // The reference application drops Cell annotations when it saves, so
    // field names come from the range ids.
    assert_eq!(
        project.target,
        SchemaNode::group(
            "Pivot",
            vec![
                SchemaNode::scalar("Range1", ScalarType::String),
                SchemaNode::scalar("Range2", ScalarType::Float),
                SchemaNode::scalar("n", ScalarType::Int),
            ],
        )
    );
    assert_eq!(project.target_options.xlsx_rows, [2, 4]);
    assert_eq!(project.target_options.xlsx_sheet.as_deref(), Some("Pivot"));
    assert_eq!(project.target_path.as_deref(), Some("pivot.xlsx"));
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );

    let output = engine::run(&project, &source(&project)).unwrap();
    let records = output.as_repeated().unwrap();
    let bytes = format_xlsx::to_bytes_transposed(
        &project.target,
        records,
        Some("Pivot"),
        &project.target_options.xlsx_rows,
    )
    .unwrap();
    // The reference application writes B2/B4, E2, and I2/I4 for this input.
    assert_eq!(
        format_xlsx::from_bytes_transposed(&bytes, &project.target, Some("Pivot"), &[2, 4])
            .unwrap(),
        vec![
            record("Alpha", Value::Float(10.0), 2),
            record("Bravo", Value::Null, 5),
            record("Echo", Value::Float(50.0), 9),
        ]
    );
}

#[test]
fn transposed_target_exports_and_reimports() {
    let project = mfd::import(&fixture("xlsx-transposed-target.mfd"))
        .unwrap()
        .project;
    let dir = TempDir::new("roundtrip");
    let exported = dir.0.join("exported.mfd");
    let warnings = mfd::export(&project, &exported).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let text = std::fs::read_to_string(&exported).unwrap();
    assert!(
        text.contains(r#"<range id="2" start="4" count="1"/>"#),
        "{text}"
    );
    assert_eq!(
        text.matches(r#"<entry name="n" type="attribute" inpkey="#)
            .count(),
        2,
        "{text}"
    );
    assert!(
        text.contains("displayselectionmode=\"selection\""),
        "{text}"
    );
    assert!(
        text.contains("<excel outputinstance=\"pivot.xlsx\"/>"),
        "{text}"
    );

    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.target, project.target);
    assert_eq!(reimported.project.target_options, project.target_options);
    assert_eq!(reimported.project.target_path, project.target_path);
    let input = source(&project);
    assert_eq!(
        engine::run(&reimported.project, &input).unwrap(),
        engine::run(&project, &input).unwrap()
    );
}

#[test]
fn rows_that_do_not_follow_the_driver_row_are_reported() {
    let dir = TempDir::new("partial-index");
    let partial = dir.design(|text| {
        text.replace(
            r#"<edge vertexkey="203"/><edge vertexkey="204"/>"#,
            r#"<edge vertexkey="203"/>"#,
        )
    });
    let warnings = mfd::import(&partial).unwrap().warnings;
    assert_eq!(
        warnings,
        [
            "transposed XLSX target `Pivot` does not connect one `n` value to every row; ferrule places every row of a record at the driver row's `n`"
        ]
    );

    let dir = TempDir::new("other-context");
    let other = dir.design(|text| {
        text.replace(
            r#"<vertex vertexkey="14"><edges><edge vertexkey="202"/></edges></vertex>"#,
            r#"<vertex vertexkey="11"><edges><edge vertexkey="202"/></edges></vertex>"#,
        )
    });
    let warnings = mfd::import(&other).unwrap().warnings;
    assert!(
        warnings.iter().any(|warning| warning
            == "transposed XLSX target `Pivot` row `Range2` is fed from another repeated context than driver row `Range1`; ferrule writes it once per driver item"),
        "{warnings:?}"
    );

    let dir = TempDir::new("update");
    let update = dir.design(|text| {
        text.replace(
            r#"<excel outputinstance="pivot.xlsx"/>"#,
            r#"<excel outputinstance="pivot.xlsx" updateexistingfile="1"/>"#,
        )
    });
    let imported = mfd::import(&update).unwrap();
    assert_eq!(
        imported.warnings,
        [
            "xlsx component `Pivot` updates a transposed table in an existing workbook; ferrule writes transposed targets to a new workbook"
        ]
    );
    assert!(!imported.project.target_options.xlsx_update_existing);
}

#[test]
fn transposed_target_export_refuses_unrepresentable_targets() {
    let project = mfd::import(&fixture("xlsx-transposed-target.mfd"))
        .unwrap()
        .project;
    let dir = TempDir::new("refusals");
    let design = dir.0.join("refused.mfd");

    let mut update = project.clone();
    update.target_options.xlsx_update_existing = true;
    let mut header = project.clone();
    header.target_options.has_header_row = Some(true);
    let mut filtered = project.clone();
    filtered.graph.nodes.insert(
        900,
        mapping::Node::Const {
            value: Value::Bool(true),
        },
    );
    filtered.root.filter = Some(900);
    for (label, project, message) in [
        (
            "update",
            update,
            "a transposed XLSX target cannot update an existing workbook",
        ),
        (
            "header",
            header,
            "a transposed XLSX target cannot use flat header, start-row, or column options",
        ),
        (
            "filter",
            filtered,
            "a transposed XLSX target exports only an uncontrolled source iteration with field bindings",
        ),
    ] {
        let error = mfd::export(&project, &design).unwrap_err().to_string();
        assert!(error.contains(message), "{label}: {error}");
        assert!(!design.exists(), "{label}");
    }
}
