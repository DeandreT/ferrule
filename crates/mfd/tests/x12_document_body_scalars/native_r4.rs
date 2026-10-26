use super::*;
use serde_json::json;

#[test]
fn nullable_consumer_cannot_replace_a_legacy_read_in_the_same_target_cache() {
    let schema = r#"{"type":"object","properties":{"result":{"type":"string"},"present":{"type":"boolean"}}}"#;
    let entries = r#"<entry name="present" type="json-property"><entry name="boolean" inpkey="72"/></entry><entry name="result" type="json-property"><entry name="string" inpkey="71"/></entry>"#;
    let f = Fixture::new(
        &unary("db", "is-not-null", 101, 102, 20),
        &(edge(21, 101) + &edge(102, 72) + &edge(21, 71)),
        Some((schema, entries)),
    );
    let imported = f.import().unwrap();
    let project = &imported.targets[0].body;
    assert!(
        project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::OwnedSourceField(_)))
    );
    assert!(
        project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::SourceField { frame: None, .. }))
    );
    let error = engine::run(project, &input(project, None))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("BEG/F1"),
        "legacy read must retain its required lookup behavior: {error}"
    );
}

pub(super) fn fixture() -> Fixture {
    let schema = r#"{"type":"object","properties":{"mapped":{"type":"string"},"present":{"type":"boolean"},"stamp":{"type":"string"},"rows":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string"},"mapped":{"type":"string"},"present":{"type":"boolean"},"counter":{"type":"string"}}}}}}"#;
    let entries = r#"<entry name="mapped" type="json-property"><entry name="string" inpkey="71"/></entry><entry name="present" type="json-property"><entry name="boolean" inpkey="72"/></entry><entry name="stamp" type="json-property"><entry name="string" inpkey="78"/></entry><entry name="rows" type="json-property"><entry name="array"><entry name="item"><entry name="object" inpkey="73"><entry name="label" type="json-property"><entry name="string" inpkey="74"/></entry><entry name="mapped" type="json-property"><entry name="string" inpkey="75"/></entry><entry name="present" type="json-property"><entry name="boolean" inpkey="76"/></entry><entry name="counter" type="json-property"><entry name="string" inpkey="77"/></entry></entry></entry></entry></entry>"#;
    let functions = map()
        + &unary("db", "is-not-null", 103, 104, 21)
        + r#"<component name="auto-number" library="core" kind="5" uid="22"><sources><datapoint/><datapoint/><datapoint/><datapoint/></sources><targets><datapoint pos="0" key="105"/></targets></component><component name="now" library="lang" kind="5" uid="23"><targets><datapoint pos="0" key="106"/></targets></component><component name="format-dateTime" library="core" kind="5" uid="24"><sources><datapoint pos="0" key="107"/><datapoint pos="1" key="108"/></sources><targets><datapoint pos="0" key="109"/></targets></component><component name="constant" library="core" kind="2" uid="25"><targets><datapoint pos="0" key="110"/></targets><data><constant value="[Y0001][M01][D01]" datatype="string"/></data></component>"#;
    let edges = edge(21, 101)
        + &edge(102, 71)
        + &edge(102, 75)
        + &edge(21, 103)
        + &edge(104, 72)
        + &edge(104, 76)
        + &edge(22, 73)
        + &edge(23, 74)
        + &edge(105, 77)
        + &edge(106, 78)
        + &edge(106, 107)
        + &edge(110, 108)
        + &edge(109, 52);
    let f = Fixture::new(&functions, &edges, Some((schema, entries)));
    f.change(
        "mapping.mfd",
        r#"<datapoint pos="1" key="52"/></sources>"#,
        r#"<datapoint pos="1" key="52"/><datapoint pos="2" key="53"/></sources>"#,
    );
    f.change(
        "mapping.mfd",
        r#"<edge vertexkey="51"/>"#,
        r#"<edge vertexkey="103"/>"#,
    );
    // Remove the duplicate 21->103 edge introduced above; the original vertex
    // remains a source declaration but no longer drives the output filename.
    f.change(
        "mapping.mfd",
        r#"<vertex vertexkey="21"><edges><edge vertexkey="103"/></edges></vertex>"#,
        "",
    );
    f.change(
        "mapping.mfd",
        r#"<edge vertexkey="52"/>"#,
        r#"<edge vertexkey="53"/>"#,
    );
    f.change(
        "mapping.mfd",
        "</vertices>",
        &format!("{}</vertices>", edge(30, 51)),
    );
    f.change(
        "mapping.mfd",
        r#"<entry name="BEG">"#,
        r#"<entry name="ST"><entry name="E02" outkey="30"/></entry><entry name="BEG">"#,
    );
    f.change(
        "850.Config",
        r#"<Segment ref="ST" minOccurs="0"/>"#,
        r#"<Segment ref="ST"/>"#,
    );
    f.change(
        "850.Config",
        r#"<Segment ref="SE" minOccurs="0"/>"#,
        r#"<Segment ref="SE"/>"#,
    );
    f.change(
        "850.Config",
        r#"<Segment ref="LIN"/>"#,
        r#"<Segment ref="LIN"/><Segment ref="BEG" minOccurs="0"/>"#,
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
        f.change(
            "Defs.Segment",
            &old,
            &format!("<Segment name=\"{name}\">{fields}</Segment>"),
        );
    }
    f.change("Envelope.Config", "ST/F143", "ST/E01");
    f.change(
        "mapping.mfd",
        "<messages>",
        r#"<settings interchangecontrolversionnumber="00401"/><messages>"#,
    );
    f
}

