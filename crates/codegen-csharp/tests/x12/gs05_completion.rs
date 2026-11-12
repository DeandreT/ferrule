//! Independent full-wire oracles for all three generated strict X12 completion profiles.
//! No native completion or generated serializer constructs the expected bytes.
use super::*;
use codegen::X12EnvelopeProfile;
use serde_json::{Value as Json, json};

const HOST: &str = include_str!("gs05_completion/Host.cs");
const CONTEXT: &str = "2030-06-15T09:45:12Z";

fn fields(id: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        id,
        (1..=count)
            .map(|index| scalar(&format!("E{index:02}")))
            .collect(),
    )
}

fn schema(profile: X12EnvelopeProfile) -> SchemaNode {
    let mut isa = fields("ISA", 16);
    children_mut(&mut isa)[11].fixed = Some("00401".into());
    let mut gs = fields("GS", 8);
    children_mut(&mut gs)[7].fixed = Some("004010".into());
    let st = fields("ST", 2);
    let body = fields("W05", 2);
    let se = fields("SE", 2);
    let ge = fields("GE", 2);
    let iea = fields("IEA", 2);
    match profile {
        X12EnvelopeProfile::SingleTransaction => {
            SchemaNode::group("Envelope", vec![isa, gs, st, body, se, ge, iea])
        }
        X12EnvelopeProfile::GroupedTransactions => SchemaNode::group(
            "Envelope",
            vec![
                isa,
                SchemaNode::group(
                    "Groups",
                    vec![
                        gs,
                        SchemaNode::group("Messages", vec![st, body, se]).repeating(),
                        ge,
                    ],
                )
                .repeating(),
                iea,
            ],
        ),
        X12EnvelopeProfile::MapForceInterchanges => SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchanges",
                    vec![
                        isa.repeating(),
                        SchemaNode::group(
                            "Groups",
                            vec![
                                gs.repeating(),
                                SchemaNode::group("Messages", vec![st, body, se.repeating()])
                                    .repeating(),
                                ge.repeating(),
                            ],
                        )
                        .repeating(),
                        iea.repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
    }
}

fn segment(values: &[&str]) -> Json {
    Json::Object(
        values
            .iter()
            .enumerate()
            .map(|(index, value)| (format!("E{:02}", index + 1), json!(value)))
            .collect(),
    )
}

fn values(profile: X12EnvelopeProfile) -> Json {
    let isa = segment(&[
        "00",
        "",
        "00",
        "",
        "ZZ",
        "GS05-SENDER",
        "ZZ",
        "GS05-RECEIVER",
        "300615",
        "0945",
        "U",
        "00401",
        "000000123",
        "0",
        "T",
        ">",
    ]);
    let gs = segment(&[
        "OW", "SENDER", "RECEIVER", "20300615", "123709", "1", "X", "004010",
    ]);
    let st = segment(&["940", "0001"]);
    let body = segment(&["N", "GS05-CONTROL"]);
    let se = segment(&["3", "0001"]);
    let ge = segment(&["1", "1"]);
    let iea = segment(&["1", "000000123"]);
    match profile {
        X12EnvelopeProfile::SingleTransaction => {
            json!({"ISA":isa,"GS":gs,"ST":st,"W05":body,"SE":se,"GE":ge,"IEA":iea})
        }
        X12EnvelopeProfile::GroupedTransactions => json!({
            "ISA":isa,"Groups":[{"GS":gs,"Messages":[{"ST":st,"W05":body,"SE":se}],"GE":ge}],"IEA":iea
        }),
        X12EnvelopeProfile::MapForceInterchanges => json!({"Interchanges":[{
            "ISA":[isa],"Groups":[{"GS":[gs],"Messages":[{"ST":st,"W05":body,"SE":[se]}],"GE":[ge]}],"IEA":[iea]
        }]}),
    }
}

fn set_time(value: &mut Json, segment_id: &str, field: &str, mode: &str, text: &str) {
    match value {
        Json::Object(fields) => {
            if let Some(segment) = fields.get_mut(segment_id) {
                let segment = if segment.is_array() {
                    &mut segment.as_array_mut().unwrap()[0]
                } else {
                    segment
                };
                let fields = segment.as_object_mut().unwrap();
                match mode {
                    "missing" => {
                        fields.remove(field);
                    }
                    "null" => {
                        fields.insert(field.into(), Json::Null);
                    }
                    _ => {
                        fields.insert(field.into(), json!(text));
                    }
                }
            } else {
                for child in fields.values_mut() {
                    set_time(child, segment_id, field, mode, text);
                }
            }
        }
        Json::Array(items) => {
            for item in items {
                set_time(item, segment_id, field, mode, text);
            }
        }
        _ => {}
    }
}

/// Literal envelope ownership/counts and punctuation, with only the two time fields varied.
fn expected_wire(gs05: &str, isa10: &str) -> String {
    format!(
        "ISA*00*          *00*          *ZZ*GS05-SENDER    *ZZ*GS05-RECEIVER  *300615*{isa10}*U*00401*000000123*0*T*>~\n\
         GS*OW*SENDER*RECEIVER*20300615*{gs05}*1*X*004010~\n\
         ST*940*0001~\nW05*N*GS05-CONTROL~\nSE*3*0001~\nGE*1*1~\nIEA*1*000000123~\n"
    )
}

fn cases(profile: X12EnvelopeProfile) -> Vec<Json> {
    let mut cases = Vec::new();
    for (label, segment_id, field, mode, text, gs05, isa10) in [
        ("gs-four", "GS", "E05", "text", "1237", "1237", "0945"),
        ("gs-six", "GS", "E05", "text", "123709", "123709", "0945"),
        (
            "gs-seven", "GS", "E05", "text", "1237091", "1237091", "0945",
        ),
        (
            "gs-eight", "GS", "E05", "text", "12370912", "12370912", "0945",
        ),
        (
            "gs-colon-four",
            "GS",
            "E05",
            "text",
            "12:37",
            "1237",
            "0945",
        ),
        (
            "gs-colon-offset",
            "GS",
            "E05",
            "text",
            "12:37:09-07:00",
            "123709",
            "0945",
        ),
        ("gs-missing", "GS", "E05", "missing", "", "094512", "0945"),
        ("gs-null", "GS", "E05", "null", "", "094512", "0945"),
        ("gs-empty", "GS", "E05", "text", "", "094512", "0945"),
        ("isa-four", "ISA", "E10", "text", "1237", "123709", "1237"),
        (
            "isa-six-truncated",
            "ISA",
            "E10",
            "text",
            "123709",
            "123709",
            "1237",
        ),
    ] {
        let mut input = values(profile);
        set_time(&mut input, segment_id, field, mode, text);
        // Missing/null values enter the typed public serializer directly. The
        // ordinary JSON source parser need not admit incomplete required fields.
        cases.push(
            json!({"label":label,"input":input,"execute_json":mode=="text",
            "expected_wire":expected_wire(gs05,isa10)}),
        );
    }
    for (label, segment_id, field, text) in [
        ("gs-three-invalid", "GS", "E05", "123"),
        ("gs-five-invalid", "GS", "E05", "12370"),
        ("gs-nine-invalid", "GS", "E05", "123709123"),
        ("gs-nondigit-invalid", "GS", "E05", "12x7"),
        ("isa-five-invalid", "ISA", "E10", "12370"),
        ("isa-seven-invalid", "ISA", "E10", "1237091"),
        ("isa-eight-invalid", "ISA", "E10", "12370912"),
    ] {
        let mut input = values(profile);
        set_time(&mut input, segment_id, field, "text", text);
        cases.push(json!({"label":label,"input":input,"execute_json":true,
            "expected_error":{"type":"Ferrule.Runtime.FerruleX12Exception","error":"Envelope",
                "message":"X12 completion requires valid header dates and times.",
                "segment_index":null,"field_path":null}}));
    }
    cases
}

fn dotnet(root: &Path, label: &str, arguments: &[&str]) -> std::process::Output {
    retain_command(
        root,
        label,
        Command::new("dotnet")
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_CLI_HOME", root.join(".dotnet-home"))
            .env("NUGET_PACKAGES", root.join(".nuget"))
            .env("NUGET_HTTP_CACHE_PATH", root.join(".nuget-http"))
            .env("NUGET_PLUGINS_CACHE_PATH", root.join(".nuget-plugins"))
            .dotnet_temp(root)
            .args(arguments)
            .current_dir(root),
    )
}

#[test]
fn gs05_completion_generated_csharp_preserves_standard_time_widths_in_all_profiles() {
    let root = TempDir::new("gs05-completion").unwrap();
    eprintln!("complete GS05 generated-host originals: {}", root.display());
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
    let mut libraries = Vec::new();
    for (name, profile) in [
        ("Single", X12EnvelopeProfile::SingleTransaction),
        ("Grouped", X12EnvelopeProfile::GroupedTransactions),
        ("MapForce", X12EnvelopeProfile::MapForceInterchanges),
    ] {
        let schema = schema(profile);
        let program = lower(&mapforce::identity(&schema));
        let policy = X12BoundaryPolicy {
            source: None,
            target: Some(X12BoundaryOptions {
                envelope_profile: profile,
                interchange_version: Some("00401".into()),
                // MapForce writes raw values before completion. Keep ':' out of
                // the physical syntax so colon-form times reach the normalizer.
                separators: Some(X12Separators {
                    element: '*',
                    component: '>',
                    segment: '~',
                    repetition: None,
                    release: None,
                }),
                autocomplete: Some(mapping::X12Autocomplete {
                    request_acknowledgement: false,
                    transaction_set: Some("940".into()),
                }),
                ..Default::default()
            }),
        };
        fs::write(
            root.join(format!("{name}-program.original.txt")),
            format!("{program:#?}\n{policy:#?}"),
        )
        .unwrap();
        fs::write(
            root.join(format!("{name}-schema.json")),
            serde_json::to_vec_pretty(&schema).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join(format!("{name}-cases.json")),
            serde_json::to_vec_pretty(&cases(profile)).unwrap(),
        )
        .unwrap();
        let original = codegen_csharp::emit_with_x12(&program, &policy);
        fs::write(
            root.join(format!("{name}-emission.original.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        let mut completion_source_seen = false;
        for artifact in original.unwrap().files() {
            if let Some(relative) = artifact.path.as_str().strip_prefix("Runtime/X12/") {
                let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../runtime/csharp/Ferrule.Runtime/X12")
                    .join(relative);
                assert_eq!(
                    artifact.contents,
                    fs::read(source).unwrap(),
                    "fresh emitted runtime: {relative}"
                );
                completion_source_seen |= relative == "FerruleX12.Completion.cs";
            }
            let destination = root.join(name).join(artifact.path.as_str());
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(destination, &artifact.contents).unwrap();
        }
        assert!(
            completion_source_seen,
            "generated library must include completion runtime"
        );
        let project = format!("{name}/Ferrule.Generated.csproj");
        let assembly = format!("-p:AssemblyName=Gs05{name}");
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
            "{name}: build originals at {}",
            root.display()
        );
        libraries.push(json!({"name":name,"assembly":format!("Gs05{name}"),"context":CONTEXT}));
    }
    fs::write(
        root.join("libraries.json"),
        serde_json::to_vec_pretty(&libraries).unwrap(),
    )
    .unwrap();
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
        ],
    );
    assert!(
        output.status.success(),
        "host build originals at {}",
        root.display()
    );
    let output = dotnet(
        &root,
        "host-run",
        &["Harness/bin/Release/net10.0/Harness.dll"],
    );
    assert!(
        output.status.success(),
        "host outcomes at {}",
        root.display()
    );
    let summary: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["cases"], 54);
    assert_eq!(summary["calls"], 204);
    root.close().unwrap();
}
