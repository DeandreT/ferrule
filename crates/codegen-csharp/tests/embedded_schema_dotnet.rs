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
    let descriptors = schemas
        .iter()
        .map(|schema| {
            let encoded =
                codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?;
            assert!(encoded.starts_with("FERRULE-EMBEDDED-SCHEMA/2\n"));
            Ok::<_, Box<dyn std::error::Error>>(encoded)
        })
        .collect::<Result<Vec<_>, _>>()?;

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
    let source = HARNESS
        .replace(
            "__ALLOWED__",
            &serde_json::to_string(&descriptors[0]).unwrap(),
        )
        .replace(
            "__ALTERNATIVE__",
            &serde_json::to_string(&descriptors[1]).unwrap(),
        )
        .replace(
            "__CONTAINS__",
            &serde_json::to_string(&descriptors[2]).unwrap(),
        )
        .replace(
            "__DEPENDENT__",
            &serde_json::to_string(&descriptors[3]).unwrap(),
        )
        .replace("__XML__", &serde_json::to_string(&descriptors[4]).unwrap());
    std::fs::write(directory.join("Program.cs"), source)
}

const HARNESS: &str = r#"using Ferrule.Runtime;

var low = BitConverter.Int64BitsToDouble(0x0031fa182c40c60d);
var high = BitConverter.Int64BitsToDouble(0x0031fa182c40c60e);
const string allowed = __ALLOWED__;
const string alternative = __ALTERNATIVE__;
const string contains = __CONTAINS__;
const string dependent = __DEPENDENT__;
const string xml = __XML__;

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
    xml["FERRULE-EMBEDDED-SCHEMA/2\n".Length..], instance, false, false, null);
if (embeddedXml != plainXml) throw new Exception("XML descriptor changed output");
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
