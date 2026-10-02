#[path = "../../mapping/tests/support/primary_root_project.rs"]
mod fixture;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn write_artifacts(root: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = root.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.contents).unwrap();
    }
}

fn checked_command(root: &Path, name: &str, mut command: Command) -> Output {
    let result = command.current_dir(root).output().unwrap();
    std::fs::write(root.join(format!("{name}.stdout")), &result.stdout).unwrap();
    std::fs::write(root.join(format!("{name}.stderr")), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{name} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}

fn registry_identities(lock: &str) -> std::collections::BTreeSet<(&str, &str, &str)> {
    fn field<'a>(block: &'a str, name: &str) -> &'a str {
        block
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)?
                    .strip_prefix(" = \"")?
                    .strip_suffix('"')
            })
            .unwrap()
    }
    lock.split("[[package]]")
        .filter(|block| {
            block
                .lines()
                .any(|line| line.starts_with("source = \"registry+"))
        })
        .map(|block| {
            (
                field(block, "name"),
                field(block, "version"),
                field(block, "checksum"),
            )
        })
        .collect()
}

struct EvidenceDirectory {
    path: PathBuf,
    keep: bool,
}
impl EvidenceDirectory {
    fn new() -> Self {
        let provided = std::env::var_os("FERRULE_REQUIRED_ROOT_CODEGEN_EVIDENCE");
        let keep = provided.is_some();
        let path = provided.map(PathBuf::from).unwrap_or_else(|| {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            std::env::temp_dir().join(format!(
                "ferrule_required_root_{}_{nonce}",
                std::process::id()
            ))
        });
        std::fs::create_dir(&path).unwrap();
        Self { path, keep }
    }
}
impl Drop for EvidenceDirectory {
    fn drop(&mut self) {
        if !self.keep && !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[test]
fn generated_rust_and_csharp_required_root_reads_match_lazy_typed_failures() {
    let project = fixture::required_primary_root_project(true);
    assert!(engine::validate(&project).is_empty());
    let program = codegen::lower(&project).unwrap();
    assert!(program.expressions.iter().any(|expression| matches!(
        expression.expression,
        codegen::Expression::SourceRootField { required: true, .. }
    )));
    let output = EvidenceDirectory::new();
    let rust = output.path.join("rust");
    let csharp = output.path.join("csharp");
    std::fs::create_dir(&rust).unwrap();
    std::fs::create_dir(&csharp).unwrap();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let artifacts = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "required-root-generated".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        },
    )
    .unwrap();
    write_artifacts(&rust, &artifacts);
    std::fs::write(rust.join("src/main.rs"), RUST_HOST).unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let lock = std::fs::read_to_string(workspace.join("Cargo.lock")).unwrap();
    std::fs::write(rust.join("Cargo.lock"), &lock).unwrap();
    let mut build = Command::new("cargo");
    build
        .args(["build", "--offline", "--quiet"])
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", rust.join("target"))
        .env("CARGO_INCREMENTAL", "0");
    checked_command(&rust, "actual-offline-build", build);
    let resolved = std::fs::read_to_string(rust.join("Cargo.lock")).unwrap();
    let baseline = registry_identities(&lock);
    for identity in registry_identities(&resolved) {
        assert!(
            baseline.contains(&identity),
            "resolved registry dependency outside workspace baseline: {identity:?}"
        );
    }
    let mut command = Command::new("cargo");
    command
        .args(["run", "--locked", "--offline", "--quiet"])
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", rust.join("target"))
        .env("CARGO_INCREMENTAL", "0");
    let result = checked_command(&rust, "actual-host", command);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "required primary root: 9 cases passed"
    );

    let artifacts = codegen_csharp::emit(&program).unwrap();
    write_artifacts(&csharp, &artifacts);
    std::fs::write(
        csharp.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>",
    )
    .unwrap();
    let host = csharp.join("Harness");
    std::fs::create_dir(&host).unwrap();
    std::fs::write(host.join("Harness.csproj"), CSHARP_PROJECT).unwrap();
    std::fs::write(host.join("Program.cs"), CSHARP_HOST).unwrap();
    let mut command = Command::new("dotnet");
    command.args([
        "build",
        "--configuration",
        "Release",
        "-warnaserror",
        "Harness/Harness.csproj",
    ]);
    checked_command(&csharp, "actual-build", command);
    let mut command = Command::new("dotnet");
    command.args([
        "run",
        "--project",
        "Harness/Harness.csproj",
        "--configuration",
        "Release",
        "--no-build",
    ]);
    let result = checked_command(&csharp, "actual-host", command);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "required primary root: 9 cases passed"
    );
}

