//! Integration probes keep the expected legacy/native behavior unchanged.
use super::*;
use serde_json::json;

fn evidence(name: &str) -> PathBuf {
    generated_test_directory(&format!("native-r3-{name}"))
}

fn observable(fixture: &Fixture) -> Result<serde_json::Value, String> {
    let imported = fixture.import().map_err(|error| error.to_string())?;
    let target = imported
        .targets
        .iter()
        .find(|target| target.name == "Target")
        .unwrap();
    engine::run(&target.body, &group(vec![("BEG", group(vec![]))]))
        .map(|value| to_json(&value))
        .map_err(|error| error.to_string())
}

#[test]
fn unrelated_target_must_not_change_legacy_absent_root_behavior() {
    let baseline = Fixture::new("", &edge(21, 71), None);
    let changed = Fixture::new(
        &unary("db", "is-not-null", 101, 102, 20),
        &(edge(21, 71) + &edge(21, 101) + &edge(102, 81) + &edge(4, 80)),
        None,
    );
    changed.change("mapping.mfd", "</children>", r#"<component name="OtherTarget" library="json" kind="31" uid="30"><properties DynamicFilenames="1"/><data><root><entry name="FileInstance" inpkey="80"><entry name="document" casttotargettypemode="cast-in-subtree"><entry name="root"><entry name="object"><entry name="present" type="json-property"><entry name="boolean" inpkey="81"/></entry></entry></entry></entry></entry></root><json schema="target.json"/></data></component></children>"#);
    let before = observable(&baseline);
    let after = observable(&changed);
    std::fs::write(
        evidence("unrelated-target").join("unrelated-target.json"),
        serde_json::to_vec_pretty(&json!({"baseline":before,"with_other_target":after})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        after, before,
        "adding another output must not change Target's legacy root read"
    );
}

#[test]
fn unreachable_component_must_not_change_legacy_absent_root_behavior() {
    let baseline = Fixture::new("", &edge(21, 71), None);
    let functions =
        unary("db", "is-not-null", 101, 102, 20) + &unary("core", "boolean", 103, 104, 21);
    let changed = Fixture::new(
        &functions,
        &(edge(21, 71) + &edge(21, 101) + &edge(102, 103)),
        None,
    );
    let before = observable(&baseline);
    let after = observable(&changed);
    std::fs::write(
        evidence("unreachable-component").join("unreachable-component.json"),
        serde_json::to_vec_pretty(&json!({"baseline":before,"with_unreachable_component":after}))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        after, before,
        "unreachable scalar metadata must not alter the legacy target"
    );
}

fn native_fixture(candidate: bool) -> Fixture {
    let fixture = if candidate {
        Fixture::new(
            &unary("db", "is-not-null", 101, 102, 20),
            &(edge(21, 71) + &edge(21, 101) + &edge(102, 72)),
            None,
        )
    } else {
        Fixture::new("", &edge(21, 71), None)
    };
    fixture.change(
        "850.Config",
        r#"<Segment ref="ST" minOccurs="0"/>"#,
        r#"<Segment ref="ST"/>"#,
    );
    fixture.change(
        "850.Config",
        r#"<Segment ref="SE" minOccurs="0"/>"#,
        r#"<Segment ref="SE"/>"#,
    );
    for (name, count) in [
        ("ISA", 16),
        ("GS", 8),
        ("ST", 2),
        ("SE", 2),
        ("GE", 2),
        ("IEA", 2),
    ] {
        let old = format!(
            "<Segment name=\"{name}\"><Data ref=\"{}\"/></Segment>",
            if name == "ST" { "F143" } else { "F1" }
        );
        let fields = (1..=count)
            .map(|index| format!("<Data ref=\"F1\" nodeName=\"E{index:02}\"/>"))
            .collect::<String>();
        fixture.change(
            "Defs.Segment",
            &old,
            &format!("<Segment name=\"{name}\">{fields}</Segment>"),
        );
    }
    fixture.change("Envelope.Config", "ST/F143", "ST/E01");
    fixture.change(
        "mapping.mfd",
        "<messages>",
        r#"<settings interchangecontrolversionnumber="00401"/><messages>"#,
    );
    fixture
}

fn emit_native(
    fixture: &Fixture,
    root: &std::path::Path,
) -> (
    Result<codegen::ArtifactSet, String>,
    Result<codegen::ArtifactSet, String>,
) {
    let imported = fixture.import().unwrap();
    let document = imported
        .eager_json_document("Target", "ResolvedFilename")
        .unwrap();
    let lowered = codegen::lower_document(&document).unwrap();
    let source = codegen::NativeX12Source {
        schema: &imported.source_schema,
        options: &imported.source_options,
        message: &imported.message_path,
        encoding: imported.source_encoding,
        structure: Some(&imported.source_structure),
    };
    let options = codegen_rust::Options {
        package_name: "mfd051_native_control_r3".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .to_string_lossy()
                .into_owned(),
        ),
    };
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();
    let rust = codegen_rust::emit_with_native_primary_eager_x12_json(
        &lowered,
        &options,
        &source,
        "ResolvedFilename",
        &imported.targets[0].driver_path,
    )
    .map_err(|error| error.to_string());
    let csharp = codegen_csharp::emit_with_native_primary_eager_x12_json(
        &lowered,
        &source,
        "ResolvedFilename",
        &imported.targets[0].driver_path,
    )
    .map_err(|error| error.to_string());
    (rust, csharp)
}

#[test]
fn ordinary_native_scalar_entry_remains_closed() {
    let root = evidence("ordinary-emitter").join("candidate");
    std::fs::create_dir_all(&root).unwrap();
    let (rust, csharp) = emit_native(&native_fixture(true), &root);
    std::fs::write(root.join("emission.json"),serde_json::to_vec_pretty(&json!({"rust":rust.as_ref().map(|a|a.len()).map_err(Clone::clone),"csharp":csharp.as_ref().map(|a|a.len()).map_err(Clone::clone)})).unwrap()).unwrap();
    assert!(
        rust.is_err(),
        "ordinary Rust emission must not acquire the native-only capability"
    );
    assert!(
        csharp.is_err(),
        "ordinary C# emission must not acquire the native-only capability"
    );
}

#[test]
fn full_native_legacy_control_emits_compilable_convenience_entries() {
    let root = evidence("legacy-control").join("control");
    std::fs::create_dir_all(&root).unwrap();
    let (rust, csharp) = emit_native(&native_fixture(false), &root);
    for (name, artifact) in [("rust", rust.unwrap()), ("csharp", csharp.unwrap())] {
        let target = root.join(name);
        std::fs::create_dir(&target).unwrap();
        for file in artifact.files() {
            let path = target.join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &file.contents).unwrap();
        }
        if name == "rust" {
            let manifest = target.join("Cargo.toml");
            let text = std::fs::read_to_string(&manifest)
                .unwrap()
                .replace("[dependencies]\n", "[dependencies]\nserde_json = \"1\"\n");
            std::fs::write(manifest, text).unwrap();
            std::fs::write(
                target.join("src/main.rs"),
                include_str!("../fixtures/x12_document_body_scalars/NativeHost.rs.txt"),
            )
            .unwrap();
        } else {
            let harness = target.join("Harness");
            std::fs::create_dir(&harness).unwrap();
            std::fs::write(harness.join("Host.csproj"),r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
            std::fs::write(
                harness.join("Program.cs"),
                include_str!("../fixtures/x12_document_body_scalars/NativeHost.cs.txt"),
            )
            .unwrap();
            std::fs::write(
                target.join("NuGet.Config"),
                "<configuration><packageSources><clear /></packageSources></configuration>",
            )
            .unwrap();
        }
    }
    let bytes=b"ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261024*1200*U*00401*000000001*0*T*>~GS*PO*SENDER*RECEIVER*20261024*1200*1*X*004010~ST*850*0001~BEG*A~LIN*ROW1*1~SE*4*0001~ST*850*0002~BEG*E~LIN*ROW2*1~SE*4*0002~ST*850*0003~BEG*X~LIN*ROW3*1~SE*4*0003~GE*3*1~IEA*1*000000001~";
    assert_eq!(bytes.iter().position(|byte| *byte == b'~'), Some(105));
    std::fs::write(root.join("input.x12"), bytes).unwrap();
    std::fs::write(root.join("expected.json"),serde_json::to_vec_pretty(&json!([{"path":"A.json","value":{"result":"A"}},{"path":"E.json","value":{"result":"E"}},{"path":"X.json","value":{"result":"X"}}])).unwrap()).unwrap();
}
