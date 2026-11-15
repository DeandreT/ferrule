use super::line_ending_fixture as toy;
use super::*;

#[test]
fn generated_line_endings_release_calls_and_selected_policy_parser() {
    use codegen::X12EnvelopeProfile as Profile;
    use mapping::X12SegmentLineEnding as Ending;
    let root = TempDir::new("x12-line-endings").unwrap();
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
    let mut cases = vec![];
    for (name, profile, ending, complete) in [
        ("SingleLf", Profile::SingleTransaction, Ending::Lf, false),
        (
            "SingleCrLf",
            Profile::SingleTransaction,
            Ending::CrLf,
            false,
        ),
        (
            "MapForceLf",
            Profile::MapForceInterchanges,
            Ending::Lf,
            false,
        ),
        (
            "MapForceCrLf",
            Profile::MapForceInterchanges,
            Ending::CrLf,
            false,
        ),
        (
            "MapForceCompletion",
            Profile::MapForceInterchanges,
            Ending::CrLf,
            true,
        ),
    ] {
        let program = codegen::lower(&toy::project(profile)).unwrap();
        let copies = if profile == Profile::MapForceInterchanges {
            2
        } else {
            1
        };
        let policy = toy::policy(profile, ending, complete);
        let descriptor = codegen::prepare_x12_boundary(&program, &policy).unwrap();
        let options = codegen_csharp::Options {
            manifest: codegen::ManifestSelection::Included,
            ..Default::default()
        };
        let artifacts = codegen_csharp::Generator::new(&options)
            .emit_with_x12(&program, &policy)
            .unwrap();
        assert_eq!(
            artifacts
                .files()
                .iter()
                .filter(|f| f.path.as_str() == "Runtime/X12/FerruleX12.OutputPolicy.cs")
                .count(),
            usize::from(!ending.is_lf())
        );
        for file in artifacts.files() {
            let destination = root.join(name).join(file.path.as_str());
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(destination, &file.contents).unwrap();
        }
        let input = if complete {
            let mut values = toy::values(profile, false);
            let owners = values["Interchanges"].as_array_mut().unwrap();
            let second = owners[0].clone();
            owners.push(second);
            values.to_string()
        } else {
            toy::expected("\n").repeat(copies)
        };
        fs::write(root.join(format!("{name}.input")), input).unwrap();
        fs::write(
            root.join(format!("{name}.expected")),
            toy::expected(ending.as_str()).repeat(copies),
        )
        .unwrap();
        fs::write(
            root.join(format!("{name}.descriptor")),
            descriptor.target_descriptor,
        )
        .unwrap();
        fs::write(
            root.join(format!("{name}.schema")),
            codegen::serialize_embedded_schema(&program.target, 1024 * 1024).unwrap(),
        )
        .unwrap();
        let project = format!("{name}/Ferrule.Generated.csproj");
        let assembly = format!("-p:AssemblyName=LineEnding{name}");
        let output = dotnet(
            &root,
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
            output.status.success(),
            "build originals: {}",
            root.display()
        );
        cases.push(serde_json::json!({"name":name,"completion":complete,"crlf":!ending.is_lf()}));
    }
    fs::write(root.join("cases.json"), serde_json::to_vec(&cases).unwrap()).unwrap();
    fs::create_dir(root.join("Harness")).unwrap();
    fs::write(root.join("Harness/Program.cs"), HOST).unwrap();
    fs::write(root.join("Harness/Harness.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup></Project>\n").unwrap();
    let output = dotnet(
        &root,
        "host-build",
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
        output.status.success(),
        "host originals: {}",
        root.display()
    );
    let output = dotnet(
        &root,
        "host-run",
        &["Harness/bin/Release/net10.0/Harness.dll"],
    );
    assert!(output.status.success(), "public calls: {}", root.display());
    assert!(String::from_utf8_lossy(&output.stdout).contains("line-ending public calls completed"));
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
using System.Reflection;
using System.Text;
using System.Text.Json.Nodes;
var utf8 = new UTF8Encoding(false, true);
object? Call(Type type, string name, params object?[] args) {
    var method=type.GetMethods(BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Static).Single(m=>m.Name==name&&m.GetParameters().Length==args.Length);
    try { return method.Invoke(null,args); } catch(TargetInvocationException e) when(e.InnerException is not null) { throw e.InnerException; }
}
void Equal(object? actual,string expected) {
    var bytes=actual switch { string text=>utf8.GetBytes(text),byte[] raw=>raw,_=>throw new Exception("unexpected result") };
    if(!bytes.AsSpan().SequenceEqual(utf8.GetBytes(expected)))throw new Exception("wire mismatch");
}
Exception Refused(Action action) { try { action(); } catch(Exception error) { return error; } throw new Exception("expected refusal"); }
Exception X12Refused(Action action,string expected) { var error=Refused(action);if(error.GetType().FullName!="Ferrule.Runtime.FerruleX12Exception"||error.GetType().GetProperty("Error")!.GetValue(error)!.ToString()!=expected)throw new Exception("wrong X12 refusal",error);return error; }
void PolicyRefused(Action action) { X12Refused(action,"Schema"); }
bool SetFirstNote(JsonNode node,string value) {
    if(node is JsonObject fields) {
        if(fields["NTE"] is JsonArray notes && notes.Count>0) { notes[0]!["E01"]=value;return true; }
        foreach(var entry in fields)if(entry.Value is not null && SetFirstNote(entry.Value,value))return true;
    } else if(node is JsonArray items) {
        foreach(var item in items)if(item is not null && SetFirstNote(item,value))return true;
    }
    return false;
}
void CompletionExpansionLimits(Type runtime,string descriptor,object document,object context,string expected) {
    foreach(int maximum in new[]{utf8.GetByteCount(expected),utf8.GetByteCount(expected)-1}) {
        object completed=Call(runtime,"SerializeOutput",descriptor,document,context)!;
        var builder=(StringBuilder)completed.GetType().GetProperty("Text")!.GetValue(completed)!;string before=builder.ToString();
        if(maximum==utf8.GetByteCount(expected)) {
            var result=Call(runtime,"ExpandSegmentLineEndings",completed,(long)maximum)!;
            Equal(result.GetType().GetProperty("Text")!.GetValue(result)!.ToString(),expected);
        } else {
            var error=Refused(()=>Call(runtime,"ExpandSegmentLineEndings",completed,(long)maximum));
            if(error.GetType().GetProperty("Error")!.GetValue(error)!.ToString()!="ResourceLimit"||builder.ToString()!=before)throw new Exception("completion limit changed builder",error);
        }
    }
}
foreach(var item in JsonNode.Parse(File.ReadAllText("cases.json"))!.AsArray()) {
    string name=item!["name"]!.GetValue<string>();bool completion=item["completion"]!.GetValue<bool>();
    var assembly=Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/LineEnding{name}.dll"));
    var map=assembly.GetType("Ferrule.Generated.GeneratedMapping",true)!;
    var runtime=assembly.GetType("Ferrule.Runtime.FerruleX12",true)!;
    var context=assembly.GetType("Ferrule.Runtime.FerruleExecutionContext",true)!.GetConstructors().Single(c=>c.GetParameters().Length==4).Invoke(["toy.mfd","toy.mfd","2030-11-22T10:15:00Z",null]);
    string input=File.ReadAllText(name+".input"), expected=File.ReadAllText(name+".expected");byte[] bytes=utf8.GetBytes(input), before=(byte[])utf8.GetBytes(input).Clone();
    if(completion) {
        var json=assembly.GetType("Ferrule.Runtime.FerruleJson",true)!;string schema=File.ReadAllText(name+".schema");
        object value=Call(json,"ParseEmbedded",schema,input)!;string snapshot=(string)Call(json,"SerializeEmbedded",schema,value)!;
        Refused(()=>Call(map,"ExecuteJsonToX12",input));Refused(()=>Call(map,"ExecuteJsonToX12Bytes",bytes));
        Refused(()=>Call(map,"SerializeX12",value));Refused(()=>Call(map,"SerializeX12Bytes",value));
        for(int round=0;round<2;round++) { Equal(Call(map,"ExecuteJsonToX12",input,context),expected);Equal(Call(map,"ExecuteJsonToX12Bytes",bytes,context),expected);Equal(Call(map,"SerializeX12",value,context),expected);Equal(Call(map,"SerializeX12Bytes",value,context),expected); }
        if((string)Call(json,"SerializeEmbedded",schema,value)! != snapshot)throw new Exception("completion mutated caller");
        var legacy=JsonNode.Parse(File.ReadAllText(name+".descriptor"))!;legacy.AsObject().Remove("segment_line_ending");
        CompletionExpansionLimits(runtime,legacy.ToJsonString(),value,context,expected);
    } else {
        object value=Call(map,"ParseX12",input)!;
        var json=assembly.GetType("Ferrule.Runtime.FerruleJson",true)!;string schema=File.ReadAllText(name+".schema");
        string snapshot=(string)Call(json,"SerializeEmbedded",schema,value)!;
        object parsedBytes=Call(map,"ParseX12Bytes",bytes)!;
        if((string)Call(json,"SerializeEmbedded",schema,parsedBytes)! != snapshot)throw new Exception("byte parse changed instance");
        for(int round=0;round<2;round++) {
            Equal(Call(map,"SerializeX12",value),expected);Equal(Call(map,"SerializeX12Bytes",value),expected);
            Equal(Call(map,"SerializeX12",value,context),expected);Equal(Call(map,"SerializeX12Bytes",value,context),expected);
            Equal(Call(map,"ExecuteX12ToX12",input),expected);Equal(Call(map,"ExecuteX12ToX12Bytes",bytes),expected);
            Equal(Call(map,"ExecuteX12ToX12",input,context),expected);Equal(Call(map,"ExecuteX12ToX12Bytes",bytes,context),expected);
        }
        if((string)Call(json,"SerializeEmbedded",schema,value)! != snapshot)throw new Exception("caller mutated");
        if(item["crlf"]!.GetValue<bool>()) {
            string descriptor=File.ReadAllText(name+".descriptor");
            int descriptorLimit=(int)runtime.GetField("MaximumSchemaBytes",BindingFlags.Public|BindingFlags.Static)!.GetRawConstantValue()!;
            if(descriptorLimit!=1024*1024)throw new Exception("unexpected descriptor byte ceiling");
            string exact=descriptor+new string(' ',descriptorLimit-utf8.GetByteCount(descriptor));
            Equal(Call(runtime,"SerializeEmbeddedWithLineEnding",exact,value),expected);
            Equal(Call(runtime,"SerializeEmbeddedBytesWithLineEnding",exact,value),expected);
            string oversized=exact+" ";
            X12Refused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",oversized,value),"ResourceLimit");
            X12Refused(()=>Call(runtime,"SerializeEmbeddedBytesWithLineEnding",oversized,value),"ResourceLimit");
            string deep=descriptor.Replace("\"constraints\":[]","\"constraints\":"+new string('[',128)+"0"+new string(']',128));
            if(deep==descriptor)throw new Exception("depth control was not constructed");
            var depthError=X12Refused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",deep,value),"Schema");
            if(depthError.Message!="Invalid embedded X12 output policy.")throw new Exception("depth did not fail at selected JSON parser",depthError);
            PolicyRefused(()=>Call(runtime,"SerializeEmbeddedBytesWithLineEnding",deep,value));
            string unknown="{\"unexpected_output_member\":true,"+descriptor[1..];
            PolicyRefused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",unknown,value));
            PolicyRefused(()=>Call(runtime,"SerializeEmbeddedBytesWithLineEnding",unknown,value));
            var explicitLf=JsonNode.Parse(descriptor)!;explicitLf["segment_line_ending"]="lf";
            string explicitLfDescriptor=explicitLf.ToJsonString();
            Equal(Call(runtime,"SerializeEmbeddedWithLineEnding",explicitLfDescriptor,value),input);
            Equal(Call(runtime,"SerializeEmbeddedBytesWithLineEnding",explicitLfDescriptor,value),input);
            Equal(Call(runtime,"SerializeEmbeddedWithLineEnding",explicitLfDescriptor,value,context),input);
            Equal(Call(runtime,"SerializeEmbeddedBytesWithLineEnding",explicitLfDescriptor,value,context),input);
            // Frozen legacy entrypoints still reject this noncanonical member.
            PolicyRefused(()=>Call(runtime,"SerializeEmbedded",explicitLfDescriptor,value));
            foreach(string control in new[]{"\n","\r"}) {
                var bad=JsonNode.Parse(snapshot)!;
                if(!SetFirstNote(bad,"alpha"+control))throw new Exception("note control was not constructed");
                object supplied=Call(json,"ParseEmbedded",schema,bad.ToJsonString())!;
                string suppliedBefore=(string)Call(json,"SerializeEmbedded",schema,supplied)!;
                X12Refused(()=>Call(map,"SerializeX12",supplied),"Value");
                X12Refused(()=>Call(map,"SerializeX12Bytes",supplied),"Value");
                X12Refused(()=>Call(map,"SerializeX12",supplied,context),"Value");
                X12Refused(()=>Call(map,"SerializeX12Bytes",supplied,context),"Value");
                if((string)Call(json,"SerializeEmbedded",schema,supplied)! != suppliedBefore)throw new Exception("field-control refusal mutated caller");
            }
            foreach(string raw in new[]{"null","true","1","[]","{}","{\"lf\":null}","\"CRLF\"","\"unknown\""}) {
                string bad=descriptor.Replace("\"segment_line_ending\":\"crlf\"","\"segment_line_ending\":"+raw);
                PolicyRefused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",bad,value));
            }
            PolicyRefused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding","{\"segment_line_ending\":\"lf\","+descriptor[1..],value));
            string nested=descriptor.Replace("\"separators\":null","\"separators\":{\"element\":\"*\",\"element\":\"!\",\"component\":\":\",\"segment\":\"~\"}");
            PolicyRefused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",nested,value));
            var old=JsonNode.Parse(descriptor)!;old.AsObject().Remove("segment_line_ending");string legacy=old.ToJsonString();
            Equal(Call(runtime,"SerializeEmbeddedWithLineEnding",legacy,value),input);
            old["schema"]="broken";var prior=Refused(()=>Call(runtime,"SerializeEmbedded",old.ToJsonString(),value));old["segment_line_ending"]=null;
            var selected=Refused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",old.ToJsonString(),value));
            if(prior.Message!=selected.Message)throw new Exception("old validation precedence changed");
            var conflict=JsonNode.Parse(descriptor)!;conflict["separators"]=new JsonObject { ["element"]="*",["component"]=":",["segment"]="\n" };
            PolicyRefused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",conflict.ToJsonString(),value));
            conflict["separators"]!["segment"]="\r";
            X12Refused(()=>Call(runtime,"SerializeEmbeddedWithLineEnding",conflict.ToJsonString(),value),"Syntax");
            X12Refused(()=>Call(runtime,"SerializeEmbeddedBytesWithLineEnding",conflict.ToJsonString(),value),"Syntax");
            foreach(int maximum in new[]{utf8.GetByteCount(expected),utf8.GetByteCount(expected)-1}) {
                object completed=Call(runtime,"SerializeOutput",legacy,value,null)!;
                var builder=(StringBuilder)completed.GetType().GetProperty("Text")!.GetValue(completed)!;string original=builder.ToString();
                if(maximum==utf8.GetByteCount(expected)) {
                    object expanded=Call(runtime,"ExpandSegmentLineEndings",completed,(long)maximum)!;
                    Equal(expanded.GetType().GetProperty("Text")!.GetValue(expanded)!.ToString(),expected);
                } else { var error=Refused(()=>Call(runtime,"ExpandSegmentLineEndings",completed,(long)maximum));if(error.GetType().GetProperty("Error")!.GetValue(error)!.ToString()!="ResourceLimit")throw new Exception("wrong limit refusal",error);if(builder.ToString()!=original)throw new Exception("expanded before byte refusal"); }
            }
        }
        if((string)Call(json,"SerializeEmbedded",schema,value)! != snapshot)throw new Exception("failed call mutated input");
    }
    if(!bytes.AsSpan().SequenceEqual(before))throw new Exception("byte input mutated");
}
Console.WriteLine("line-ending public calls completed");
"#;
