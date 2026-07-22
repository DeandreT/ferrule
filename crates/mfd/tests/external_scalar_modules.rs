use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_external_scalar_{label}_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

#[test]
fn imports_csharp_invariant_number_formatter() -> Result<(), Box<dyn Error>> {
    exercise_formatter(
        "csharp",
        "cs",
        "Finance, Version=1.0.0.0, Culture=neutral, PublicKeyToken=null",
        "Money.Render",
        "Finance/Money.cs",
        r##"using System.Globalization;
public class Money {
    public static string Render(decimal amount) {
        return amount.ToString("#,##0.00", CultureInfo.InvariantCulture);
    }
}"##,
    )
}

#[test]
fn imports_java_decimal_format_wrapper() -> Result<(), Box<dyn Error>> {
    exercise_formatter(
        "java",
        "java",
        "demo.PriceText",
        "demo.PriceText.Render",
        "demo/PriceText.java",
        r##"package demo;
public class PriceText {
    public static String Render(java.math.BigDecimal amount) {
        java.text.NumberFormat formatter = new java.text.DecimalFormat("#,##0.00");
        return formatter.format(amount.doubleValue());
    }
}"##,
    )
}

fn exercise_formatter(
    label: &str,
    language: &str,
    library: &str,
    function: &str,
    module_path: &str,
    module: &str,
) -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new(label)?;
    write_common_schemas(&dir.0, "string")?;
    write(&dir.0.join(module_path), module)?;
    write(
        &dir.0.join("mapping.mfd"),
        &mapping(language, library, function),
    )?;

    let input = input(Value::Float(1234.5));
    let imported = import_clean(&dir.0.join("mapping.mfd"))?;
    assert_value(
        &engine::run(&imported.project, &input)?,
        &Value::String("1,234.50".into()),
    );
    roundtrip(
        &dir.0,
        &imported.project,
        &input,
        &Value::String("1,234.50".into()),
    )
}

#[test]
fn imports_xquery_arithmetic_function() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new("xquery")?;
    write_common_schemas(&dir.0, "decimal")?;
    write(
        &dir.0.join("fees.xq"),
        r#"xquery version "1.0";
module namespace fee="urn:ferrule:test";
declare function fee:calculate($amount as xs:decimal) as xs:decimal {
    $amount * 0.15
};"#,
    )?;
    write(
        &dir.0.join("mapping.mfd"),
        &mapping("xquery", "fees", "fee:calculate"),
    )?;

    let input = input(Value::Int(200));
    let imported = import_clean(&dir.0.join("mapping.mfd"))?;
    assert_value(
        &engine::run(&imported.project, &input)?,
        &Value::Float(30.0),
    );
    roundtrip(&dir.0, &imported.project, &input, &Value::Float(30.0))
}

fn write_common_schemas(dir: &Path, result_type: &str) -> Result<(), std::io::Error> {
    write(
        &dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Amount" type="xs:decimal"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &dir.join("target.xsd"),
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Report"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:{result_type}"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        ),
    )
}

fn mapping(language: &str, library: &str, function: &str) -> String {
    format!(
        r#"<mapping><component name="map"><properties SelectedLanguage="{language}"/><structure><children>
  <component name="source" library="xml" kind="14"><data><root>
    <entry name="Source"><entry name="Row" outkey="10"><entry name="Amount" outkey="11"/></entry></entry>
  </root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Source"/></data></component>
  <component name="{function}" library="{library}" kind="5">
    <sources><datapoint pos="0" key="20"/></sources><targets><datapoint pos="0" key="21"/></targets>
  </component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
    <entry name="Report"><entry name="Row" inpkey="30"><entry name="Value" inpkey="31"/></entry></entry>
  </root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Report"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="31"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
    )
}

fn input(value: Value) -> Instance {
    Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![Instance::Group(vec![(
            "Amount".into(),
            Instance::Scalar(value),
        )])]),
    )])
}

fn import_clean(path: &Path) -> Result<mfd::Imported, Box<dyn Error>> {
    let imported = mfd::import(path)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    Ok(imported)
}

fn assert_value(output: &Instance, expected: &Value) {
    let value = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .and_then(|rows| rows.first())
        .and_then(|row| row.field("Value"))
        .and_then(Instance::as_scalar);
    assert_eq!(value, Some(expected));
}

fn roundtrip(
    dir: &Path,
    project: &mapping::Project,
    input: &Instance,
    expected: &Value,
) -> Result<(), Box<dyn Error>> {
    let path = dir.join("roundtrip.mfd");
    let warnings = mfd::export(project, &path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let imported = import_clean(&path)?;
    assert_value(&engine::run(&imported.project, input)?, expected);
    Ok(())
}
