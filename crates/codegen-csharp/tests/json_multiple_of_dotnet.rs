use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, Program, ScalarTargetDomain, TargetConstruction,
    TargetScope,
};
use ir::{JsonMultipleOf, JsonMultipleOfConstraints, ScalarType, SchemaNode};

#[test]
fn emitted_package_enforces_exact_source_and_normalized_target_multiples()
-> Result<(), Box<dyn std::error::Error>> {
    let corpus = multiple_of_corpus()?;
    assert_eq!(corpus.len(), 406);
    assert!(corpus.iter().any(|case| case.expected));
    assert!(corpus.iter().any(|case| !case.expected));

    let mut source_fields = vec![
        multiple_of_scalar("Quantity", ScalarType::Int, "3")?,
        multiple_of_scalar("Fraction", ScalarType::Float, "0.1")?,
        SchemaNode::scalar("Raw", ScalarType::String),
    ];
    for case in &corpus {
        source_fields.push(multiple_of_scalar_with_divisor(
            &case.name,
            ScalarType::Float,
            case.divisor,
        )?);
    }
    let program = Program {
        source: SchemaNode::group("Source", source_fields),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![multiple_of_scalar("Amount", ScalarType::Float, "0.25")?],
        ),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Raw".into()],
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Amount".into(),
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::Float),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };

    let harness = render_harness(&corpus)?;
    run_generated(&program, &harness)
}

fn multiple_of_scalar(
    name: &str,
    ty: ScalarType,
    divisor: &str,
) -> Result<SchemaNode, Box<dyn std::error::Error>> {
    let divisor = JsonMultipleOf::from_decimal_lexical(divisor)
        .ok_or("test multipleOf divisor is representable")?;
    multiple_of_scalar_with_divisor(name, ty, divisor)
}

fn multiple_of_scalar_with_divisor(
    name: &str,
    ty: ScalarType,
    divisor: JsonMultipleOf,
) -> Result<SchemaNode, Box<dyn std::error::Error>> {
    let constraints = JsonMultipleOfConstraints::new([[divisor]])?;
    SchemaNode::scalar(name, ty)
        .with_json_multiple_of(constraints)
        .ok_or_else(|| "test multipleOf constraints match numeric scalar".into())
}

struct MultipleOfCase {
    name: String,
    divisor_source: &'static str,
    divisor: JsonMultipleOf,
    value_lexical: String,
    value_bits: u64,
    expected: bool,
}

fn multiple_of_corpus() -> Result<Vec<MultipleOfCase>, Box<dyn std::error::Error>> {
    const DIVISORS: [&str; 14] = [
        "1",
        "2",
        "3",
        "0.1",
        "0.03",
        "0.06",
        "0.07",
        "0.25",
        "1e-7",
        "1e20",
        "5e-324",
        "1e-323",
        "22250738585072014e-324",
        "17976931348623157e292",
    ];
    let values = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        2.0,
        -2.0,
        3.0,
        5.0,
        6.0,
        7.0,
        10.0,
        12.0,
        0.1,
        0.2,
        0.3,
        f64::from_bits(0.3_f64.to_bits() + 1),
        0.21,
        0.14,
        -0.3,
        -0.31,
        1e-7,
        1e-8,
        1e20,
        1e21,
        f64::from_bits(1),
        f64::from_bits(2),
        f64::from_bits(3),
        f64::MIN_POSITIVE,
        f64::MAX,
    ];

    let mut cases = Vec::with_capacity(DIVISORS.len() * values.len());
    for (divisor_index, divisor_source) in DIVISORS.into_iter().enumerate() {
        let divisor = JsonMultipleOf::from_decimal_lexical(divisor_source)
            .ok_or("audit divisor is representable")?;
        for (value_index, value) in values.into_iter().enumerate() {
            let value_lexical = match (divisor_index + value_index) % 3 {
                0 => value.to_string(),
                1 => format!("{value:e}"),
                _ => format!("{value:E}"),
            };
            cases.push(MultipleOfCase {
                name: format!("Case{:03}", cases.len()),
                divisor_source,
                divisor,
                value_lexical,
                value_bits: value.to_bits(),
                expected: divisor.divides_f64(value),
            });
        }
    }
    Ok(cases)
}

fn run_generated(program: &Program, harness: &str) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(program)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/Json/FerruleJson.MultipleOf.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("JsonMultipleOfConstraints"))
    }));
    let directory = TempDirectory::new()?;
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.contents)?;
    }
    write_harness(directory.path(), harness)?;

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
    assert_command_succeeded("dotnet build", &build);

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
    assert_command_succeeded("generated harness", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated JSON multipleOf passed"
    );
    Ok(())
}

fn write_harness(root: &Path, program: &str) -> Result<(), std::io::Error> {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        directory.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(directory.join("Program.cs"), program)?;
    Ok(())
}

fn render_harness(corpus: &[MultipleOfCase]) -> Result<String, Box<dyn std::error::Error>> {
    let mut valid_fields = vec![
        r#""Quantity":6"#.to_owned(),
        r#""Fraction":0.3"#.to_owned(),
        r#""Raw":"1.50""#.to_owned(),
    ];
    for case in corpus.iter().filter(|case| case.expected) {
        valid_fields.push(format!(r#""{}":{}"#, case.name, case.value_lexical));
    }
    let valid_input = format!("{{{}}}", valid_fields.join(","));

    let mut invalid_cases = vec![
        (
            "integer source mismatch".to_owned(),
            r#"{"Quantity":7,"Raw":"1.50"}"#.to_owned(),
        ),
        (
            "adjacent float source mismatch".to_owned(),
            r#"{"Fraction":0.30000000000000004,"Raw":"1.50"}"#.to_owned(),
        ),
        (
            "normalized target mismatch".to_owned(),
            r#"{"Raw":"1.3"}"#.to_owned(),
        ),
    ];
    for case in corpus.iter().filter(|case| !case.expected) {
        invalid_cases.push((
            format!(
                "{} divisor={} value={} bits={:016x}",
                case.name, case.divisor_source, case.value_lexical, case.value_bits
            ),
            format!(r#"{{"{}":{},"Raw":"1.50"}}"#, case.name, case.value_lexical),
        ));
    }

    let mut harness = String::from(
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var validOutput = GeneratedMapping.ExecuteJson(
"#,
    );
    writeln!(harness, "    {});", serde_json::to_string(&valid_input)?)?;
    harness.push_str(
        r#"if (!string.Equals(
        validOutput,
"#,
    );
    writeln!(
        harness,
        "        {},",
        serde_json::to_string("{\n  \"Amount\": 1.5\n}\n")?
    )?;
    harness.push_str(
        r#"
        StringComparison.Ordinal))
{
    throw new Exception($"valid multipleOf corpus produced: {validOutput}");
}

foreach (var (label, input) in new (string Label, string Input)[]
         {
"#,
    );
    for (label, input) in invalid_cases {
        writeln!(
            harness,
            "             ({}, {}),",
            serde_json::to_string(&label)?,
            serde_json::to_string(&input)?
        )?;
    }
    harness.push_str(
        r#"         })
{
    try
    {
        _ = GeneratedMapping.ExecuteJson(input);
        throw new Exception($"multipleOf mismatch should fail: {label}: {input}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary)
    {
    }
}

Console.WriteLine("generated JSON multipleOf passed");
"#,
    );
    Ok(harness)
}

fn assert_command_succeeded(label: &str, output: &std::process::Output) {
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
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_multiple_of_dotnet_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&path)?;
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
