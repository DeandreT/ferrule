use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_csv_quote_{}_{}",
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

#[test]
fn native_csv_quote_setting_roundtrips_and_executes_locally()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let design = temp.0.join("custom-quote.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="text" kind="16"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Rows" outkey="10"><entry name="Name" outkey="11"/><entry name="Age" outkey="12"/></entry></entry></entry></root>
    <text type="csv" inputinstance="source.csv"><settings separator="," quote="'" firstrownames="true"><names root="People" block="Rows"><field0 name="Name" type="string"/><field1 name="Age" type="integer"/></names></settings></text>
  </data></component>
  <component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="20"><entry name="Name" inpkey="21"/><entry name="Age" inpkey="22"/></entry></entry></entry></root>
    <text type="csv" outputinstance="target.csv"><settings separator="," quote="'" firstrownames="true"><names root="People" block="Rows"><field0 name="Name" type="string"/><field1 name="Age" type="integer"/></names></settings></text>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut project = imported.project;
    assert_eq!(project.source_options.csv_quote, Some('\''));
    assert_eq!(project.target_options.csv_quote, Some('\''));
    assert!(engine::validate(&project).is_empty());

    let source_text = "Name,Age\n'O''Neil, Jr.',29\n";
    let rows = format_csv::from_str_with_quote(
        source_text,
        &project.source,
        project.source_options.delimiter,
        project.source_options.csv_quote,
        true,
    )?;
    assert_eq!(
        rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("O'Neil, Jr.".into()))
    );
    let output = engine::run(&project, &Instance::Repeated(rows))?;
    let output_text = format_csv::to_string_with_quote(
        &project.target,
        output
            .as_repeated()
            .ok_or("CSV target did not produce rows")?,
        project.target_options.delimiter,
        project.target_options.csv_quote,
        true,
    )?;
    assert_eq!(output_text, source_text);

    let exported = temp.0.join("roundtrip.mfd");
    let report = mfd::export_with_profile(&project, &exported, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible());
    assert_eq!(
        std::fs::read_to_string(&exported)?
            .matches("quote=\"'\"")
            .count(),
        2
    );
    let reimported = mfd::import(&exported)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source_options.csv_quote, Some('\''));
    assert_eq!(reimported.project.target_options.csv_quote, Some('\''));
    for (quote, encoded) in [('&', "&amp;"), ('<', "&lt;")] {
        project.source_options.csv_quote = Some(quote);
        project.target_options.csv_quote = Some(quote);
        let escaped_design = temp.0.join(format!("escaped-{}.mfd", quote as u32));
        mfd::export_with_profile(&project, &escaped_design, mfd::ExportProfile::NativeMfd)?;
        let xml = std::fs::read_to_string(&escaped_design)?;
        assert_eq!(xml.matches(&format!("quote=\"{encoded}\"")).count(), 2);
        let reimported = mfd::import(&escaped_design)?;
        assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
        assert_eq!(reimported.project.source_options.csv_quote, Some(quote));
        assert_eq!(reimported.project.target_options.csv_quote, Some(quote));
    }
    project.source_options.csv_quote = None;
    project.target_options.csv_quote = None;
    project.source_options.csv_quote_disabled = true;
    project.target_options.csv_quote_disabled = true;
    assert!(engine::validate(&project).is_empty());
    let raw_quote_text = "Name,Age\n\"Jane\",29\n";
    let rows = format_csv::from_str_with_dialect(
        raw_quote_text,
        &project.source,
        project.source_options.delimiter,
        project.source_options.csv_quote,
        project.source_options.csv_quote_disabled,
        true,
    )?;
    let output = engine::run(&project, &Instance::Repeated(rows))?;
    assert_eq!(
        format_csv::to_string_with_dialect(
            &project.target,
            output
                .as_repeated()
                .ok_or("CSV target did not produce rows")?,
            project.target_options.delimiter,
            project.target_options.csv_quote,
            project.target_options.csv_quote_disabled,
            true,
        )?,
        raw_quote_text
    );
    let no_quote_design = temp.0.join("no-quote.mfd");
    mfd::export_with_profile(&project, &no_quote_design, mfd::ExportProfile::NativeMfd)?;
    assert_eq!(
        std::fs::read_to_string(&no_quote_design)?
            .matches("quote=\"\"")
            .count(),
        2
    );
    let reimported = mfd::import(&no_quote_design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(reimported.project.source_options.csv_quote_disabled);
    assert!(reimported.project.target_options.csv_quote_disabled);
    project.source_options.csv_quote = Some('\'');
    assert!(mfd::preflight_export(&project, &temp.0.join("conflicting.mfd")).is_err());
    project.source_options.csv_quote = None;

    let invalid = temp.0.join("invalid-quote.mfd");
    let original = std::fs::read_to_string(&design)?;
    std::fs::write(&invalid, original.replace("quote=\"'\"", "quote=\"||\""))?;
    let best_effort = mfd::import(&invalid)?;
    assert!(
        best_effort
            .warnings
            .iter()
            .any(|warning| warning.contains("quote setting"))
    );
    assert!(
        mfd::import_with_profile(
            &invalid,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .is_err()
    );
    Ok(())
}
