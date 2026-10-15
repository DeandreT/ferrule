use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_cloned_json_occurrence_{}_{}",
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

fn write_fixture(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::write(
        dir.join("source.schema.json"),
        r#"{"type":"object","properties":{"First":{"type":"array","items":{"type":"object","properties":{"Value":{"type":"string"}},"required":["Value"],"additionalProperties":false}},"Second":{"type":"array","items":{"type":"object","properties":{"Value":{"type":"string"},"Alternate":{"type":"string"}},"required":["Value","Alternate"],"additionalProperties":false}}},"required":["First","Second"],"additionalProperties":false}"#,
    )?;
    std::fs::write(
        dir.join("target.schema.json"),
        r#"{"type":"object","properties":{"Rows":{"type":"array","items":{"type":"object","properties":{"Value":{"type":"string"}},"required":["Value"],"additionalProperties":false}}},"required":["Rows"],"additionalProperties":false}"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">
      <entry name="First" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="10"><entry name="Value" type="json-property"><entry name="string" outkey="11"/></entry></entry></entry></entry></entry>
      <entry name="Second" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="20"><entry name="Value" type="json-property"><entry name="string" outkey="21"/></entry><entry name="Alternate" type="json-property"><entry name="string" outkey="22"/></entry></entry></entry></entry></entry>
    </entry></entry></entry></entry></root><json schema="source.schema.json" inputinstance="source.json"/>
  </data></component>
  <component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object"><entry name="Rows" type="json-property"><entry name="array"><entry name="item" type="json-item">
      <entry name="object" inpkey="30"><entry name="Value" type="json-property"><entry name="string" inpkey="31"/></entry></entry>
      <entry name="object" clone="1" inpkey="40"><entry name="Value" type="json-property"><entry name="string" inpkey="41"/></entry></entry>
      <entry name="object" clone="1" inpkey="50"><entry name="Value" type="json-property"><entry name="string" inpkey="51"/></entry></entry>
    </entry></entry></entry></entry></entry></entry></entry></root><json schema="target.schema.json" outputinstance="target.json"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="40"/><edge vertexkey="50"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="51"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn verify_occurrences(reverse_feed_order: bool) -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let design = write_fixture(&dir.0)?;
    if reverse_feed_order {
        let mut text = std::fs::read_to_string(&design)?;
        for (original, reversed) in [(10, 200), (11, 201), (20, 100), (21, 101), (22, 102)] {
            for attribute in ["outkey", "vertexkey"] {
                text = text.replace(
                    &format!("{attribute}=\"{original}\""),
                    &format!("{attribute}=\"{reversed}\""),
                );
            }
        }
        std::fs::write(&design, text)?;
    }
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let rows_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Rows")
        .ok_or("missing Rows scope")?;
    let segments = rows_scope
        .concatenated()
        .ok_or("JSON occurrence branches were not concatenated")?
        .iter()
        .collect::<Vec<_>>();
    assert_eq!(segments.len(), 3);
    for (segment, source) in segments.iter().zip(["First", "Second", "Second"]) {
        assert_eq!(
            segment.source().expect("physical source selector"),
            Some([source.to_string()].as_slice())
        );
        assert_eq!(segment.bindings.len(), 1);
        assert_eq!(segment.bindings[0].target_field, "Value");
    }

    let cases: [(&str, &[&str]); 3] = [
        (
            r#"{"First":[{"Value":"A1"},{"Value":"A2"}],"Second":[{"Value":"B1","Alternate":"C1"}]}"#,
            &["A1", "A2", "B1", "C1"],
        ),
        (
            r#"{"First":[],"Second":[{"Value":"B1","Alternate":"C1"},{"Value":"B2","Alternate":"C2"}]}"#,
            &["B1", "B2", "C1", "C2"],
        ),
        (r#"{"First":[{"Value":"A1"}],"Second":[]}"#, &["A1"]),
    ];
    for (document, expected) in cases {
        let input = format_json::from_str(document, &imported.project.source)?;
        let output = engine::run(&imported.project, &input)?;
        let rows = output
            .field("Rows")
            .and_then(Instance::as_repeated)
            .ok_or("Rows output is not repeated")?;
        let actual = rows
            .iter()
            .map(|row| row.field("Value").and_then(Instance::as_scalar).cloned())
            .collect::<Vec<_>>();
        let expected = expected
            .iter()
            .map(|value| Some(Value::String((*value).into())))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "input={document}; output={output:?}");
    }

    let program = codegen::lower(&imported.project)
        .map_err(|diagnostics| format!("codegen lowering: {diagnostics:?}"))?;
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-cloned-json-occurrences".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .canonicalize()?
                    .display()
                    .to_string(),
            ),
        },
    )?;
    let csharp = codegen_csharp::emit(&program)?;
    if let Some(root) = std::env::var_os("FERRULE_CLONED_JSON_ARTIFACT_ROOT") {
        let output = PathBuf::from(root).join(if reverse_feed_order {
            "reversed"
        } else {
            "ordinary"
        });
        std::fs::create_dir(&output)?;
        write_artifacts(&output.join("rust"), &rust)?;
        write_artifacts(&output.join("csharp"), &csharp)?;
    }
    Ok(())
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) -> std::io::Result<()> {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.contents)?;
    }
    Ok(())
}

#[test]
fn cloned_json_occurrences_keep_branch_bindings_and_source_order() -> Result<(), Box<dyn Error>> {
    verify_occurrences(false)
}

#[test]
fn cloned_json_occurrences_keep_order_with_reversed_feed_keys() -> Result<(), Box<dyn Error>> {
    verify_occurrences(true)
}