#[test]
fn native_csharp_nullable_emission_packages_compile_ready_artifacts() {
    let f = fixture();
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
    let lowered = codegen::lower_native_x12_nullable_document(&document, &source).unwrap();
    assert!(codegen::lower_document(&document).is_err());
    assert!(codegen::validate_program(lowered.program()).is_err());
    assert!(lowered.program().xml_output_mode().is_err());
    assert!(codegen_csharp::emit(lowered.program()).is_err());

    // Exercise actual emission and packaging, not just native preflight.
    // The ordinary XML classifier above rejects this filtered nullable-root
    // fixture, so a regression to ordinary packaging fails before this unwrap.
    let artifacts = codegen_csharp::emit_with_native_primary_eager_x12_nullable_scalars(
        &lowered,
        &source,
        "ResolvedFilename",
        &imported.targets[0].driver_path,
    )
    .unwrap();
    let files = artifacts
        .files()
        .iter()
        .map(|file| (file.path.as_str(), file.contents.as_slice()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(files.len(), artifacts.files().len());
    let text = |path: &str| std::str::from_utf8(files[path]).unwrap();
    let project = text("Ferrule.Generated.csproj");
    for include in [
        "GeneratedMapping.cs",
        "GeneratedTargetBuilder.cs",
        "Runtime/**/*.cs",
    ] {
        assert!(project.contains(&format!("<Compile Include=\"{include}\" />")));
    }
    assert!(project.contains("<TargetFramework>net10.0</TargetFramework>"));
    assert!(project.contains("<TreatWarningsAsErrors>true</TreatWarningsAsErrors>"));
    for required in [
        "GeneratedTargetBuilder.cs",
        "Runtime/FerruleSourceOrigin.cs",
        "Runtime/Json5/FerruleMapForceJson5.cs",
        "Runtime/Json5/FerruleJson.Json5Codec.cs",
        "Runtime/Json5/FerruleJson.Json5Schema.cs",
        "Runtime/X12/FerruleX12.SourceLexical.cs",
        "Runtime/X12/FerruleX12.SourceStructure.cs",
        "Runtime/X12/FerruleX12.cs",
        "Runtime/X12/FerruleX12.Schema.cs",
        "Runtime/X12/FerruleX12.Reader.cs",
        "Runtime/X12/FerruleX12.Numeric.cs",
        "Runtime/X12/FerruleX12.Writer.cs",
        "Runtime/X12/FerruleX12.Completion.cs",
        "Runtime/X12/FerruleX12.Lexical.cs",
        "Runtime/X12/FerruleX12Exception.cs",
    ] {
        assert!(
            !files
                .get(required)
                .unwrap_or_else(|| panic!("missing {required}"))
                .is_empty()
        );
    }
    assert!(!files.contains_key("Runtime/FerruleXml.DynamicInputDocumentOutputs.cs"));
    let mapping = text("GeneratedMapping.cs");
    for method in [
        "ParseNativePrimaryX12Document",
        "AcquireNativePrimaryX12",
        "SerializeMapForceJson",
        "EvaluatePrimaryFilename",
        "PrepareNativeX12NullableMessage",
    ] {
        assert!(mapping.contains(method), "missing {method}");
    }
    assert!(mapping.contains("public static partial class GeneratedMapping"));
    // The existing full-envelope test writes these artifacts plus the C# host
    // project for parent-serialized compilation and execution.
}

#[test]
fn native_selected_message_profile_emits_full_envelope_hosts() {
    let f = fixture();
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
        "ordinary lowering must retain its closed ownership policy"
    );
    let lowered = codegen::lower_native_x12_nullable_document(&document, &source).unwrap();
    assert!(lowered.native_x12_nullable_binding().is_some());
    assert!(
        codegen::validate_program(lowered.program()).is_err(),
        "the ordinary Program validator must remain closed"
    );
    let options = codegen_rust::Options {
        package_name: "mfd051_native_nullable_r4".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .to_string_lossy()
                .into_owned(),
        ),
    };
    assert!(codegen_rust::emit(lowered.program(), &options).is_err());
    assert!(codegen_csharp::emit(lowered.program()).is_err());
    let root = generated_test_directory("native-r4");
    for (name, artifact) in [
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
                include_str!("../fixtures/x12_document_body_scalars/NullableNativeHost.rs.txt"),
            )
            .unwrap();
        } else {
            let harness = target.join("Harness");
            std::fs::create_dir(&harness).unwrap();
            std::fs::write(harness.join("Host.csproj"),r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
            std::fs::write(
                harness.join("Program.cs"),
                include_str!("../fixtures/x12_document_body_scalars/NullableNativeHost.cs.txt"),
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
    let row = |label: &str, mapped: serde_json::Value, present: bool, counter: &str| json!({"label":label,"mapped":mapped,"present":present,"counter":counter});
    let stamp = "2026-10-24T12:00:00";
    let expected = json!([
        {"path":"000120261025.json","value":{"mapped":"alpha","present":true,"stamp":stamp,"rows":[row("R1",json!("alpha"),true,"1"),row("R2",json!("alpha"),true,"2")]}},
        {"path":"000220261025.json","value":{"mapped":null,"present":false,"stamp":stamp,"rows":[row("R3",json!(null),false,"3")]}},
        {"path":"000320261025.json","value":{"mapped":"","present":true,"stamp":stamp,"rows":[row("R4",json!(""),true,"4")]}}
    ]);
    std::fs::write(
        root.join("expected.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
}

#[test]
fn native_message_profile_does_not_admit_arbitrary_owned_reads() {
    let f = fixture();
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
    assert!(codegen::lower_document(&document).is_err());
    let lowered = codegen::lower_native_x12_nullable_document(&document, &source).unwrap();
    for bad in 0..3 {
        let mut program = lowered.program().clone();
        let field = program
            .expressions
            .iter_mut()
            .find_map(|node| {
                if let codegen::Expression::OwnedSourceField(field) = &mut node.expression {
                    Some(field)
                } else {
                    None
                }
            })
            .unwrap();
        match bad {
            0 => field.frame = None,
            1 => {
                field.source.owner = mapping::SourceOwner::Named {
                    name: "Other".into(),
                }
            }
            _ => field.source.path = vec!["Rows".into(), "LIN".into(), "F1".into()],
        }
        assert!(codegen::validate_native_x12_nullable_scalars(&program, &source).is_err());
    }
    assert!(codegen::validate_native_x12_nullable_scalars(lowered.program(), &source).is_ok());
    let mut owned_iteration = lowered.program().clone();
    owned_iteration.root.children[0].iteration = Some(codegen::IterationPlan::owned_source(
        mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: vec!["Rows".into()],
        },
    ));
    assert!(codegen::validate_native_x12_nullable_scalars(&owned_iteration, &source).is_err());
    let mut optional_scope = lowered.program().clone();
    optional_scope.root.children[0].iteration = Some(codegen::IterationPlan::optional_presence(0));
    let error = codegen::validate_native_x12_nullable_scalars(&optional_scope, &source)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("only ordinary repeated source scopes"),
        "{error}"
    );
    let mut aggregate_context = lowered.program().clone();
    let root_field = aggregate_context
        .expressions
        .iter()
        .find(|node| matches!(node.expression, codegen::Expression::OwnedSourceField(_)))
        .unwrap()
        .id;
    let aggregate = aggregate_context
        .expressions
        .iter_mut()
        .find_map(|node| {
            if let codegen::Expression::Aggregate { value, .. } = &mut node.expression {
                Some(value)
            } else {
                None
            }
        })
        .unwrap();
    *aggregate = codegen::AggregateValue::Expression(root_field);
    let error = codegen::validate_native_x12_nullable_scalars(&aggregate_context, &source)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("aggregate-owned expression contexts"),
        "{error}"
    );
    let mut find_context = lowered.program().clone();
    // Lowering prunes unused constants. Supply a fresh, valid predicate rather
    // than depending on an unrelated true literal surviving optimization.
    let predicate = find_context
        .expressions
        .iter()
        .map(|node| node.id)
        .max()
        .unwrap()
        .checked_add(1)
        .unwrap();
    find_context.expressions.push(codegen::ExpressionNode {
        id: predicate,
        expression: codegen::Expression::Const {
            value: Value::Bool(true),
        },
    });
    let find = find_context
        .expressions
        .iter_mut()
        .find(|node| matches!(node.expression, codegen::Expression::Aggregate { .. }))
        .unwrap();
    find.expression = codegen::Expression::CollectionFind {
        collection: vec!["Rows".into()],
        predicate,
        value: root_field,
    };
    let error = codegen::validate_native_x12_nullable_scalars(&find_context, &source)
        .unwrap_err()
        .to_string();
    assert!(error.contains("find-owned expression contexts"), "{error}");
    // A permitted outer source filter must not authorize the owned root read
    // inside the expression-local collection context.
    find_context.root.children[0].iteration = Some(codegen::IterationPlan::new(
        codegen::SourceIteration::new(vec!["Rows".into()]),
        Some(predicate),
        None,
        vec![],
        codegen::IterationOutput::Repeated,
    ));
    let error = codegen::validate_native_x12_nullable_scalars(&find_context, &source)
        .unwrap_err()
        .to_string();
    assert!(error.contains("find-owned expression contexts"), "{error}");
}

#[test]
fn native_lowering_binds_the_exact_source_descriptor() {
    let f = fixture();
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
    let lowered = codegen::lower_native_x12_nullable_document(&document, &source).unwrap();
    let different = codegen::NativeX12Source {
        schema: &imported.source_schema,
        options: &imported.source_options,
        message: &imported.message_path,
        encoding: mapping::X12SourceEncoding::Latin1,
        structure: Some(&imported.source_structure),
    };
    let options = codegen_rust::Options {
        package_name: "native_descriptor_negative".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .to_string_lossy()
                .into_owned(),
        ),
    };
    assert!(
        codegen_rust::emit_with_native_primary_eager_x12_nullable_scalars(
            &lowered,
            &options,
            &different,
            "ResolvedFilename",
            &imported.targets[0].driver_path
        )
        .is_err()
    );
    assert!(
        codegen_csharp::emit_with_native_primary_eager_x12_nullable_scalars(
            &lowered,
            &different,
            "ResolvedFilename",
            &imported.targets[0].driver_path
        )
        .is_err()
    );
}
