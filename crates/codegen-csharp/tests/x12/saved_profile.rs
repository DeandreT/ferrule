//! Independently specified physical-option oracles, including all parsed null slots.
use super::*;
use mapping::{EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind, X12Autocomplete};

const INPUT: &str = include_str!("profile-input.x12");
const SOURCE: &str = include_str!("profile-source-expected.json");
const SCALED: &str = include_str!("profile-scaled-expected.json");
const OUTPUT: &str = include_str!("profile-output-expected.x12");
const COMPLETED: &str = include_str!("profile-completed-expected.x12");
const SUPPLIED_COMPLETION: &str = include_str!("profile-supplied-completion-expected.x12");

fn profile_schema() -> SchemaNode {
    let mut fields = envelope("940");
    let mut prefix = segment("N9", 2);
    children_mut(&mut prefix)[0].fixed = Some("OP".into());
    let mut reference = segment("N9", 2);
    children_mut(&mut reference)[0].fixed = Some("LI".into());
    let mut quantity = segment("W01", 3);
    for child in &mut children_mut(&mut quantity)[..2] {
        child.kind = SchemaKind::Scalar {
            ty: ScalarType::Float,
        };
    }
    fields.extend([
        segment("W05", 3),
        segment("DTM", 2),
        segment("AMT", 1),
        SchemaNode::group(
            "Detail",
            vec![
                SchemaNode::group("Prefix", vec![prefix]).repeating(),
                reference,
                segment("LX", 1),
                quantity,
            ],
        )
        .repeating(),
        SchemaNode::group("W76", vec![SchemaNode::scalar("W7601", ScalarType::Float)]),
    ]);
    fields.extend(trailers());
    SchemaNode::group("Interchange", fields)
}
fn profile_project() -> Project {
    let schema = profile_schema();
    project(
        schema.clone(),
        schema,
        Nodes(BTreeMap::new()),
        Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    )
}
fn failed_project() -> Project {
    let mut project = profile_project();
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::String("AUTHORED-FAILURE".into()),
        },
    );
    project.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: vec!["Detail".into()],
        },
        selection: mapping::FailureSelection::All,
        message: Some(1),
    });
    project
}
fn implied() -> Vec<EdiImpliedDecimal> {
    [
        vec!["Detail", "W01", "W0101"],
        vec!["Detail", "W01", "W0102"],
        vec!["W76", "W7601"],
    ]
    .into_iter()
    .map(|path| EdiImpliedDecimal::new(path.into_iter().map(str::to_owned).collect(), 2).unwrap())
    .collect()
}
fn lexical() -> Vec<EdiLexicalFormat> {
    [
        (vec!["ISA", "ISA09"], EdiLexicalKind::CompactDate6),
        (
            vec!["ISA", "ISA10"],
            EdiLexicalKind::CompactTime {
                min_digits: 4,
                max_digits: 4,
            },
        ),
        (vec!["GS", "GS04"], EdiLexicalKind::CompactDate8),
        (
            vec!["GS", "GS05"],
            EdiLexicalKind::CompactTime {
                min_digits: 4,
                max_digits: 8,
            },
        ),
        (vec!["DTM", "DTM01"], EdiLexicalKind::CompactDate8),
        (
            vec!["DTM", "DTM02"],
            EdiLexicalKind::CompactTime {
                min_digits: 4,
                max_digits: 8,
            },
        ),
        (
            vec!["AMT", "AMT01"],
            EdiLexicalKind::Decimal { max_chars: 8 },
        ),
        (
            vec!["Detail", "W01", "W0101"],
            EdiLexicalKind::Decimal { max_chars: 10 },
        ),
        (
            vec!["Detail", "W01", "W0102"],
            EdiLexicalKind::Decimal { max_chars: 10 },
        ),
        (
            vec!["W76", "W7601"],
            EdiLexicalKind::Decimal { max_chars: 10 },
        ),
    ]
    .into_iter()
    .map(|(path, kind)| {
        EdiLexicalFormat::new(path.into_iter().map(str::to_owned).collect(), kind).unwrap()
    })
    .collect()
}
fn syntax() -> X12Separators {
    X12Separators {
        element: '*',
        component: ':',
        segment: '~',
        repetition: Some('^'),
        release: None,
    }
}
fn options(lenient: bool, scaled: bool, formatted: bool, complete: bool) -> X12BoundaryOptions {
    X12BoundaryOptions {
        separators: Some(syntax()),
        lenient_segments: lenient,
        implied_decimals: if scaled { implied() } else { vec![] },
        lexical_formats: if formatted { lexical() } else { vec![] },
        autocomplete: complete.then_some(X12Autocomplete {
            request_acknowledgement: true,
            transaction_set: Some("940".into()),
        }),
        ..Default::default()
    }
}
fn constrained_completion() -> X12BoundaryOptions {
    let mut selected = options(false, false, true, true);
    selected.constraints = vec![
        mapping::EdiValueConstraint::new(vec!["ISA".into(), "ISA09".into()], 6, 6, vec![]).unwrap(),
        mapping::EdiValueConstraint::new(vec!["DTM".into(), "DTM01".into()], 8, 8, vec![]).unwrap(),
        mapping::EdiValueConstraint::new(
            vec!["W05".into(), "W0503".into()],
            8,
            8,
            vec!["TEXT^TAG".into()],
        )
        .unwrap(),
    ];
    selected
}
fn evidence(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferrule_x12_profile_{label}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    eprintln!("complete authored X12 profile evidence: {}", root.display());
    root
}
fn all_values(instance: &ir::Instance) -> serde_json::Value {
    use ir::{Instance, Value};
    match instance {
        Instance::Group(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), all_values(value)))
                .collect(),
        ),
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            serde_json::Value::Array(items.iter().map(all_values).collect())
        }
        Instance::Scalar(Value::String(value)) => value.clone().into(),
        Instance::Scalar(Value::Float(value)) => serde_json::json!(value),
        Instance::Scalar(Value::Int(value)) => serde_json::json!(value),
        Instance::Scalar(Value::Bool(value)) => (*value).into(),
        Instance::Scalar(Value::Null | Value::JsonNull(_) | Value::XmlNil(_)) => {
            serde_json::Value::Null
        }
        Instance::DocumentSet(_) => panic!("authored one-document fixture"),
    }
}
fn all_kinds(instance: &ir::Instance) -> serde_json::Value {
    use ir::{Instance, Value};
    match instance {
        Instance::Group(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), all_kinds(value)))
                .collect(),
        ),
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            serde_json::Value::Array(items.iter().map(all_kinds).collect())
        }
        Instance::Scalar(value) => match value {
            Value::String(_) => "String",
            Value::Float(_) => "Double",
            Value::Int(_) => "Int64",
            Value::Bool(_) => "Bool",
            Value::Null => "Null",
            Value::JsonNull(_) => "JsonNull",
            Value::XmlNil(_) => "XmlNil",
        }
        .into(),
        Instance::DocumentSet(_) => panic!("authored one-document fixture"),
    }
}
fn expected_kinds(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), expected_kinds(value)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(expected_kinds).collect())
        }
        serde_json::Value::String(_) => "String".into(),
        serde_json::Value::Number(_) => "Double".into(),
        serde_json::Value::Null => "Null".into(),
        _ => panic!("authored scalar oracle type"),
    }
}
fn json_projection(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            fields.retain(|_, value| !value.is_null());
            for value in fields.values_mut() {
                json_projection(value);
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                json_projection(value);
            }
        }
        _ => {}
    }
}
fn lexical_input() -> serde_json::Value {
    let mut input: serde_json::Value = serde_json::from_str(SCALED).unwrap();
    input["DTM"]["DTM01"] = "2028-02-29+05:30".into();
    input["DTM"]["DTM02"] = "12:34:56.120+05:30".into();
    input["ISA"]["ISA09"] = "2028-02-29".into();
    input["ISA"]["ISA10"] = "12:34:00+05:30".into();
    input["GS"]["GS04"] = "2028-02-29Z".into();
    input["GS"]["GS05"] = "12:34:56.12+05:30".into();
    json_projection(&mut input);
    input
}
#[test]
fn native_profile_full_values_and_literal_wire_oracles() {
    let root = evidence("native");
    let schema = profile_schema();
    fs::write(root.join("input.x12"), INPUT).unwrap();
    fs::write(root.join("source.expected.json"), SOURCE).unwrap();
    fs::write(root.join("scaled.expected.json"), SCALED).unwrap();
    for (label, input) in [
        ("plain", INPUT.to_owned()),
        (
            "unknown-top",
            INPUT
                .replace("ST*940*0101~\n", "ST*940*0101~\nZZZ*UNDECLARED~\n")
                .replace("SE*13*", "SE*14*"),
        ),
        (
            "unknown-detail",
            INPUT
                .replace("LX*1~\n", "LX*1~\nZZZ*UNDECLARED~\n")
                .replace("SE*13*", "SE*14*"),
        ),
    ] {
        fs::write(root.join(format!("{label}-input.x12")), &input).unwrap();
        let original = format_edi::x12::from_str(&input, &schema, true);
        fs::write(
            root.join(format!("{label}-parse.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        let mut parsed = original.expect("full original outcome retained");
        let mut expected: serde_json::Value = serde_json::from_str(SOURCE).unwrap();
        if label != "plain" {
            expected["SE"]["SE01"] = "14".into();
        }
        assert!(json_values_match(&all_values(&parsed), &expected));
        assert_eq!(all_kinds(&parsed), expected_kinds(&expected));
        let original = format_edi::apply_implied_decimals(&mut parsed, &implied());
        fs::write(
            root.join(format!("{label}-scale.debug.txt")),
            format!("{original:#?}\n{parsed:#?}"),
        )
        .unwrap();
        original.expect("scaling outcome retained");
        let mut expected: serde_json::Value = serde_json::from_str(SCALED).unwrap();
        if label != "plain" {
            expected["SE"]["SE01"] = "14".into();
        }
        assert!(json_values_match(&all_values(&parsed), &expected));
        assert_eq!(all_kinds(&parsed), expected_kinds(&expected));
    }
    for (label, input, expected_kind) in [
        (
            "protected-iteration",
            INPUT
                .replace("W01*1250**EA~\n", "")
                .replace("SE*13*", "SE*12*"),
            "UnexpectedSegment",
        ),
        (
            "protected-sibling",
            INPUT
                .replace("W01*1250**EA~\nN9*LI*REF-B~\nLX*2~\nW01*-500*0*EA~\n", "")
                .replace("SE*13*", "SE*9*"),
            "UnexpectedSegment",
        ),
        (
            "matching-invalid-number",
            INPUT.replace("W01*1250", "W01*NaN"),
            "ElementParse",
        ),
    ] {
        let original = format_edi::x12::from_str(&input, &schema, true);
        fs::write(root.join(format!("{label}-input.x12")), &input).unwrap();
        fs::write(
            root.join(format!("{label}-error.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        let actual = match original {
            Err(format_edi::EdiFormatError::UnexpectedSegment { .. }) => "UnexpectedSegment",
            Err(format_edi::EdiFormatError::ElementParse { .. }) => "ElementParse",
            _ => "Other",
        };
        assert_eq!(actual, expected_kind);
    }
    let input = lexical_input();
    fs::write(
        root.join("lexical-input.json"),
        serde_json::to_string_pretty(&input).unwrap(),
    )
    .unwrap();
    let original = format_json::from_str(&input.to_string(), &schema);
    fs::write(
        root.join("lexical-json-parse.debug.txt"),
        format!("{original:#?}"),
    )
    .unwrap();
    let mut instance = original.expect("JSON parser outcome retained");
    let original = format_edi::apply_output_lexical_formats(&mut instance, &lexical());
    fs::write(
        root.join("lexical-result.debug.txt"),
        format!("{original:#?}\n{instance:#?}"),
    )
    .unwrap();
    original.expect("lexical outcome retained");
    let original = format_edi::x12::to_string_with_syntax(
        &schema,
        &instance,
        format_edi::x12::Separators {
            element: '*',
            component: ':',
            segment: '~',
            repetition: Some('^'),
            release: None,
        },
        Some("00401"),
    );
    fs::write(
        root.join("output-result.debug.txt"),
        format!("{original:#?}"),
    )
    .unwrap();
    fs::write(root.join("output.expected.x12"), OUTPUT).unwrap();
    assert_eq!(original.unwrap(), OUTPUT);

    for (label, number, wire) in [
        ("bounded-float-rounding", 0.30000000000000004, "0.3"),
        ("float-negative-zero", -0.0, "-0"),
    ] {
        let mut input = lexical_input();
        input["W76"]["W7601"] = serde_json::json!(number);
        let expected = OUTPUT.replace("W76*7.5", &format!("W76*{wire}"));
        fs::write(root.join(format!("{label}-input.json")), input.to_string()).unwrap();
        fs::write(root.join(format!("{label}-expected.x12")), &expected).unwrap();
        let original = format_json::from_str(&input.to_string(), &schema);
        fs::write(
            root.join(format!("{label}-parse.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        let mut instance = original.expect("numeric lexical input retained");
        let original = format_edi::apply_output_lexical_formats(&mut instance, &lexical());
        fs::write(
            root.join(format!("{label}-lexical.debug.txt")),
            format!("{original:#?}\n{instance:#?}"),
        )
        .unwrap();
        original.expect("numeric lexical result retained");
        let original = format_edi::x12::to_string_with_syntax(
            &schema,
            &instance,
            format_edi::x12::Separators {
                element: '*',
                component: ':',
                segment: '~',
                repetition: Some('^'),
                release: None,
            },
            Some("00401"),
        );
        fs::write(
            root.join(format!("{label}-wire.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        assert_eq!(original.unwrap(), expected);
    }

    for (label, text, wire) in [
        ("iso-short-time", "12:34", "123400"),
        ("compact-four-digit-time", "1234", "1234"),
    ] {
        let mut input = lexical_input();
        input["DTM"]["DTM02"] = text.into();
        let expected = OUTPUT.replace("DTM*20280229*12345612", &format!("DTM*20280229*{wire}"));
        fs::write(root.join(format!("{label}-input.json")), input.to_string()).unwrap();
        fs::write(root.join(format!("{label}-expected.x12")), &expected).unwrap();
        let original = format_json::from_str(&input.to_string(), &schema);
        fs::write(
            root.join(format!("{label}-parse.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        let mut instance = original.expect("time lexical input retained");
        let original = format_edi::apply_output_lexical_formats(&mut instance, &lexical());
        fs::write(
            root.join(format!("{label}-lexical.debug.txt")),
            format!("{original:#?}\n{instance:#?}"),
        )
        .unwrap();
        original.expect("time lexical result retained");
        let original = format_edi::x12::to_string_with_syntax(
            &schema,
            &instance,
            format_edi::x12::Separators {
                element: '*',
                component: ':',
                segment: '~',
                repetition: Some('^'),
                release: None,
            },
            Some("00401"),
        );
        fs::write(
            root.join(format!("{label}-wire.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        assert_eq!(original.unwrap(), expected);
    }
    let mut supplied = lexical_input();
    supplied["GS"]["GS05"] = "12:34:56+05:30".into();
    let mut empty = supplied.clone();
    for field in [
        "ISA01", "ISA02", "ISA03", "ISA04", "ISA05", "ISA07", "ISA13", "ISA14", "ISA15",
    ] {
        empty["ISA"][field] = "".into();
    }
    for field in ["ISA09", "ISA10"] {
        empty["ISA"].as_object_mut().unwrap().remove(field);
    }
    for field in ["GS04", "GS05"] {
        empty["GS"].as_object_mut().unwrap().remove(field);
    }
    empty["GS"]["GS06"] = "".into();
    empty["ST"]["ST02"] = "".into();
    for segment in ["SE", "GE", "IEA"] {
        for (_, value) in empty[segment].as_object_mut().unwrap() {
            *value = "".into();
        }
    }
    for (label, input, expected) in [
        ("completion-supplied", supplied, SUPPLIED_COMPLETION),
        ("completion-null-date-fields", empty, COMPLETED),
    ] {
        fs::write(root.join(format!("{label}-input.json")), input.to_string()).unwrap();
        fs::write(root.join(format!("{label}-expected.x12")), expected).unwrap();
        let original = format_json::from_str(&input.to_string(), &schema);
        fs::write(
            root.join(format!("{label}-parse.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        let mut instance = original.expect("completion input retained");
        let original = format_edi::apply_output_lexical_formats(&mut instance, &lexical());
        fs::write(
            root.join(format!("{label}-lexical.debug.txt")),
            format!("{original:#?}\n{instance:#?}"),
        )
        .unwrap();
        original.expect("completion lexical outcome retained");
        let original = format_edi::x12::to_string_with_syntax_and_autocomplete(
            &schema,
            &instance,
            format_edi::x12::Separators {
                element: '*',
                component: ':',
                segment: '~',
                repetition: Some('^'),
                release: None,
            },
            Some("00401"),
            format_edi::x12::Autocomplete {
                current_datetime: "2028-02-29T12:34:56.12+05:30",
                request_acknowledgement: true,
                transaction_set: Some("940"),
            },
        );
        fs::write(
            root.join(format!("{label}-output.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        assert_eq!(original.unwrap(), expected);
    }
}

#[test]
fn compiled_profile_public_apis_match_complete_authored_oracles() {
    let root = evidence("compiled");
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    for (name, value) in [
        ("input.x12", INPUT),
        ("source.expected.json", SOURCE),
        ("scaled.expected.json", SCALED),
        ("output.expected.x12", OUTPUT),
        ("completed.expected.x12", COMPLETED),
        ("supplied-completion.expected.x12", SUPPLIED_COMPLETION),
        ("Host.cs", include_str!("SavedProfileHost.cs")),
    ] {
        fs::write(root.join(name), value).unwrap();
    }
    fs::write(
        root.join("lexical-input.json"),
        serde_json::to_string_pretty(&lexical_input()).unwrap(),
    )
    .unwrap();
    let program = lower(&profile_project());
    let mut references = String::new();
    for (name, policy) in [
        (
            "Strict",
            X12BoundaryPolicy {
                source: Some(options(false, false, false, false)),
                target: None,
            },
        ),
        (
            "Lenient",
            X12BoundaryPolicy {
                source: Some(options(true, false, false, false)),
                target: None,
            },
        ),
        (
            "Scaled",
            X12BoundaryPolicy {
                source: Some(options(true, true, false, false)),
                target: None,
            },
        ),
        (
            "Formatted",
            X12BoundaryPolicy {
                source: None,
                target: Some(options(false, false, true, false)),
            },
        ),
        (
            "InactiveTarget",
            X12BoundaryPolicy {
                source: None,
                target: Some(options(true, true, true, false)),
            },
        ),
        (
            "Completed",
            X12BoundaryPolicy {
                source: None,
                target: Some(options(false, false, true, true)),
            },
        ),
        (
            "FailedMap",
            X12BoundaryPolicy {
                source: None,
                target: Some(options(false, false, true, true)),
            },
        ),
        (
            "ConstrainedCompleted",
            X12BoundaryPolicy {
                source: None,
                target: Some(constrained_completion()),
            },
        ),
    ] {
        let program = if name == "FailedMap" {
            lower(&failed_project())
        } else {
            program.clone()
        };
        let original = codegen_csharp::emit_with_x12(&program, &policy);
        fs::write(
            root.join(format!("{name}-emit.debug.txt")),
            format!("{original:#?}"),
        )
        .unwrap();
        let artifacts = original.expect("original emission retained");
        let runtime_paths: std::collections::BTreeSet<_> = artifacts
            .files()
            .iter()
            .filter_map(|file| file.path.as_str().strip_prefix("Runtime/X12/"))
            .collect();
        assert_eq!(
            runtime_paths,
            std::collections::BTreeSet::from([
                "FerruleX12.cs",
                "FerruleX12.Schema.cs",
                "FerruleX12.Reader.cs",
                "FerruleX12.Numeric.cs",
                "FerruleX12.Writer.cs",
                "FerruleX12.Completion.cs",
                "FerruleX12.Lexical.cs",
                "FerruleX12Exception.cs",
            ]),
            "complete planned runtime membership"
        );
        for file in artifacts.files() {
            if let Some(relative) = file.path.as_str().strip_prefix("Runtime/X12/") {
                assert_eq!(
                    file.contents,
                    fs::read(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../runtime/csharp/Ferrule.Runtime/X12")
                            .join(relative)
                    )
                    .unwrap(),
                    "fresh emitted runtime body"
                );
            }
            let path = root.join(name).join(file.path.as_str());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &file.contents).unwrap();
        }
        let assembly = format!("Ferrule.X12.Profile.{name}");
        let original = retain_command(
            &root,
            &format!("{name}-build"),
            Command::new("dotnet")
                .args([
                    "build",
                    &format!("{name}/Ferrule.Generated.csproj"),
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
                    "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                    "-p:EnableTargetingPackDownload=false",
                    "-p:EnableRuntimePackDownload=false",
                    &format!("-p:AssemblyName={assembly}"),
                ])
                .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
                .env("DOTNET_NOLOGO", "1")
                .current_dir(&root),
        );
        assert!(
            original.status.success(),
            "{name} fresh standalone compilation; complete outcome at {}",
            root.display()
        );
        references.push_str(&format!("<Reference Include=\"{assembly}\"><HintPath>../{name}/bin/Release/net10.0/{assembly}.dll</HintPath><Aliases>{name}</Aliases></Reference>\n"));
    }
    fs::create_dir(root.join("Harness")).unwrap();
    fs::copy(root.join("Host.cs"), root.join("Harness/Program.cs")).unwrap();
    fs::write(root.join("Harness/Harness.csproj"), format!("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup>{references}</ItemGroup></Project>\n")).unwrap();
    let original = retain_command(
        &root,
        "host-build",
        Command::new("dotnet")
            .args([
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
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
            ])
            .current_dir(&root),
    );
    assert!(original.status.success(), "original host build retained");
    let original = retain_command(
        &root,
        "host-run",
        Command::new("dotnet")
            .args([
                "run",
                "--project",
                "Harness/Harness.csproj",
                "-c",
                "Release",
                "--no-build",
            ])
            .current_dir(&root),
    );
    assert!(
        original.status.success(),
        "original public-host outcomes retained at {}",
        root.display()
    );
    let summary: serde_json::Value = serde_json::from_slice(&original.stdout).unwrap();
    assert_eq!(summary["failed"], 0);
    assert!(summary["passed"].as_u64().unwrap() >= 40);
}
