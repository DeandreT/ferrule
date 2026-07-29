use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_protobuf_package_{}_{}",
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

fn write_package(package: &Path, schema_reference: &str) -> Result<PathBuf, std::io::Error> {
    let maps = package.join("maps/orders");
    let api = package.join("schemas/api");
    let shared = package.join("schemas/shared");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&api)?;
    std::fs::create_dir_all(&shared)?;
    std::fs::write(
        api.join("root.proto"),
        r#"syntax = "proto3";
package ferrule.package;
import "schemas/shared/value.proto";
message Message {
  shared.Value value = 1;
}
"#,
    )?;
    std::fs::write(
        shared.join("value.proto"),
        r#"syntax = "proto3";
package shared;
message Value {
  string text = 1;
}
"#,
    )?;
    std::fs::write(
        package.join("schemas/result.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Text" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let mapping = maps.join("mapping.mfd");
    let design = r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="32">
  <resources/>
  <component name="defaultmap" uid="1">
    <properties SelectedLanguage="builtin"/>
    <structure><children>
      <component name="Message" library="binary" uid="2" kind="33">
        <data><root>
          <entry name="FileInstance"><entry name="document" type="doc-protobuf">
            <document schemafile="SCHEMA_REFERENCE" root="{ferrule.package}Message"/>
            <entry name="Message"><entry name="value"><entry name="text" outkey="11"/></entry></entry>
          </entry></entry>
        </root><binary inputinstance="message.bin"/></data>
      </component>
      <component name="Output" library="xml" uid="3" kind="14">
        <properties XSLTDefaultOutput="1"/>
        <data><root>
          <entry name="FileInstance"><entry name="document">
            <entry name="Output"><entry name="Text" inpkey="21"/></entry>
          </entry></entry>
        </root><document schema="..\..\schemas\result.xsd" outputinstance="result.xml" instanceroot="{}Output"/></data>
      </component>
    </children></structure>
    <connections><edge from="11" to="21"/></connections>
  </component>
</mapping>
"#
    .replace("SCHEMA_REFERENCE", schema_reference);
    std::fs::write(&mapping, design)?;
    Ok(mapping)
}

fn embedded_layout(options: &mapping::ProtobufOptions) -> format_protobuf::Layout {
    format_protobuf::Layout::parse_files(
        options.schema_path.as_deref().unwrap_or("root.proto"),
        &options.schema,
        options
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )
    .unwrap_or_else(|error| panic!("embedded protobuf graph should parse: {error}"))
}

#[test]
fn relocated_package_resolves_windows_parent_path_and_embeds_import_graph()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let original = directory.0.join("original");
    let mapping = write_package(&original, r"..\..\schemas\api\root.proto")?;
    let relocated = directory.0.join("relocated");
    std::fs::rename(&original, &relocated)?;
    let relocated_mapping = relocated.join(mapping.strip_prefix(&original)?);
    let options = mfd::ImportOptions::default().with_package_root(&relocated);

    let imported = mfd::import_with_options(&relocated_mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let protobuf = imported
        .project
        .source_options
        .protobuf
        .as_ref()
        .ok_or("protobuf options were not embedded")?;
    assert_eq!(
        protobuf.schema_path.as_deref(),
        Some("schemas/api/root.proto")
    );
    assert_eq!(
        protobuf
            .imports
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["schemas/shared/value.proto"]
    );

    std::fs::remove_dir_all(relocated.join("schemas"))?;
    let layout = embedded_layout(protobuf);
    let source = Instance::Group(vec![(
        "value".into(),
        Instance::Group(vec![(
            "text".into(),
            Instance::Scalar(Value::String("portable".into())),
        )]),
    )]);
    let bytes = format_protobuf::to_vec(&layout, &protobuf.root_message, &source)?;
    let decoded = format_protobuf::from_slice(&layout, &protobuf.root_message, &bytes)?;
    let output = engine::run(&imported.project, &decoded)?;
    assert_eq!(
        output.field("Text").and_then(Instance::as_scalar),
        Some(&Value::String("portable".into()))
    );
    Ok(())
}

#[test]
fn protobuf_root_cannot_traverse_above_selected_package() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = TempDir::new()?;
    let package = directory.0.join("package");
    let mapping = write_package(&package, r"..\..\..\outside\root.proto")?;
    let options = mfd::ImportOptions::default().with_package_root(&package);

    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.project.source_options.protobuf.is_none());
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("traverses above package root")
            && warning.contains("without executable protobuf metadata")
    }));
    Ok(())
}

#[cfg(unix)]
#[test]
fn protobuf_import_symlink_cannot_escape_selected_package() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::symlink;

    let directory = TempDir::new()?;
    let package = directory.0.join("package");
    let mapping = write_package(&package, r"..\..\schemas\api\root.proto")?;
    let outside = directory.0.join("outside-value.proto");
    std::fs::write(
        &outside,
        r#"syntax = "proto3"; package shared; message Value { string text = 1; }"#,
    )?;
    let imported_schema = package.join("schemas/shared/value.proto");
    std::fs::remove_file(&imported_schema)?;
    symlink(&outside, &imported_schema)?;
    let options = mfd::ImportOptions::default().with_package_root(&package);

    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.project.source_options.protobuf.is_none());
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("escapes its configured base")
            && warning.contains("without executable protobuf metadata")
    }));
    Ok(())
}
