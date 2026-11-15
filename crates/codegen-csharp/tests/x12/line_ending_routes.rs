//! Modest compiled-route smoke test sources; execution requires a separate grant.
use super::line_ending_fixture as toy;
use super::*;
use codegen::{
    Binding as CodeBinding, CsvInputPolicy, CsvX12BoundaryPolicy,
    DocumentBoundaryOptions as Boundary, Expression, ExpressionNode, IterationPlan,
    NamedDocumentBoundaryOptions, StaticDocumentBoundaryPolicy, TargetConstruction, TargetScope,
    X12EnvelopeProfile,
};
use mapping::X12SegmentLineEnding as Ending;

fn group(name: &str) -> TargetScope {
    TargetScope {
        target_field: name.into(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: vec![],
        children: vec![],
    }
}

fn bind(program: &mut Program, scope: &mut TargetScope, field: &str, expression: Expression) {
    let id = u32::try_from(program.expressions.len() + 1).unwrap();
    program.expressions.push(ExpressionNode { id, expression });
    scope.bindings.push(CodeBinding {
        target_field: field.into(),
        expression: id,
        target_domain: ScalarType::String.into(),
        repeating: false,
    });
}

fn csv_program() -> Program {
    let envelope = X12EnvelopeProfile::SingleTransaction;
    let mut program = codegen::lower(&toy::project(envelope)).unwrap();
    // CSV admission expects one canonical non-repeating row schema. The input
    // reader supplies the row sequence; only the NTE scope iterates those rows.
    program.source = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("A", ScalarType::String),
            SchemaNode::scalar("C", ScalarType::String),
        ],
    );
    program.expressions.clear();
    let constants = toy::values(envelope, true);
    let mut root = group("");
    for name in ["ISA", "GS", "ST", "NTE", "SE", "GE", "IEA"] {
        let mut segment = group(name);
        if name == "NTE" {
            segment.repeating = true;
            segment.iteration = Some(IterationPlan::source(vec![]));
            for (field, source) in [("E01", "A"), ("E03", "C")] {
                bind(
                    &mut program,
                    &mut segment,
                    field,
                    Expression::SourceField {
                        frame: None,
                        path: vec![source.into()],
                    },
                );
            }
        } else {
            for (field, value) in constants[name].as_object().unwrap() {
                bind(
                    &mut program,
                    &mut segment,
                    field,
                    Expression::Const {
                        value: ir::Value::String(value.as_str().unwrap().into()),
                    },
                );
            }
        }
        root.children.push(segment);
    }
    program.root = root;
    program
}

fn emit_case(
    root: &Path,
    name: &str,
    namespace: &str,
    class: &str,
    artifacts: &codegen::ArtifactSet,
    selected: bool,
) {
    assert_eq!(
        artifacts
            .files()
            .iter()
            .filter(|file| file.path.as_str() == "Runtime/X12/FerruleX12.OutputPolicy.cs")
            .count(),
        usize::from(selected)
    );
    for file in artifacts.files() {
        let destination = root.join(name).join(file.path.as_str());
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, &file.contents).unwrap();
    }
    fs::write(
        root.join(format!("{name}.map")),
        format!("{namespace}.{class}"),
    )
    .unwrap();
    let project = format!("{name}/Ferrule.Generated.csproj");
    let assembly = format!("-p:AssemblyName=EndingRoute{name}");
    let result = dotnet(
        root,
        &format!("{name}-build"),
        &[
            "build",
            &project,
            "-c",
            "Release",
            "--configfile",
            "NuGet.Config",
            "-warnaserror",
            "-m:1",
            "-nr:false",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
            "-p:EnableTargetingPackDownload=false",
            "-p:EnableRuntimePackDownload=false",
            &assembly,
        ],
    );
    assert!(
        result.status.success(),
        "route build originals: {}",
        root.display()
    );
}

