use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{Program, TargetConstruction, TargetScope};
use ir::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
    ItemCountRange, JsonAllowedValue, JsonAllowedValues, JsonContainsConstraint,
    JsonContainsConstraints, JsonContainsPredicate, JsonDependentSchemaConstraint,
    JsonDependentSchemaConstraints, JsonSchemaPredicate, NumberBound, NumberRange, NumericRange,
    ScalarType, SchemaNode,
};

#[test]
fn rust_encoded_float_metadata_executes_in_generated_csharp()
-> Result<(), Box<dyn std::error::Error>> {
    let low = FiniteF64::new(f64::from_bits(0x0031_fa18_2c40_c60d)).unwrap();
    let high = FiniteF64::new(f64::from_bits(0x0031_fa18_2c40_c60e)).unwrap();
    let exact_low = || -> Result<SchemaNode, &'static str> {
        let range = NumberRange::new(
            Some(NumberBound::inclusive(low)),
            Some(NumberBound::inclusive(low)),
        )
        .ok_or("low float range is valid")?;
        SchemaNode::scalar("Value", ScalarType::Float)
            .with_numeric_range(NumericRange::Number(range))
            .ok_or("float scalar accepts range")
    };
    let allowed = SchemaNode::scalar("Value", ScalarType::Float)
        .with_json_allowed_values(JsonAllowedValues::new([
            JsonAllowedValue::Float(low),
            JsonAllowedValue::Float(high),
        ])?)
        .ok_or("float scalar accepts allowed values")?;
    let alternative =
        SchemaNode::group("Root", vec![SchemaNode::scalar("Value", ScalarType::Float)])
            .with_alternatives(vec![GroupAlternative {
                name: "low".into(),
                members: vec!["Value".into()],
                required: vec!["Value".into()],
                constraints: vec![GroupAlternativeConstraint {
                    member: "Value".into(),
                    value: GroupAlternativeConstraintValue::Float(low),
                }],
            }])
            .ok_or("float discriminator is valid")?;
    let contains = SchemaNode::scalar("Values", ScalarType::Float)
        .repeating()
        .with_json_contains(
            JsonContainsConstraints::new([JsonContainsConstraint::new(
                JsonContainsPredicate::schema(exact_low()?),
                ItemCountRange::new(1, None).ok_or("positive contains count")?,
            )])
            .ok_or("contains constraint is valid")?,
        )
        .ok_or("array accepts contains")?;
    let dependent_predicate = SchemaNode::group(
        "predicate",
        vec![
            SchemaNode::scalar("Trigger", ScalarType::Bool),
            exact_low()?,
        ],
    )
    .with_required_fields(vec!["Value".into()])
    .ok_or("predicate requires Value")?;
    let dependent = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Trigger", ScalarType::Bool),
            SchemaNode::scalar("Value", ScalarType::Float),
        ],
    )
    .with_json_dependent_schemas(
        JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
            "Trigger",
            JsonSchemaPredicate::schema(dependent_predicate),
        )])
        .ok_or("dependent schema is valid")?,
    )
    .ok_or("group accepts dependent schema")?;
    let xml = SchemaNode::group(
        "Root",
        vec![{
            let mut child = exact_low()?;
            child.name = "Amount".into();
            child
        }],
    );

    let schemas = [allowed, alternative, contains, dependent, xml];
    let markers: [&[(&str, u64)]; 5] = [
        &[
            ("/json_allowed_values/0/value", 0x0031_fa18_2c40_c60d),
            ("/json_allowed_values/1/value", 0x0031_fa18_2c40_c60e),
        ],
        &[(
            "/kind/alternatives/0/constraints/0/value/value",
            0x0031_fa18_2c40_c60d,
        )],
        &[
            (
                "/json_contains/0/predicate/schema/numeric_range/bounds/minimum/value",
                0x0031_fa18_2c40_c60d,
            ),
            (
                "/json_contains/0/predicate/schema/numeric_range/bounds/maximum/value",
                0x0031_fa18_2c40_c60d,
            ),
        ],
        &[
            (
                "/json_dependent_schemas/0/predicate/schema/kind/children/1/numeric_range/bounds/minimum/value",
                0x0031_fa18_2c40_c60d,
            ),
            (
                "/json_dependent_schemas/0/predicate/schema/kind/children/1/numeric_range/bounds/maximum/value",
                0x0031_fa18_2c40_c60d,
            ),
        ],
        &[
            (
                "/kind/children/0/numeric_range/bounds/minimum/value",
                0x0031_fa18_2c40_c60d,
            ),
            (
                "/kind/children/0/numeric_range/bounds/maximum/value",
                0x0031_fa18_2c40_c60d,
            ),
        ],
    ];
    let mut descriptors = Vec::new();
    for schema in &schemas {
        let encoded =
            codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?;
        assert_eq!(encoded, serde_json::to_string(schema)?);
        let decoded: SchemaNode = serde_json::from_str(&encoded)?;
        assert_eq!(
            serde_json::to_string(&decoded)?,
            serde_json::to_string(schema)?
        );
        descriptors.push(encoded);
    }
    for (schema, positions) in schemas.iter().zip(markers) {
        let mut payload = serde_json::to_value(schema)?;
        for &(path, bits) in positions {
            let slot = payload
                .pointer_mut(path)
                .expect("literal Float metadata slot");
            assert_eq!(slot.as_f64().unwrap().to_bits(), bits);
            *slot = serde_json::Value::String(format!("FERRULE-F64-BITS:{bits:016x}"));
        }
        descriptors.push(format!(
            "FERRULE-EMBEDDED-SCHEMA/2\n{}",
            serde_json::to_string(&payload)?
        ));
    }

    let program = Program {
        xml_boundary: None,
        source: SchemaNode::group("Source", Vec::new()),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", Vec::new()),
        expressions: Vec::new(),
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };
    let artifacts = codegen_csharp::emit(&program)?;
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .ok_or("generated mapping source exists")?;
    let generated = std::str::from_utf8(&generated.contents)?;
    assert!(generated.contains("FerruleJson.ParseEmbedded("));
    assert!(generated.contains("FerruleJson.SerializeEmbedded("));
    assert!(
        artifacts
            .files()
            .iter()
            .any(|file| file.path.as_str() == "Runtime/FerruleEmbeddedSchema.cs")
    );

    let directory = TempDirectory::new()?;
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.contents)?;
    }
    write_harness(directory.path(), &descriptors)?;
    let build = Command::new("dotnet")
        .args([
            "build",
            "-warnaserror",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(directory.path())
        .output()?;
    assert_succeeded("generated C# build", &build);
    let run = Command::new("dotnet")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--no-build",
        ])
        .current_dir(directory.path())
        .output()?;
    assert_succeeded("generated C# embedded schema harness", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "embedded schemas passed"
    );
    Ok(())
}

