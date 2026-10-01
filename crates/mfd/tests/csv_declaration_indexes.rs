use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};
use mapping::Project;

const FIELDS: &str = "<field0 name=\"A\" type=\"string\"/><field1 name=\"B\" type=\"integer\"/>";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-indexes-{}-{}",
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

fn source_component(name: &str, root: &str, base: usize, fields: &str) -> String {
    format!(
        r#"<component name="{name}" library="text" kind="16"><data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" outkey="{base}">
<entry name="B" outkey="{}"/><entry name="A" outkey="{}"/>
</entry></entry></entry></root>
<text type="csv" inputinstance="input.csv"><settings separator="," firstrownames="false">
<names root="{root}" block="Rows">{fields}</names></settings></text>
</data></component>"#,
        base + 2,
        base + 1,
    )
}

fn design(fields: &str, extra: &str) -> String {
    let source = source_component("source", "Input", 10, fields);
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>{source}{extra}
<component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="20">
<entry name="B" inpkey="22"/><entry name="A" inpkey="21"/>
</entry></entry></entry></root>
<text type="csv" outputinstance="output.csv"><settings separator="," firstrownames="false">
<names root="Output" block="Rows">{fields}</names></settings></text>
</data></component></children><graph><vertices>
<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
<vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
    )
}

fn execute(project: &Project) -> Result<String, Box<dyn Error>> {
    assert_eq!(project.source_options.has_header_row, Some(false));
    assert_eq!(project.target_options.has_header_row, Some(false));
    assert!(engine::validate(project).is_empty());
    let rows = format_csv::from_str("alpha,7\nbeta,11\n", &project.source, None, false)?;
    assert_eq!(
        rows[0].field("A").and_then(Instance::as_scalar),
        Some(&Value::String("alpha".into()))
    );
    assert_eq!(
        rows[0].field("B").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    let output = engine::run(project, &Instance::Repeated(rows))?;
    Ok(format_csv::to_string(
        &project.target,
        output.as_repeated().ok_or("expected CSV target rows")?,
        None,
        false,
    )?)
}

#[test]
fn shuffled_declarations_execute_in_numeric_order_through_two_strict_cycles()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("shuffled.mfd");
    std::fs::write(
        &path,
        design(
            "<field1 name=\"B\" type=\"integer\"/><field0 name=\"A\" type=\"string\"/>",
            "",
        ),
    )?;
    let imported = mfd::import(&path)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut project = imported.project;
    let SchemaKind::Group { children, .. } = &project.source.kind else {
        return Err("CSV source must be a group".into());
    };
    assert_eq!(children[0].name, "A");
    assert_eq!(children[1].name, "B");
    assert_eq!(
        children[1].kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    );
    let source_schema = project.source.clone();
    let target_schema = project.target.clone();
    for cycle in 0..2 {
        assert_eq!(execute(&project)?, "alpha,7\nbeta,11\n");
        let exported = temp.0.join(format!("strict-{cycle}.mfd"));
        let report = mfd::export_with_profile(&project, &exported, mfd::ExportProfile::NativeMfd)?;
        assert!(report.is_native_compatible());
        assert!(report.warnings.is_empty());
        let reimported = mfd::import(&exported)?;
        assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
        project = reimported.project;
        assert_eq!(project.source, source_schema);
        assert_eq!(project.target, target_schema);
    }
    assert_eq!(execute(&project)?, "alpha,7\nbeta,11\n");
    Ok(())
}

#[test]
fn invalid_declarations_skip_the_component_with_actionable_diagnostics()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    for (index, (fields, reason)) in [
        ("<field name=\"A\"/>", "valid numeric suffix"),
        ("<fieldword name=\"A\"/>", "valid numeric suffix"),
        (
            "<field999999999999999999999999 name=\"A\"/>",
            "valid numeric suffix",
        ),
        (
            "<field0 name=\"A\"/><field0 name=\"B\"/>",
            "duplicate field declaration indexes",
        ),
        (
            "<field0 name=\"A\"/><field00 name=\"B\"/>",
            "duplicate field declaration indexes",
        ),
        ("<field0 name=\"\"/>", "empty or duplicate field name"),
        ("<field0/>", "empty or duplicate field name"),
        (
            "<field0 name=\"A\"/><field1 name=\"A\"/>",
            "empty or duplicate field name",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let path = temp.0.join(format!("invalid-{index}.mfd"));
        std::fs::write(
            &path,
            design(FIELDS, &source_component("broken", "Broken", 100, fields)),
        )?;
        let imported = mfd::import(&path)?;
        assert_eq!(imported.project.source.name, "Input");
        assert!(imported.project.extra_sources.is_empty());
        let warnings = imported
            .warnings
            .iter()
            .filter(|warning| {
                warning.contains("csv component `broken`") && warning.contains(reason)
            })
            .collect::<Vec<_>>();
        assert_eq!(warnings.len(), 1, "{:?}", imported.warnings);
        assert!(warnings[0].contains(reason), "{}", warnings[0]);
        assert!(warnings[0].contains("skipped"));
        assert!(matches!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            ),
            Err(mfd::MfdError::IncompatibleImport(report)) if !report.executable
        ));
        assert_eq!(execute(&imported.project)?, "alpha,7\nbeta,11\n");
    }
    Ok(())
}

#[test]
fn noncontiguous_numeric_indexes_follow_the_fixed_width_convention() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("gaps.mfd");
    std::fs::write(
        &path,
        design(
            "<field7 name=\"B\" type=\"integer\"/><field2 name=\"A\" type=\"string\"/>",
            "",
        ),
    )?;
    let imported = mfd::import(&path)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(execute(&imported.project)?, "alpha,7\nbeta,11\n");
    Ok(())
}