#[test]
fn csv_and_mixed_static_line_endings_release_public_routes() {
    let root = TempDir::new("x12-line-ending-routes").unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
        root.join("global.json"),
    )
    .unwrap();
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    fs::write(root.join("wire-lf.expected"), toy::expected("\n")).unwrap();
    fs::write(root.join("wire-crlf.expected"), toy::expected("\r\n")).unwrap();
    fs::write(root.join("input.csv"), "alpha,omega\nbeta,\n").unwrap();
    fs::write(root.join("input.x12"), toy::expected("\n")).unwrap();
    let mut cases = vec![];
    for (name, ending) in [("CsvLf", Ending::Lf), ("CsvCrLf", Ending::CrLf)] {
        let program = csv_program();
        let policy = CsvX12BoundaryPolicy {
            source: CsvInputPolicy {
                has_headers: false,
                preserve_empty_strings: true,
                ..Default::default()
            },
            target: X12BoundaryOptions {
                segment_line_ending: ending,
                interchange_version: Some("00401".into()),
                ..Default::default()
            },
        };
        // First namespace segment is intentional: compile the selected helper
        // beside the precise private-name sentinel, not merely a later segment.
        let namespace = if ending.is_lf() {
            "Ferrule.Generated"
        } else {
            "SegmentOutputProfile.Toy"
        };
        let class = if ending.is_lf() {
            "GeneratedMapping"
        } else {
            "ToyMapping"
        };
        let identity = if ending.is_lf() {
            Default::default()
        } else {
            codegen_csharp::Identity::new("Ferrule.Generated", namespace, class).unwrap()
        };
        let options = codegen_csharp::Options {
            identity,
            manifest: codegen::ManifestSelection::Included,
        };
        let artifacts = codegen_csharp::Generator::new(&options)
            .emit_with_csv_x12_adapters(&program, &policy)
            .unwrap();
        emit_case(&root, name, namespace, class, &artifacts, !ending.is_lf());
        cases.push(serde_json::json!({"name":name,"route":"csv","primaryCrLf":!ending.is_lf()}));
    }
    for (name, primary, named) in [
        ("StaticPrimaryLf", Ending::Lf, Ending::CrLf),
        ("StaticPrimaryCrLf", Ending::CrLf, Ending::Lf),
    ] {
        let mut project = toy::project(X12EnvelopeProfile::SingleTransaction);
        project.extra_targets.push(mapping::NamedTarget {
            name: "Other".into(),
            path: None,
            schema: project.target.clone(),
            options: Default::default(),
            root: project.root.clone(),
        });
        let program = codegen::lower(&project).unwrap();
        let target = |ending| {
            Boundary::X12(X12BoundaryOptions {
                segment_line_ending: ending,
                interchange_version: Some("00401".into()),
                ..Default::default()
            })
        };
        let policy = StaticDocumentBoundaryPolicy {
            source: target(Ending::Lf),
            target: target(primary),
            extra_sources: vec![],
            extra_targets: vec![NamedDocumentBoundaryOptions {
                name: "Other".into(),
                options: target(named),
            }],
        };
        let options = codegen_csharp::Options {
            manifest: codegen::ManifestSelection::Included,
            ..Default::default()
        };
        let artifacts = codegen_csharp::Generator::new(&options)
            .emit_with_static_document_adapters(&program, &policy)
            .unwrap();
        emit_case(
            &root,
            name,
            "Ferrule.Generated",
            "GeneratedMapping",
            &artifacts,
            true,
        );
        cases.push(serde_json::json!({"name":name,"route":"static","primaryCrLf":!primary.is_lf(),"namedCrLf":!named.is_lf()}));
    }
    fs::write(
        root.join("route-cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    fs::create_dir(root.join("Harness")).unwrap();
    fs::write(root.join("Harness/Program.cs"), HOST).unwrap();
    fs::write(root.join("Harness/Harness.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup></Project>\n").unwrap();
    let result = dotnet(
        &root,
        "routes-host-build",
        &[
            "build",
            "Harness/Harness.csproj",
            "-c",
            "Release",
            "--configfile",
            "NuGet.Config",
            "-warnaserror",
            "-m:1",
            "-nr:false",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
            "-p:EnableTargetingPackDownload=false",
            "-p:EnableRuntimePackDownload=false",
        ],
    );
    assert!(
        result.status.success(),
        "route host build originals: {}",
        root.display()
    );
    let result = dotnet(
        &root,
        "routes-host-run",
        &["Harness/bin/Release/net10.0/Harness.dll"],
    );
    assert!(
        result.status.success(),
        "route public call originals: {}",
        root.display()
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("line-ending CSV/static calls completed: 64")
    );
    root.close().unwrap();
}

fn dotnet(root: &Path, label: &str, args: &[&str]) -> std::process::Output {
    let mut command = Command::new("dotnet");
    command
        .args(args)
        .current_dir(root)
        .dotnet_temp(root)
        .env("DOTNET_CLI_HOME", root.join(".dotnet-home"))
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1");
    retain_command(root, label, &mut command)
}

const HOST: &str = r#"
using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json.Nodes;
var utf8=new UTF8Encoding(false,true);
int calls=0;
object Call(Type type,string name,params object?[] args) {
    var method=type.GetMethods(BindingFlags.Public|BindingFlags.Static).Single(m=>m.Name==name&&m.GetParameters().Length==args.Length);
    try { return method.Invoke(null,args)!; }
    catch(TargetInvocationException error) when(error.InnerException is not null) { throw error.InnerException; }
}
object Property(object value,string name)=>value.GetType().GetProperty(name)!.GetValue(value)!;
void Equal(object actual,string expected) {
    var bytes=actual switch { string text=>utf8.GetBytes(text),byte[] raw=>raw,_=>throw new Exception("unexpected wire type") };
    if(!bytes.AsSpan().SequenceEqual(utf8.GetBytes(expected)))throw new Exception("independent wire mismatch");
}
void X12(object format) { if(format.ToString()!="X12")throw new Exception("unexpected physical format"); }
void Selected(object result,bool named,string expected) {
    if(result.GetType().Name!=(named?"Named":"Primary"))throw new Exception("incorrect selected owner");
    object output=named?Property(result,"Output"):result;
    if(named&&(string)Property(output,"Name")!="Other")throw new Exception("incorrect named owner");
    X12(Property(output,"Format"));Equal(Property(output,"Document"),expected);calls++;
}
void Complete(object result,string primary,string named) {
    X12(Property(result,"PrimaryFormat"));Equal(Property(result,"Primary"),primary);
    var extras=((IEnumerable)Property(result,"Extras")).Cast<object>().ToArray();
    if(extras.Length!=1||(string)Property(extras[0],"Name")!="Other")throw new Exception("complete output order/membership changed");
    X12(Property(extras[0],"Format"));Equal(Property(extras[0],"Document"),named);calls++;
}
string lf=File.ReadAllText("wire-lf.expected"),crlf=File.ReadAllText("wire-crlf.expected");
foreach(var entry in JsonNode.Parse(File.ReadAllText("route-cases.json"))!.AsArray()) {
    string name=entry!["name"]!.GetValue<string>();
    string typeName=File.ReadAllText(name+".map");
    var assembly=Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/EndingRoute{name}.dll"));
    var map=assembly.GetType(typeName,true)!;
    var context=assembly.GetType("Ferrule.Runtime.FerruleExecutionContext",true)!.GetConstructors()
        .Single(c=>c.GetParameters().Length==4).Invoke(["toy.mfd","toy.mfd","2030-11-22T10:15:00Z",null]);
    string primary=entry["primaryCrLf"]!.GetValue<bool>()?crlf:lf;
    bool csv=entry["route"]!.GetValue<string>()=="csv";
    string input=File.ReadAllText(csv?"input.csv":"input.x12");
    byte[] bytes=utf8.GetBytes(input),before=(byte[])utf8.GetBytes(input).Clone();
    if(csv) {
        for(int round=0;round<2;round++) {
            Equal(Call(map,"ExecuteCsvToX12",input),primary);calls++;
            Equal(Call(map,"ExecuteCsvToX12Bytes",bytes),primary);calls++;
            Equal(Call(map,"ExecuteCsvToX12WithContext",input,context),primary);calls++;
            Equal(Call(map,"ExecuteCsvToX12BytesWithContext",bytes,context),primary);calls++;
        }
    } else {
        string named=entry["namedCrLf"]!.GetValue<bool>()?crlf:lf;
        if(primary==named)throw new Exception("mixed-policy fixture is not independent");
        string ns=typeName[..typeName.LastIndexOf('.')];
        var empty=Array.CreateInstance(assembly.GetType(ns+".NamedDocumentInput",true)!,0);
        var emptyBytes=Array.CreateInstance(assembly.GetType(ns+".NamedDocumentBytesInput",true)!,0);
        var selection=assembly.GetType("Ferrule.Runtime.FerruleTargetSelection",true)!;
        object first=Call(selection,"Primary"),other=Call(selection,"Named","Other");
        for(int round=0;round<2;round++) {
            Selected(Call(map,"ExecuteDocumentSelectedTarget",input,first),false,primary);
            Selected(Call(map,"ExecuteDocumentBytesSelectedTarget",bytes,first),false,primary);
            Selected(Call(map,"ExecuteDocumentSelectedTargetWithHost",input,first,empty,context),false,primary);
            Selected(Call(map,"ExecuteDocumentBytesSelectedTargetWithHost",bytes,first,emptyBytes,context),false,primary);
            Selected(Call(map,"ExecuteDocumentSelectedTarget",input,other),true,named);
            Selected(Call(map,"ExecuteDocumentBytesSelectedTarget",bytes,other),true,named);
            Selected(Call(map,"ExecuteDocumentSelectedTargetWithHost",input,other,empty,context),true,named);
            Selected(Call(map,"ExecuteDocumentBytesSelectedTargetWithHost",bytes,other,emptyBytes,context),true,named);
            Complete(Call(map,"ExecuteDocumentOutputs",input),primary,named);
            Complete(Call(map,"ExecuteDocumentBytesOutputs",bytes),primary,named);
            Complete(Call(map,"ExecuteDocumentOutputsWithHost",input,empty,context),primary,named);
            Complete(Call(map,"ExecuteDocumentBytesOutputsWithHost",bytes,emptyBytes,context),primary,named);
        }
    }
    if(!bytes.AsSpan().SequenceEqual(before))throw new Exception("route input bytes mutated");
}
if(calls!=64)throw new Exception($"route membership mismatch: {calls}");
Console.WriteLine($"line-ending CSV/static calls completed: {calls}");
"#;
