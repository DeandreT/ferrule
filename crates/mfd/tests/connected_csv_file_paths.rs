use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-csv-file-paths-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn design(source_wrapper: &str, target_wrapper: &str, edges: &str) -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
<component name="path" library="core" kind="6"><targets><datapoint key="1"/></targets><data><input datatype="string" required="1"/><parameter usageKind="input" name="FilePath"/></data></component>
<component name="source" library="text" kind="16"><data>
<root><entry name="FileInstance" {source_wrapper}><entry name="document"><entry name="Rows" outkey="10"><entry name="Value" outkey="11"/></entry></entry></entry></root>
<text type="csv" inputinstance="preview.csv"><settings separator="," firstrownames="true"><names root="Input" block="Rows"><field0 name="Value" type="string"/></names></settings></text>
</data></component>
<component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
<root><entry name="FileInstance" {target_wrapper}><entry name="document"><entry name="Rows" inpkey="20"><entry name="Value" inpkey="21"/></entry></entry></entry></root>
<text type="csv" outputinstance="result.csv"><settings separator="|" firstrownames="false"><names root="Output" block="Rows"><field0 name="Value" type="string"/></names></settings></text>
</data></component>
</children><graph><vertices>
<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
{edges}
</vertices></graph></structure></component></mapping>"#
    )
}

#[test]
fn connected_csv_filename_inputs_remain_warned_partial_imports() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    for (component, source, target, port) in [
        ("source", r#"inpkey="13""#, "", "13"),
        ("target", "", r#"inpkey="23""#, "23"),
    ] {
        let path = directory.0.join(format!("{component}.mfd"));
        let xml = design(
            source,
            target,
            &format!(r#"<vertex vertexkey="1"><edges><edge vertexkey="{port}"/></edges></vertex>"#),
        );
        std::fs::write(&path, &xml)?;
        let imported = mfd::import(&path)?;
        let expected = format!(
            "CSV component `{component}` has connected FileInstance input port `{port}`; filename-driven documents are unsupported"
        );
        assert!(
            imported.warnings.contains(&expected),
            "{:?}",
            imported.warnings
        );
        assert_eq!(imported.project.root.bindings.len(), 1);
        assert!(engine::validate(&imported.project).is_empty());
        let error = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .err()
        .ok_or("connected filename was admitted")?;
        assert!(error.to_string().contains(&expected));
        assert_eq!(std::fs::read_to_string(&path)?, xml);
    }
    Ok(())
}

#[test]
fn connected_csv_filename_outputs_are_not_ordinary_value_ports() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    for (component, source, target, port) in [
        ("source", r#"outkey="14""#, "", "14"),
        ("target", "", r#"outkey="24""#, "24"),
    ] {
        let path = directory.0.join(format!("{component}.mfd"));
        let xml = design(source, target, "").replace(
            r#"vertex vertexkey="11""#,
            &format!(r#"vertex vertexkey="{port}""#),
        );
        std::fs::write(&path, xml)?;
        let imported = mfd::import(&path)?;
        let expected = format!(
            "CSV component `{component}` has connected FileInstance output port `{port}`; filename-driven documents are unsupported"
        );
        assert!(
            imported.warnings.contains(&expected),
            "{:?}",
            imported.warnings
        );
        assert!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn unconnected_csv_wrappers_and_fileinstance_named_fields_stay_executable()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let xml = design(
        r#"inpkey="13" outkey="14""#,
        r#"inpkey="23" outkey="24""#,
        "",
    );
    for (name, xml) in [
        ("unused-wrapper-ports", xml.clone()),
        (
            "ordinary-field-name",
            xml.replace("name=\"Value\"", "name=\"FileInstance\""),
        ),
    ] {
        let path = directory.0.join(format!("{name}.mfd"));
        std::fs::write(&path, xml)?;
        let imported = mfd::import(&path)?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )
            .is_ok()
        );
        assert_eq!(imported.project.target_path.as_deref(), Some("result.csv"));
    }
    Ok(())
}
