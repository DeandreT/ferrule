use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Node, RuntimeValue};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_path_functions_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_design(path: &Path) {
    std::fs::write(
        path,
        r#"<mapping version="26">
  <resources/>
  <component name="map"><structure><children>
    <component name="source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Relative" outkey="10"/>
      </entry></entry></entry></root>
      <document inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="mfd-filepath" library="core" kind="5">
      <targets><datapoint pos="0" key="20"/></targets>
    </component>
    <component name="main-mfd-filepath" library="core" kind="5">
      <targets><datapoint pos="0" key="21"/></targets>
    </component>
    <component name="get-folder" library="core" kind="5">
      <sources><datapoint pos="0" key="30"/></sources>
      <targets><datapoint pos="0" key="31"/></targets>
    </component>
    <component name="remove-folder" library="core" kind="5">
      <sources><datapoint pos="0" key="32"/></sources>
      <targets><datapoint pos="0" key="33"/></targets>
    </component>
    <component name="resolve-filepath" library="core" kind="5">
      <sources><datapoint pos="0" key="34"/><datapoint pos="1" key="35"/></sources>
      <targets><datapoint pos="0" key="36"/></targets>
    </component>
    <component name="target" library="xml" kind="14">
      <properties XSLTDefaultOutput="1"/>
      <data>
        <root><entry name="FileInstance"><entry name="document"><entry name="Target">
          <entry name="MfdPath" inpkey="40"/>
          <entry name="MainPath" inpkey="41"/>
          <entry name="Folder" inpkey="42"/>
          <entry name="Name" inpkey="43"/>
          <entry name="Resolved" inpkey="44"/>
        </entry></entry></entry></root>
        <document outputinstance="target.xml" instanceroot="{}Target"/>
      </data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="20"><edges><edge vertexkey="40"/><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="21"><edges><edge vertexkey="41"/></edges></vertex>
    <vertex vertexkey="31"><edges><edge vertexkey="42"/><edge vertexkey="34"/></edges></vertex>
    <vertex vertexkey="10"><edges><edge vertexkey="32"/><edge vertexkey="35"/></edges></vertex>
    <vertex vertexkey="33"><edges><edge vertexkey="43"/></edges></vertex>
    <vertex vertexkey="36"><edges><edge vertexkey="44"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )
    .unwrap();
}

fn binding_node<'a>(project: &'a mapping::Project, field: &str) -> &'a Node {
    let binding = project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == field)
        .unwrap();
    project.graph.nodes.get(&binding.node).unwrap()
}

fn source(relative: &str) -> Instance {
    Instance::Group(vec![(
        "Relative".to_string(),
        Instance::Scalar(Value::String(relative.to_string())),
    )])
}

fn assert_output(output: &Instance) {
    for (field, expected) in [
        ("MfdPath", "/maps/library.mfd"),
        ("MainPath", "/maps/main.mfd"),
        ("Folder", "/maps/"),
        ("Name", "report.xml"),
        ("Resolved", "/report.xml"),
    ] {
        assert_eq!(
            output.field(field).and_then(Instance::as_scalar),
            Some(&Value::String(expected.to_string())),
            "field {field}"
        );
    }
}

#[test]
fn path_functions_import_execute_and_round_trip() {
    let dir = TempDir::new();
    let design = dir.0.join("path-functions.mfd");
    write_design(&design);

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        binding_node(&imported.project, "MfdPath"),
        Node::RuntimeValue {
            value: RuntimeValue::MappingFilePath
        }
    ));
    assert!(matches!(
        binding_node(&imported.project, "MainPath"),
        Node::RuntimeValue {
            value: RuntimeValue::MainMappingFilePath
        }
    ));
    for (field, expected_function) in [
        ("Folder", "get_folder"),
        ("Name", "remove_folder"),
        ("Resolved", "resolve_filepath"),
    ] {
        assert!(matches!(
            binding_node(&imported.project, field),
            Node::Call { function, .. } if function == expected_function
        ));
    }

    let execution = engine::ExecutionContext::with_main_mapping_file_path(
        Path::new("/maps/library.mfd"),
        Path::new("/maps/main.mfd"),
    );
    let output =
        engine::run_with_context(&imported.project, &source("../report.xml"), &execution).unwrap();
    assert_output(&output);

    let exported = dir.0.join("round-trip.mfd");
    let warnings = mfd::export(&imported.project, &exported).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let output =
        engine::run_with_context(&reimported.project, &source("../report.xml"), &execution)
            .unwrap();
    assert_output(&output);
}
