use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_fixed_width_parser_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_inline_fixed_width_layout_and_executes() {
    let imported = mfd::import(&fixture("fixed-width.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("fixed-width.txt")
    );

    let SchemaKind::Group {
        children: fields, ..
    } = &imported.project.source.kind
    else {
        panic!("fixed-width source should be a row group");
    };
    assert_eq!(
        fields
            .iter()
            .map(|field| {
                let SchemaKind::Scalar { ty } = field.kind else {
                    panic!("fixed-width field should be scalar");
                };
                (field.name.as_str(), ty)
            })
            .collect::<Vec<_>>(),
        vec![
            ("Code", ScalarType::Int),
            ("Name", ScalarType::String),
            ("Active", ScalarType::Bool),
        ]
    );
    let layout = imported
        .project
        .source_options
        .fixed_width
        .as_ref()
        .unwrap();
    assert_eq!(
        layout
            .field_widths()
            .iter()
            .map(|width| width.get())
            .collect::<Vec<_>>(),
        vec![3, 6, 5]
    );
    assert_eq!(layout.fill_char(), '_');
    assert!(layout.record_delimiters());
    assert!(layout.treat_empty_as_absent());

    let rows = format_csv::read_fixed_width(
        &fixture("fixed-width.txt"),
        &imported.project.source,
        layout,
    )
    .unwrap();
    let output = engine::run(&imported.project, &Instance::Repeated(rows)).unwrap();
    let output = output.as_repeated().unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(
        output[0].field("Code").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    assert_eq!(
        output[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );
    assert_eq!(
        output[1].field("Active").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
}

#[test]
fn unsupported_text_modes_have_specific_warnings() {
    let imported = mfd::import(&fixture("fixed-width-warnings.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 3, "{:?}", imported.warnings);
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("FlexText component `legacy-flex`")
            && warning.contains("configuration `legacy.mft` was not found")
    }));
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("fixed-length string parser `parse-record`")
            && warning.contains("component could not be compiled")
    }));
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("fixed-length component `legacy-encoding`")
            && warning.contains("assumes UTF-8")
    }));
}

#[test]
fn imports_inline_fixed_width_string_parser_and_executes_per_source_row()
-> Result<(), Box<dyn Error>> {
    let imported = mfd::import(&fixture("fixed-width-string-parser.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Repeated(vec![
        Instance::Group(vec![(
            "Raw".into(),
            Instance::Scalar(Value::String("007Ada___".into())),
        )]),
        Instance::Group(vec![(
            "Raw".into(),
            Instance::Scalar(Value::String("012Grace".into())),
        )]),
    ]);
    let output = engine::run(&imported.project, &source)?;
    let rows = output.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Code").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    assert_eq!(
        rows[1].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Grace".into()))
    );

    let dir = TempDir::new()?;
    let design = dir.0.join("fixed-width-parser.mfd");
    let warnings = mfd::export(&imported.project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design)?;
    assert_eq!(xml.matches("usageKind=\"stringparse\"").count(), 1);
    assert!(xml.contains("<text type=\"flf\""));
    assert!(!xml.contains("name=\"flextext_parse_field\""));

    let reimported = mfd::import(&design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let output = engine::run(&reimported.project, &source)?;
    let rows = output.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Code").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    assert_eq!(
        rows[1].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Grace".into()))
    );
    Ok(())
}
