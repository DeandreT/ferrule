use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaKind};

#[test]
fn import_mfd_help_lists_repeatable_json_schema_roots() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--help"])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("--package-manifest <FILE>"), "{stdout}");
    assert!(stdout.contains("--json-schema-root <DIR>"), "{stdout}");
    assert!(
        stdout.contains("Repeat to search multiple catalogs"),
        "{stdout}"
    );
    Ok(())
}

#[test]
fn package_manifest_resolves_json_schema_and_rebases_instance_paths() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    let maps = directory.path().join("maps");
    let catalog = directory.path().join("resources/json");
    std::fs::create_dir_all(&maps)?;
    write_schema(&catalog, "integer")?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping)?;
    let manifest = directory.path().join("ferrule-package.json");
    std::fs::write(
        &manifest,
        r#"{"schemaVersion":1,"kind":"ferrule.mapping-package",
            "catalogs":[{"kind":"json-schema","root":"resources/json"}]}"#,
    )?;
    let output = directory.path().join("projects/project.json");
    std::fs::create_dir_all(output.parent().ok_or("project has no parent")?)?;

    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--mfd"])
        .arg(&mapping)
        .args(["--out"])
        .arg(&output)
        .args(["--package-manifest"])
        .arg(&manifest)
        .output()?;

    assert_success(&result);
    assert_source_type(&output, ScalarType::Int)?;
    let project: mapping::Project = serde_json::from_slice(&std::fs::read(output)?)?;
    assert_eq!(project.source_path.as_deref(), Some("../maps/input.json"));
    assert_eq!(project.target_path.as_deref(), Some("../maps/output.xml"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_mfd_rebases_from_the_canonical_design_directory() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new()?;
    let package = directory.path().join("package");
    let maps = package.join("maps");
    let alias = directory.path().join("alias");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&alias)?;
    write_schema(&package, "integer")?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping)?;
    let linked_mapping = alias.join("mapping.mfd");
    symlink(&mapping, &linked_mapping)?;
    let manifest = package.join("ferrule-package.json");
    std::fs::write(
        &manifest,
        r#"{"schemaVersion":1,"kind":"ferrule.mapping-package"}"#,
    )?;
    let output = package.join("projects/project.json");
    std::fs::create_dir_all(output.parent().ok_or("project has no parent")?)?;

    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--mfd"])
        .arg(&linked_mapping)
        .args(["--out"])
        .arg(&output)
        .args(["--package-manifest"])
        .arg(&manifest)
        .output()?;

    assert_success(&result);
    let project: mapping::Project = serde_json::from_slice(&std::fs::read(output)?)?;
    assert_eq!(project.source_path.as_deref(), Some("../maps/input.json"));
    assert_eq!(project.target_path.as_deref(), Some("../maps/output.xml"));
    Ok(())
}

#[test]
fn import_mfd_searches_repeated_json_schema_roots_in_order() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let maps = directory.path().join("maps");
    let empty = directory.path().join("empty");
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&empty)?;
    write_schema(&first, "integer")?;
    write_schema(&second, "string")?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping)?;

    let fallback_output = directory.path().join("fallback.json");
    let fallback = run_import(&mapping, &fallback_output, [&empty, &second])?;
    assert_success(&fallback);
    assert_source_type(&fallback_output, ScalarType::String)?;

    let ordered_output = directory.path().join("ordered.json");
    let ordered = run_import(&mapping, &ordered_output, [&first, &second])?;
    assert_success(&ordered);
    assert_source_type(&ordered_output, ScalarType::Int)?;
    Ok(())
}

fn run_import(
    mapping: &Path,
    output: &Path,
    roots: [&PathBuf; 2],
) -> Result<Output, std::io::Error> {
    Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--mfd"])
        .arg(mapping)
        .args(["--out"])
        .arg(output)
        .args(["--json-schema-root"])
        .arg(roots[0])
        .args(["--json-schema-root"])
        .arg(roots[1])
        .output()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("(0 warning(s))"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_source_type(path: &Path, expected: ScalarType) -> Result<(), Box<dyn Error>> {
    let project: mapping::Project = serde_json::from_slice(&std::fs::read(path)?)?;
    let code = project
        .source
        .child("Code")
        .ok_or("catalog schema should define Code")?;
    assert!(matches!(
        &code.kind,
        SchemaKind::Scalar { ty } if *ty == expected
    ));
    assert!(engine::validate(&project).is_empty());
    Ok(())
}

fn write_schema(root: &Path, code_type: &str) -> Result<(), std::io::Error> {
    let shared = root.join("shared");
    std::fs::create_dir_all(&shared)?;
    std::fs::write(
        shared.join("source.schema.json"),
        format!(
            r#"{{
  "title":"Envelope",
  "type":"object",
  "properties":{{"Code":{{"type":"{code_type}"}}}},
  "required":["Code"],
  "additionalProperties":false
}}"#
        ),
    )
}

fn write_mapping(path: &Path) -> Result<(), std::io::Error> {
    std::fs::write(
        path,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root">
      <entry name="object"><entry name="Code" type="json-property">
        <entry name="value" outkey="10"/>
      </entry></entry>
    </entry></entry></entry></root>
    <json schema="..\shared\source.schema.json" inputinstance="input.json"/>
  </data></component>
  <component name="target" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Output"><entry name="Code" inpkey="20"/></entry></root>
      <document outputinstance="output.xml" instanceroot="{}Output"/>
    </data>
  </component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_json_schema_catalog_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