const RUST_HOST: &str = r#"
use codegen_runtime::{Instance, InstanceGroup, PrimaryRootError, RuntimeError, Value, XmlTypeOrigin};
fn root(value: Option<Value>, origin: XmlTypeOrigin<'_>, marker: bool) -> Instance {
    let mut fields = value.into_iter().map(|value| ("Code".into(), Instance::Scalar(value))).collect::<Vec<_>>();
    if marker { fields.push(("\u{1f}ferrule-xml-type".into(), Instance::Scalar(Value::String("Derived".into())))); }
    Instance::Group(InstanceGroup::from(fields).with_xml_type_origin(origin).unwrap())
}
fn main() {
    for text in ["present", ""] {
        let source = root(Some(Value::String(text.into())), XmlTypeOrigin::Explicit("Derived"), false);
        let output = required_root_generated::execute(&source).unwrap();
        assert_eq!(output.field("Code").and_then(Instance::as_scalar), Some(&Value::String(text.into())));
    }
    for missing in [None, Some(Value::Null)] {
        let source = root(missing, XmlTypeOrigin::Explicit("Derived"), false);
        assert!(matches!(required_root_generated::execute(&source), Err(RuntimeError::PrimaryRoot {
            node: 1, source: PrimaryRootError::MissingRequiredField { path }
        }) if path == ["Code"]));
    }
    for (origin, marker) in [(XmlTypeOrigin::Absent, false), (XmlTypeOrigin::Explicit("Base"), false), (XmlTypeOrigin::Absent, true)] {
        let source = root(None, origin, marker);
        let output = required_root_generated::execute(&source).unwrap();
        assert_eq!(output.field("Code").and_then(Instance::as_scalar), Some(&Value::Null));
    }
    let unknown = root(None, XmlTypeOrigin::Unknown, false);
    assert!(matches!(required_root_generated::execute(&unknown), Err(RuntimeError::PrimaryRoot { node: 0, source: PrimaryRootError::UnknownXmlTypeOrigin })));
    let repeated = Instance::Repeated(vec![root(Some(Value::String("first".into())), XmlTypeOrigin::Explicit("Derived"), false)]);
    assert!(matches!(required_root_generated::execute(&repeated), Err(RuntimeError::PrimaryRoot { node: 0, source: PrimaryRootError::ExpectedGroup { found: "repeated" } })));
    println!("required primary root: 9 cases passed");
}
"#;

const CSHARP_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
<PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup>
<ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>"#;

const CSHARP_HOST: &str = r#"
using Ferrule.Runtime;
using Ferrule.Generated;
static FerruleInstance Root(FerruleValue? value, FerruleXmlTypeOrigin origin, bool marker = false) {
    var fields = new List<FerruleField>();
    if (value.HasValue) fields.Add(new FerruleField("Code", new FerruleScalar(value.Value)));
    if (marker) fields.Add(new FerruleField("\u001fferrule-xml-type", new FerruleScalar(FerruleValue.FromString("Derived"))));
    return new FerruleGroup(fields).WithXmlTypeOrigin(origin);
}
static FerruleValue Code(FerruleInstance output) => ((FerruleScalar)((FerruleGroup)output).Fields.Single(field => field.Name == "Code").Value).Value;
static void Failure(FerruleInstance input, uint node, FerrulePrimaryRootError expected, string? path = null, string? found = null) {
    try { GeneratedMapping.Execute(input); throw new Exception("expected required primary root failure"); }
    catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.PrimaryRoot) {
        var failure = error.PrimaryRoot ?? throw new Exception("missing structured primary root failure");
        if (error.Node != node || failure.Error != expected || string.Join('/', failure.Path) != (path ?? "") || failure.Found != found) throw;
    }
}
foreach (var text in new[] { "present", "" }) {
    var output = GeneratedMapping.Execute(Root(FerruleValue.FromString(text), FerruleXmlTypeOrigin.Explicit("Derived")));
    if (Code(output) != FerruleValue.FromString(text)) throw new Exception("present scalar differs");
}
foreach (var missing in new FerruleValue?[] { null, FerruleValue.Null })
    Failure(Root(missing, FerruleXmlTypeOrigin.Explicit("Derived")), 1, FerrulePrimaryRootError.MissingRequiredField, "Code");
foreach (var pair in new[] { (FerruleXmlTypeOrigin.Absent, false), (FerruleXmlTypeOrigin.Explicit("Base"), false), (FerruleXmlTypeOrigin.Absent, true) }) {
    if (Code(GeneratedMapping.Execute(Root(null, pair.Item1, pair.Item2))) != FerruleValue.Null) throw new Exception("lazy false branch differs");
}
Failure(Root(null, FerruleXmlTypeOrigin.Unknown), 0, FerrulePrimaryRootError.UnknownXmlTypeOrigin);
Failure(new FerruleRepeated([Root(FerruleValue.FromString("first"), FerruleXmlTypeOrigin.Explicit("Derived"))]), 0, FerrulePrimaryRootError.ExpectedGroup, found: "repeated");
Console.WriteLine("required primary root: 9 cases passed");
"#;
