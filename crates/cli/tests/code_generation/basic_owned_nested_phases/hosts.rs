//! Public emitted-library consumers. Literal rendering only translates the
//! already-frozen typed fixture; it never derives a result from an execution.
use super::*;
use std::fmt::Write as _;

fn literal(value: &Instance, language: &str) -> String {
    match value {
        Instance::Scalar(Value::String(value)) => {
            if language == "rust" {
                format!("scalar(Value::String({value:?}.into()))")
            } else {
                format!(
                    "new FerruleScalar(FerruleValue.FromString({}))",
                    serde_json::to_string(value).unwrap()
                )
            }
        }
        Instance::Scalar(Value::Int(value)) => {
            if language == "rust" {
                format!("scalar(Value::Int({value}))")
            } else {
                format!("new FerruleScalar(FerruleValue.FromInt64({value}L))")
            }
        }
        Instance::Scalar(Value::Bool(value)) => {
            if language == "rust" {
                format!("scalar(Value::Bool({value}))")
            } else {
                format!("new FerruleScalar(FerruleValue.FromBoolean({value}))")
            }
        }
        Instance::Group(fields) => {
            let fields = fields
                .iter()
                .map(|(name, value)| {
                    let value = literal(value, language);
                    if language == "rust" {
                        format!("field({name:?},{value})")
                    } else {
                        format!(
                            "new FerruleField({},{value})",
                            serde_json::to_string(name).unwrap()
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            if language == "rust" {
                format!("group([{fields}])")
            } else {
                format!("new FerruleGroup(new FerruleField[]{{{fields}}})")
            }
        }
        Instance::Repeated(items) => {
            let items = items
                .iter()
                .map(|item| literal(item, language))
                .collect::<Vec<_>>()
                .join(",");
            if language == "rust" {
                format!("repeated([{items}])")
            } else {
                format!("new FerruleRepeated(new FerruleInstance[]{{{items}}})")
            }
        }
        other => panic!("fixture has an unplanned typed value: {other:?}"),
    }
}

fn rust_host(cases: &[Case]) -> String {
    let mut source = String::from(
        r#"use codegen_runtime::{field,group,repeated,scalar,Value,Instance};
use ferrule_generated_mapping::{execute_with_basic_owned_sources,execute_selected_with_basic_owned_sources,
    BasicNamedSourceInput,OwnedSourceLimits,RuntimeError,TargetSelection,ExecutionOutputs,SelectedTargetOutput};
fn limits()->OwnedSourceLimits { OwnedSourceLimits {max_roots:4096,max_active_frames:256,
    max_path_segments:256,max_metadata_bytes:16777216,max_walk_contexts:1000000} }
fn main()->Result<(),Box<dyn std::error::Error>> {
 let records=std::path::PathBuf::from(std::env::args_os().nth(1).ok_or("records path")?);
 std::fs::create_dir(&records)?;
 let mut failures=Vec::new();
 let mut calls=0usize;
"#,
    );
    for case in cases {
        let primary = literal(&case.primary, "rust");
        let named = literal(&case.named, "rust");
        writeln!(
            source,
            "{{ let primary:Instance={primary}; let named:Instance={named};"
        )
        .unwrap();
        source.push_str("let supplied=[BasicNamedSourceInput{name:\"Ref\",instance:&named}];\n");
        match &case.expected {
            Expected::Tree(tree) => {
                writeln!(source, "let tree:Instance={};", literal(tree, "rust")).unwrap();
                source.push_str("let expected_all=Ok(ExecutionOutputs{primary:tree.clone(),extras:vec![]});\nlet expected_selected=Ok(SelectedTargetOutput::Primary(tree));\n");
            }
            Expected::Raise { node, message } => {
                let error = format!(
                    "RuntimeError::MappingException{{node:{node},message:Some({message:?}.into())}}"
                );
                writeln!(
                    source,
                    "let expected_all=Err({error}); let expected_selected=Err({error});"
                )
                .unwrap();
            }
            Expected::ItemCount { node } => {
                let error = format!("RuntimeError::NotAnItemCount{{node:{node},found:\"bool\"}}");
                writeln!(
                    source,
                    "let expected_all=Err({error}); let expected_selected=Err({error});"
                )
                .unwrap();
            }
        }
        let id = case.id;
        writeln!(source, r#"let actual=execute_with_basic_owned_sources(&primary,&supplied,None,limits());
 std::fs::write(records.join({id:?}.to_owned()+"-all.txt"),format!("{{actual:#?}}\n"))?;
 if actual!=expected_all {{ failures.push(format!("{{}} all: {{actual:#?}}",{id:?})); }} calls+=1;
 let actual=execute_selected_with_basic_owned_sources(&primary,&supplied,None,limits(),TargetSelection::Primary);
 std::fs::write(records.join({id:?}.to_owned()+"-selected.txt"),format!("{{actual:#?}}\n"))?;
 if actual!=expected_selected {{ failures.push(format!("{{}} selected: {{actual:#?}}",{id:?})); }} calls+=1;
 }}"#).unwrap();
    }
    source.push_str("assert_eq!(calls,28); println!(\"Basic nested phases: {calls} calls, {} mismatches\",failures.len());\nif !failures.is_empty(){ return Err(failures.join(\"\\n\").into()); } Ok(()) }\n");
    source
}

fn csharp_host(cases: &[Case]) -> String {
    let mut source = String::from(
        r#"using Ferrule.Generated;
using Ferrule.Runtime;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Serialization;
internal static class Host {
static readonly List<string> Failures=new();
static string Records="";
static int Calls;
static FerruleOwnedSourceLimits Limits()=>new(4096,256,256,16777216,1000000);
static readonly JsonSerializerOptions JsonOptions=new(){Converters={new JsonStringEnumConverter()}};
static string Json(object? value)=>JsonSerializer.Serialize(value,JsonOptions);
static object Snapshot(FerruleInstance value)=>value switch {
 FerruleScalar scalar=>new object[]{"Scalar",scalar.Value.Kind.ToString(),scalar.Value.ToString()},
 FerruleGroup group=>new object[]{"Group",group.Fields.Select(f=>new object[]{f.Name,Snapshot(f.Value)}).ToArray()},
 FerruleRepeated repeated=>new object[]{"Repeated",repeated.Items.Select(Snapshot).ToArray()},
 _=>throw new InvalidOperationException("unplanned output instance kind: "+value.GetType())
};
// Every declared machine-readable property is retained, including null payloads
// and typed causes. Human exception text is also kept separately, not matched.
static string ErrorSnapshot(FerruleRuntimeException error)=>Json(typeof(FerruleRuntimeException)
 .GetProperties(BindingFlags.Public|BindingFlags.Instance|BindingFlags.DeclaredOnly)
 .OrderBy(p=>p.Name,StringComparer.Ordinal).ToDictionary(p=>p.Name,p=>p.GetValue(error)));
static void Attempt(string id,bool selected,FerruleInstance primary,FerruleInstance named,
 FerruleInstance? expected,FerruleRuntimeException? expectedError) {
 var label=id+(selected?"-selected":"-all"); Calls++;
 try {
  var supplied=new[]{new NamedInput("Ref",named)};
  FerruleInstance tree;
  if(selected) {
   var output=GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(primary,FerruleTargetSelection.Primary(),supplied,Limits(),null);
   if(output is not SelectedTargetOutput.Primary item) throw new InvalidOperationException("selected output kind");
   tree=item.Instance;
  } else {
   var output=GeneratedMapping.ExecuteWithBasicOwnedSources(primary,supplied,Limits(),null);
   if(output.Extras.Count!=0) throw new InvalidOperationException("unexpected extra outputs");
   tree=output.Primary;
  }
  var actual=Json(Snapshot(tree));
  File.WriteAllText(Path.Combine(Records,label+".txt"),actual+"\n");
  if(expectedError is not null||expected is null||actual!=Json(Snapshot(expected))) Failures.Add(label+" unexpected tree: "+actual);
 } catch(FerruleRuntimeException error) {
   var record=Path.Combine(Records,label+".txt"); File.WriteAllText(record,error+"\n");
   if(error.InnerException is not null) Failures.Add(label+" unexpected inner cause: "+error.InnerException);
  try {
   var actual=ErrorSnapshot(error); File.AppendAllText(record,actual+"\n");
   if(expectedError is null||actual!=ErrorSnapshot(expectedError)) Failures.Add(label+" unexpected runtime error: "+actual);
  } catch(Exception snapshotError) {
   File.AppendAllText(record,snapshotError+"\n"); Failures.Add(label+" unexpected unrenderable error: "+error);
  }
 } catch(Exception error) {
  File.WriteAllText(Path.Combine(Records,label+".txt"),error+"\n"); Failures.Add(label+" unexpected exception: "+error);
 }
}
static int Main(string[] args) {
 Records=args[0]; Directory.CreateDirectory(Records);
"#,
    );
    for case in cases {
        let primary = literal(&case.primary, "csharp");
        let named = literal(&case.named, "csharp");
        writeln!(
            source,
            "{{ FerruleInstance primary={primary}; FerruleInstance named={named};"
        )
        .unwrap();
        match &case.expected {
            Expected::Tree(tree) => {
                writeln!(
                    source,
                    "FerruleInstance? expected={}; FerruleRuntimeException? error=null;",
                    literal(tree, "csharp")
                )
                .unwrap();
            }
            Expected::Raise { node, message } => {
                writeln!(source, "FerruleInstance? expected=null; FerruleRuntimeException? error=new(FerruleRuntimeError.MappingException,\"frozen\",node:{node}U,mappingExceptionMessage:{});", serde_json::to_string(message).unwrap()).unwrap();
            }
            Expected::ItemCount { node } => {
                writeln!(source, "FerruleInstance? expected=null; FerruleRuntimeException? error=new(FerruleRuntimeError.NotAnItemCount,\"frozen\",node:{node}U,foundKind:FerruleValueKind.Bool);").unwrap();
            }
        }
        writeln!(source, "Attempt({0:?},false,primary,named,expected,error); Attempt({0:?},true,primary,named,expected,error); }}", case.id).unwrap();
    }
    source.push_str("if(Calls!=28)throw new InvalidOperationException(\"call inventory\");\nConsole.WriteLine($\"Basic nested phases: {Calls} calls, {Failures.Count} mismatches\");\nforeach(var failure in Failures)Console.Error.WriteLine(failure); return Failures.Count==0?0:1; } }\n");
    source
}

fn save_artifacts(directory: &Path, artifacts: codegen::ArtifactSet) -> TestResult<()> {
    std::fs::create_dir(directory)?;
    for artifact in artifacts.into_files() {
        let destination = directory.join(artifact.path.as_str());
        std::fs::create_dir_all(destination.parent().unwrap())?;
        std::fs::write(destination, artifact.contents)?;
    }
    Ok(())
}
fn assert_command(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed (originals retained):\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn exercise(language: &str) -> TestResult<()> {
    let project = project_model();
    let cases = cases();
    let mut directory = Directory::new(language)?;
    freeze(&directory.path, &project, &cases)?;
    let before = serde_json::to_vec(&project)?;
    let lowering = codegen::lower_with_basic_owned_windows(&project);
    std::fs::write(
        directory.path.join("lowering-observation.txt"),
        format!("{lowering:#?}\n"),
    )?;
    let lowered = lowering?;
    assert_eq!(
        serde_json::to_vec(&project)?,
        before,
        "public lowering preserves Project"
    );
    std::fs::write(
        directory.path.join("lowered-program.txt"),
        format!("{lowered:#?}\n"),
    )?;
    let generated = directory.path.join("generated");
    let host = directory.path.join("host");
    std::fs::create_dir(&host)?;
    let records = directory.path.join("actual-records");
    if language == "rust" {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../codegen-runtime")
            .canonicalize()?;
        let options = codegen_rust::Options {
            package_name: "ferrule-generated-mapping".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_str().ok_or("UTF-8 runtime path")?.into(),
            ),
        };
        save_artifacts(
            &generated,
            codegen_rust::emit_with_basic_owned_windows(&lowered, &options)?,
        )?;
        let unchanged = artifact_files(&generated)?;
        std::fs::create_dir(host.join("src"))?;
        std::fs::write(host.join("src/main.rs"), rust_host(&cases))?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname=\"ferrule-basic-nested-phase-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\n"
            ),
        )?;
        let mut command = Command::new("cargo");
        command
            .args([
                "run",
                "--quiet",
                "--offline",
                "--jobs",
                "1",
                "--manifest-path",
            ])
            .arg(host.join("Cargo.toml"))
            .arg("--")
            .arg(&records)
            .current_dir(&directory.path);
        // The shared helper enforces a dedicated target, a one-job nonincremental
        // host build, exact command retention and the existing cache policy.
        let output = host_policy::recorded_output(&mut command, &directory.path, "rust-host")?;
        assert_command(&output, "Rust public consumer");
        assert_eq!(
            artifact_files(&generated)?,
            unchanged,
            "emitted Rust library unchanged"
        );
    } else {
        save_artifacts(
            &generated,
            codegen_csharp::emit_with_basic_owned_windows(&lowered)?,
        )?;
        let unchanged = artifact_files(&generated)?;
        std::fs::write(host.join("Program.cs"), csharp_host(&cases))?;
        std::fs::write(
            host.join("Host.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include="../generated/Ferrule.Generated.csproj"/></ItemGroup></Project>"#,
        )?;
        let artifacts = directory.path.join("artifacts");
        let mut build = dotnet_command(&directory.path);
        build
            .args([
                "build",
                "--disable-build-servers",
                "--nologo",
                "--verbosity",
                "quiet",
                "--configuration",
                "Release",
                "-m:1",
                "-p:UseSharedCompilation=false",
                "--artifacts-path",
            ])
            .arg(&artifacts)
            .arg(host.join("Host.csproj"))
            .current_dir(&directory.path)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let built = host_policy::recorded_output(&mut build, &directory.path, "csharp-build")?;
        assert_command(&built, "C# public consumer build");
        let mut command = dotnet_command(&directory.path);
        command
            .arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(&records)
            .current_dir(&directory.path)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let output = host_policy::recorded_output(&mut command, &directory.path, "csharp-host")?;
        assert_command(&output, "C# public consumer");
        assert_eq!(
            artifact_files(&generated)?,
            unchanged,
            "emitted C# library unchanged"
        );
    }
    assert_eq!(
        std::fs::read_dir(&records)?.count(),
        28,
        "14 cases x two public calls"
    );
    directory.complete = true;
    Ok(())
}