fn write_harness(root: &Path, descriptors: &[String]) -> Result<(), std::io::Error> {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        directory.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    let literal = |index: usize| {
        format!(
            "mode == 0 ? {} : {}",
            serde_json::to_string(&descriptors[index]).unwrap(),
            serde_json::to_string(&descriptors[index + 5]).unwrap()
        )
    };
    let source = HARNESS
        .replace("__ALLOWED__", &literal(0))
        .replace("__ALTERNATIVE__", &literal(1))
        .replace("__CONTAINS__", &literal(2))
        .replace("__DEPENDENT__", &literal(3))
        .replace("__XML__", &literal(4));
    std::fs::write(directory.join("Program.cs"), source)
}

const HARNESS: &str = r#"using Ferrule.Runtime;

for (var mode = 0; mode < 2; mode++) {
var low = BitConverter.Int64BitsToDouble(0x0031fa182c40c60d);
var high = BitConverter.Int64BitsToDouble(0x0031fa182c40c60e);
var allowed = __ALLOWED__;
var alternative = __ALTERNATIVE__;
var contains = __CONTAINS__;
var dependent = __DEPENDENT__;
var xml = __XML__;

static FerruleScalar Scalar(double value) => new(FerruleValue.FromDouble(value));
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static void Reject(Action action) {
    try { action(); throw new Exception("expected JSON boundary rejection"); }
    catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary) { }
}

_ = FerruleJson.SerializeEmbedded(allowed, Scalar(low));
_ = FerruleJson.SerializeEmbedded(allowed, Scalar(high));
_ = FerruleJson.SerializeEmbedded(alternative, Group(Field("Value", Scalar(low))));
Reject(() => FerruleJson.SerializeEmbedded(alternative, Group(Field("Value", Scalar(high)))));
_ = FerruleJson.SerializeEmbedded(contains, new FerruleRepeated(new[] { Scalar(low) }));
Reject(() => FerruleJson.SerializeEmbedded(contains, new FerruleRepeated(new[] { Scalar(high) })));
_ = FerruleJson.SerializeEmbedded(dependent, Group(
    Field("Trigger", new FerruleScalar(FerruleValue.FromBoolean(true))),
    Field("Value", Scalar(low))));
Reject(() => FerruleJson.SerializeEmbedded(dependent, Group(
    Field("Trigger", new FerruleScalar(FerruleValue.FromBoolean(true))),
    Field("Value", Scalar(high)))));
var instance = Group(Field("Amount", Scalar(low)));
var embeddedXml = FerruleXml.SerializeEmbedded(7, xml, instance, false, false, null);
var plainXml = FerruleXml.Serialize(7,
    mode == 0 ? xml : xml["FERRULE-EMBEDDED-SCHEMA/2\n".Length..], instance, false, false, null);
if (embeddedXml != plainXml) throw new Exception("XML descriptor changed output");
}
Console.WriteLine("embedded schemas passed");
"#;

fn assert_succeeded(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp-codegen-csharp-embedded-schema");
        std::fs::create_dir_all(&base)?;
        let path = base.join(format!(
            "embedded-schema-csharp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
