use super::*;
use std::process::Command;

#[path = "../../../codegen-rust/src/tests/host_target.rs"]
#[allow(dead_code)]
mod host_target;
use host_target::GeneratedHostCommand;

struct Output(PathBuf);
impl Drop for Output {
    fn drop(&mut self) {
        // This directory was exclusively created below the test's owned fixture.
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn save(root: &std::path::Path, artifacts: codegen::ArtifactSet) {
    for file in artifacts.into_files() {
        let path = root.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, file.contents).unwrap();
    }
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn invoked_parameter_public_consumers_keep_order_and_host_failures() {
    compile_public_consumers(invoked_fixture(), RUST, CSHARP);
}

fn compile_public_consumers(fixture: Fixture, rust_template: &str, csharp_template: &str) {
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let lowered = codegen::lower_document(&imported.document).unwrap();
    let folder = fixture.0.join("generated");
    std::fs::create_dir(&folder).unwrap();
    let output = Output(folder);
    let rust = output.0.join("rust");
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .unwrap();
    save(
        &rust,
        codegen_rust::emit_invoked_with_primary_json_projection(
            &lowered,
            &codegen_rust::Options {
                package_name: "invoked_parameter_consumer".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    runtime.to_str().unwrap().into(),
                ),
            },
            &imported.invocation,
            &imported.source.shape,
        )
        .unwrap(),
    );
    let key = serde_json::to_string(&imported.invocation.key_parameter).unwrap();
    let rust_source = rust_template.replace("KEY_NAME", &key);
    std::fs::write(rust.join("src/main.rs"), rust_source).unwrap();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::copy(repository.join("Cargo.lock"), rust.join("Cargo.lock")).unwrap();
    let result = Command::new("cargo")
        .current_dir(&rust)
        .args(["run", "--offline", "--quiet"])
        .generated_host_output(&rust)
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let csharp = output.0.join("csharp");
    save(
        &csharp,
        codegen_csharp::emit_invoked_with_primary_json_projection(
            &lowered,
            &imported.invocation,
            &imported.source.shape,
        )
        .unwrap(),
    );
    let project = csharp.join("Ferrule.Generated.csproj");
    assert!(project.is_file());
    let text = std::fs::read_to_string(&project)
        .unwrap()
        .replace(
            "<TargetFramework>",
            "<OutputType>Exe</OutputType><TargetFramework>",
        )
        .replace(
            "<Compile Include=\"GeneratedMapping.cs\" />",
            "<Compile Include=\"GeneratedMapping.cs\" /><Compile Include=\"Program.cs\" />",
        );
    std::fs::write(&project, text).unwrap();
    std::fs::write(
        csharp.join("Program.cs"),
        csharp_template.replace("KEY_NAME", &key),
    )
    .unwrap();
    std::fs::write(
        csharp.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>",
    )
    .unwrap();
    let result = Command::new("dotnet")
        .current_dir(&csharp)
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .args([
            "run",
            "--project",
            "Ferrule.Generated.csproj",
            "--configuration",
            "Release",
            "-p:UseSharedCompilation=false",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn pure_sibling_conditionals_execute_in_generated_rust_and_csharp() {
    compile_conditional_consumers(
        invoked_sibling_fixture(),
        r#"{"rows":[{"name":"a","name":"a","qty":"first","qty":"last"},{"name":"a","qty":"second"}]}"#,
        &["last", "last", "second"],
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn nested_sibling_fallbacks_execute_in_generated_rust_and_csharp() {
    compile_conditional_consumers(
        invoked_nested_fixture(),
        r#"{"rows":[{"name":"a","name":"a","qty":"","fallback":"one","fallback":"replacement"},{"name":"a","qty":"second"},{"name":"a","qty":""}]}"#,
        &["replacement", "replacement", "second"],
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn sibling_qualifier_presence_executes_in_generated_rust_and_csharp() {
    compile_conditional_consumers(
        invoked_qualifier_fixture(),
        r#"{"rows":[{"name":"a","qty":""},{"name":"a","qty":"value"},{"name":"a","qty":null}]}"#,
        &["QUAL", "QUAL", ""],
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn sibling_value_alias_executes_in_generated_rust_and_csharp() {
    let fixture = invoked_qualifier_fixture();
    fixture.change(
        r#"<vertex vertexkey="603"><edges><edge vertexkey="85"/></edges></vertex>"#,
        r#"<vertex vertexkey="604"><edges><edge vertexkey="85"/></edges></vertex>"#,
    );
    compile_conditional_consumers(
        fixture,
        r#"{"rows":[{"name":"a","qty":"first","qty":"last"},{"name":"a","qty":""},{"name":"a","qty":null}]}"#,
        &["last", "", ""],
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn date_format_executes_in_generated_rust_and_csharp() {
    compile_conditional_consumers(
        invoked_date_fixture(),
        r#"{"rows":[{"name":"a","qty":"2026-10-27"},{"name":"a","qty":"2020-01-02"}]}"#,
        &["20261027", "20200102"],
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn context_sum_executes_in_generated_rust_and_csharp() {
    compile_conditional_consumers(
        invoked_context_sum_fixture(),
        r#"{"rows":[{"name":"a","qty":2,"qty":3},{"name":"a","qty":7}]}"#,
        &["5"],
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn ancestor_guard_executes_in_generated_rust_and_csharp() {
    let old = r#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2}]}"#;
    let new = r#"{"rows":[{"name":"a","flag":"","flag":"","details":[{"qty":"first"},{"qty":"second"},{"qty":null}]},{"name":"b","flag":"no","details":[{"qty":"other"}]}]}"#;
    let rust = RUST.replace(old, new).replace("all.extend(results.iter()", r#"
        for document in &results {
            let interchange=&document.value().field("Interchange").unwrap().as_repeated().unwrap()[0];
            let group=&interchange.field("Group").unwrap().as_repeated().unwrap()[0];
            let message=group.field("Message_850").unwrap();
            let message=message.as_repeated().map_or(message,|items|&items[0]);
            let actual=message.field("Lines").unwrap().as_repeated().unwrap().iter().map(|line|line.field("LIN").and_then(Instance::as_repeated).map_or(0,|items|items.len())).collect::<Vec<_>>();
            assert_eq!(actual,[2,2,0,0]);
        }
        all.extend(results.iter()"#);
    let csharp = CSHARP.replace(&serde_json::to_string(old).unwrap(), &serde_json::to_string(new).unwrap()).replace("all.AddRange(result.Documents", r#"
        FerruleInstance Field(FerruleInstance value,string name)=>((FerruleGroup)value).Fields.Single(field=>field.Name==name).Value;
        foreach(var document in result.Documents) {
            var interchange=((FerruleRepeated)Field(document.Value,"Interchange")).Items[0];
            var group=((FerruleRepeated)Field(interchange,"Group")).Items[0];
            var message=Field(group,"Message_850");
            if(message is FerruleRepeated repeated)message=repeated.Items[0];
            var actual=((FerruleRepeated)Field(message,"Lines")).Items.Select(line=>((FerruleGroup)line).Fields.SingleOrDefault(field=>field.Name=="LIN")?.Value is FerruleRepeated inner ? inner.Items.Count : 0).ToArray();
            if(!actual.SequenceEqual(new[]{2,2,0,0}))throw new Exception("ancestor ownership");
        }
        all.AddRange(result.Documents"#);
    compile_public_consumers(invoked_ancestor_guard_fixture(), &rust, &csharp);
}

fn compile_conditional_consumers(fixture: Fixture, new: &str, expected: &[&str]) {
    let old = r#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2}]}"#;
    let rust_expected = expected
        .iter()
        .map(|text| {
            format!(
                "Instance::Scalar(Value::String({}.into()))",
                serde_json::to_string(text).unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let csharp_expected = expected
        .iter()
        .map(|text| serde_json::to_string(text).unwrap())
        .collect::<Vec<_>>()
        .join(",");
    let rust = RUST.replace(old, new)
        .replace("[\"Pout.edia\",\"Pout.edib\",\"Qout.edia\",\"Qout.edib\"]", "[\"Pout.edia\",\"Qout.edia\"]")
        .replace("all.extend(results.iter()", r#"
            let interchange=&results[0].value().field("Interchange").unwrap().as_repeated().unwrap()[0];
            let group=&interchange.field("Group").unwrap().as_repeated().unwrap()[0];
            let message=group.field("Message_850").unwrap();
            let message=message.as_repeated().map_or(message,|items|&items[0]);
            let actual=message.field("Lines").unwrap().as_repeated().unwrap().iter().map(|line|line.field("LIN").unwrap().field("F1").unwrap().clone()).collect::<Vec<_>>();
            assert_eq!(actual,[EXPECTED_VALUES]);
            all.extend(results.iter()"#).replace("EXPECTED_VALUES", &rust_expected);
    let csharp = CSHARP.replace(&serde_json::to_string(old).unwrap(), &serde_json::to_string(new).unwrap())
        .replace("new[]{\"Pout.edia\",\"Pout.edib\",\"Qout.edia\",\"Qout.edib\"}", "new[]{\"Pout.edia\",\"Qout.edia\"}")
        .replace("all.AddRange(result.Documents", r#"
        FerruleInstance Field(FerruleInstance value,string name)=>((FerruleGroup)value).Fields.Single(field=>field.Name==name).Value;
        var interchange=((FerruleRepeated)Field(result.Documents[0].Value,"Interchange")).Items[0];
        var group=((FerruleRepeated)Field(interchange,"Group")).Items[0];
        var message=Field(group,"Message_850");
        if(message is FerruleRepeated repeated)message=repeated.Items[0];
        var actual=((FerruleRepeated)Field(message,"Lines")).Items.Select(line=>((FerruleScalar)Field(Field(line,"LIN"),"F1")).Value.StringValue).ToArray();
        if(!actual.SequenceEqual(new[]{EXPECTED_VALUES}))throw new Exception("conditional ownership");
        all.AddRange(result.Documents"#).replace("EXPECTED_VALUES", &csharp_expected);
    compile_public_consumers(fixture, &rust, &csharp);
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn integer_source_values_truncate_in_generated_rust_and_csharp() {
    compile_conditional_consumers(
        invoked_integer_fixture(),
        r#"{"rows":[{"name":"a","qty":1.9},{"name":"a","qty":-1.9},{"name":"a","qty":-0.9},{"name":"a","qty":9223372036854775808.9}]}"#,
        &["1", "-1", "0", "9223372036854775808"],
    );
}

#[test]
#[ignore = "compiles generated Rust and C# consumers; requires Cargo, .NET 10 and a coordinated host target"]
fn fixed_date_prefix_parses_in_generated_rust_and_csharp() {
    compile_conditional_consumers(
        invoked_date_prefix_fixture(),
        r#"{"rows":[{"name":"a","qty":"20261028235959"}]}"#,
        &["2026-10-28"],
    );
}

const RUST: &str = r###"
use codegen_runtime::{Instance,Value,ExecutionContext,RuntimeParameters};
use invoked_parameter_consumer as generated;
fn main() {
    assert_eq!(generated::parameter_input_name(), "Variables");
    let mut all=Vec::new();
    let mut visits=0;
    for text in ["{}", "{\"projectCode\":\"P\",\"projectCode\":\"P\",\"projectCode\":\"Q\"}"] {
        let prepared=generated::prepare_parameter_invocations(&Value::String(text.into()),Default::default(),Default::default()).unwrap();
        prepared.visit::<String>(|item| {
            visits+=1;
            let mut params=RuntimeParameters::new();
            params.insert("Input",Value::String("source.json".into())).unwrap();
            params.insert(KEY_NAME,Value::String(item.key().into())).unwrap();
            let context=ExecutionContext::new(std::path::Path::new("test.mfd")).with_parameters(&params);
            let documents=generated::acquire_primary_json(&context,
                |name| { assert_eq!(name,"source.json"); Ok::<_,String>(vec![Ok(name.to_owned())].into_iter()) },
                |name| generated::parse_primary_json_source(name,br#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2}]}"#).map_err(|e|e.to_string()),
                Default::default(),1).map_err(|e|format!("{e:?}"))?.collect::<Result<Vec<_>,_>>().map_err(|e|format!("{e:?}"))?;
            let source=Instance::Group(vec![("Documents".into(),Instance::Repeated(documents))].into());
            let Instance::DocumentSet(results)=generated::execute_with_context(&source,&context).map_err(|e|e.to_string())? else { panic!("document set") };
            all.extend(results.iter().map(|d|d.path().to_owned()));
            Ok(())
        }).unwrap();
    }
    assert_eq!(visits,2);
    assert_eq!(all,["Pout.edia","Pout.edib","Qout.edia","Qout.edib"]);
    assert!(generated::prepare_parameter_invocations(&Value::String("{".into()),Default::default(),Default::default()).is_err());
    let prepared=generated::prepare_parameter_invocations(&Value::String("{\"projectCode\":\"P\",\"projectCode\":\"Q\",\"projectCode\":\"R\"}".into()),Default::default(),Default::default()).unwrap();
    let mut seen=Vec::new();
    let result=prepared.visit(|item| { seen.push(item.key().to_owned()); if item.key()=="Q" {Err("host failure")} else {Ok(())} });
    assert_eq!(result,Err("host failure"));assert_eq!(seen,["P","Q"]);
}
"###;

const CSHARP: &str = r###"
using Ferrule.Runtime;
using Ferrule.Generated;
if (GeneratedMapping.ParameterInputName != "Variables") throw new Exception("parameter");
var all=new List<string>();int visits=0;
foreach(var text in new[]{"{}","{\"projectCode\":\"P\",\"projectCode\":\"P\",\"projectCode\":\"Q\"}"}) {
    var prepared=GeneratedMapping.PrepareParameterInvocations(FerruleValue.FromString(text));
    prepared.Visit(item=> {
        visits++;
        var values=new FerruleRuntimeParameters(new[]{new KeyValuePair<string,FerruleValue>("Input",FerruleValue.FromString("source.json")),item.KeyBinding});
        if(item.KeyBinding.Key != KEY_NAME) throw new Exception("key binding");
        var context=new FerruleExecutionContext("test.mfd","test.mfd",runtimeParameters:values);
        var documents=GeneratedMapping.AcquirePrimaryJson(context,
            name=>name=="source.json"?new[]{name}:throw new Exception("input path"),
            name=>GeneratedMapping.ParsePrimaryJsonSource(name,System.Text.Encoding.UTF8.GetBytes("{\"rows\":[{\"name\":\"a\",\"qty\":1},{\"name\":\"b\",\"qty\":2}]}")),new(),1).ToArray();
        var source=new FerruleGroup(new[]{new FerruleField("Documents",new FerruleRepeated(documents))});
        var result=(FerruleDocumentSet)GeneratedMapping.Execute(source,context);
        all.AddRange(result.Documents.Select(d=>d.Path));
    });
}
if(visits!=2 || !all.SequenceEqual(new[]{"Pout.edia","Pout.edib","Qout.edia","Qout.edib"})) throw new Exception("order");
try {GeneratedMapping.PrepareParameterInvocations(FerruleValue.FromString("{"));throw new Exception("parse expected");} catch(FerruleJsonSourceException) {}
var failing=GeneratedMapping.PrepareParameterInvocations(FerruleValue.FromString("{\"projectCode\":\"P\",\"projectCode\":\"Q\",\"projectCode\":\"R\"}"));
var seen=new List<string>();var failure=new InvalidOperationException("host failure");
try {failing.Visit(item=>{seen.Add(item.Key);if(item.Key=="Q")throw failure;});throw new Exception("failure expected");}
catch(InvalidOperationException actual) {if(!ReferenceEquals(actual,failure))throw;}
if(!seen.SequenceEqual(new[]{"P","Q"})) throw new Exception("failure order");
"###;
