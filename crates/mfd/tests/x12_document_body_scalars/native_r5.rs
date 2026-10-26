use super::*;

#[test]
fn emit_duplicate_frame_guard_with_unchosen_nullable_read() {
    let f = super::native_r4::fixture();
    // Keep a nullable root read in a false branch. A late reader failure is not
    // an adequate guard: this body otherwise samples clocks and advances rows.
    f.change(
        "mapping.mfd",
        r#"<edge vertexkey="71"/>"#,
        r#"<edge vertexkey="206"/>"#,
    );
    for input in [75, 72, 76] {
        f.change("mapping.mfd", &format!("<edge vertexkey=\"{input}\"/>"), "");
    }
    let components = r#"<component name="constant" library="core" kind="2" uid="30"><targets><datapoint pos="0" key="200"/></targets><data><constant value="0" datatype="decimal"/></data></component><component name="boolean" library="core" kind="5" uid="31"><sources><datapoint pos="0" key="201"/></sources><targets><datapoint pos="0" key="202"/></targets></component><component name="constant" library="core" kind="2" uid="32"><targets><datapoint pos="0" key="203"/></targets><data><constant value="safe" datatype="string"/></data></component><component name="if-else" library="core" kind="4" uid="33"><sources><datapoint pos="0" key="204"/><datapoint pos="1" key="206"/><datapoint pos="2" key="207"/></sources><targets><datapoint pos="0" key="208"/></targets></component><component name="boolean" library="core" kind="5" uid="34"><sources><datapoint pos="0" key="209"/></sources><targets><datapoint pos="0" key="210"/></targets></component>"#;
    f.change(
        "mapping.mfd",
        "</children>",
        &format!("{components}</children>"),
    );
    let edges = edge(200, 201)
        + &edge(202, 204)
        + &edge(203, 207)
        + &edge(208, 71)
        + &edge(208, 75)
        + &edge(202, 72)
        + &edge(202, 76)
        + &edge(104, 209);
    f.change("mapping.mfd", "</vertices>", &format!("{edges}</vertices>"));
    let imported = f.import().unwrap();
    let document = imported
        .eager_json_document("Target", "ResolvedFilename")
        .unwrap();
    let source = codegen::NativeX12Source {
        schema: &imported.source_schema,
        options: &imported.source_options,
        message: &imported.message_path,
        encoding: imported.source_encoding,
        structure: Some(&imported.source_structure),
    };
    assert!(
        codegen::lower_document(&document).is_err(),
        "ordinary lowering must remain closed"
    );
    let lowered = codegen::lower_native_x12_nullable_document(&document, &source).unwrap();
    assert!(
        lowered
            .program()
            .expressions
            .iter()
            .any(|node| matches!(node.expression, codegen::Expression::OwnedSourceField(_))),
        "the false branch must retain the nullable read"
    );
    let options = codegen_rust::Options {
        package_name: "mfd051_native_guard_r7".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .to_string_lossy()
                .into_owned(),
        ),
    };
    let root = generated_test_directory("native-guard-r7");
    for (name, artifacts) in [
        (
            "rust",
            codegen_rust::emit_with_native_primary_eager_x12_nullable_scalars(
                &lowered,
                &options,
                &source,
                "ResolvedFilename",
                &imported.targets[0].driver_path,
            )
            .unwrap(),
        ),
        (
            "csharp",
            codegen_csharp::emit_with_native_primary_eager_x12_nullable_scalars(
                &lowered,
                &source,
                "ResolvedFilename",
                &imported.targets[0].driver_path,
            )
            .unwrap(),
        ),
    ] {
        let target = root.join(name);
        std::fs::create_dir(&target).unwrap();
        for file in artifacts.files() {
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
                include_str!("../fixtures/x12_document_body_scalars/GuardNativeHost.rs.txt"),
            )
            .unwrap();
        } else {
            let harness = target.join("Harness");
            std::fs::create_dir(&harness).unwrap();
            std::fs::write(harness.join("Host.csproj"),r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
            std::fs::write(
                harness.join("Program.cs"),
                include_str!("../fixtures/x12_document_body_scalars/GuardNativeHost.cs.txt"),
            )
            .unwrap();
            std::fs::write(
                target.join("NuGet.Config"),
                "<configuration><packageSources><clear /></packageSources></configuration>",
            )
            .unwrap();
        }
    }
    std::fs::write(root.join("input.x12"),b"ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261024*1200*U*00401*000000001*0*T*>~GS*PO*SENDER*RECEIVER*20261024*1200*1*X*004010~ST*850*0001~BEG*A~LIN*R1*1~BEG*SHADOW~LIN*R2*1~BEG*SHADOW~SE*7*0001~ST*850*0002~BEG~LIN*R3*1~BEG*SHADOW~SE*5*0002~ST*850*0003~BEG*E~LIN*R4*1~BEG*SHADOW~SE*5*0003~GE*3*1~IEA*1*000000001~").unwrap();
}
