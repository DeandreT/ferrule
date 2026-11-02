//! Self-authored wire/whole-output oracles. Both emitted public boundaries are
//! compiled; neither backend supplies the other backend's expected result.
use super::primary_documents_harness as harness;
use super::*;
use mapping::{EdiBoundaryKind, EdiValueConstraint, FormatOptions};
use serde_json::{Value as Json, json};

struct Subject {
    name: String,
    document: mapping::DocumentProject,
    cases: Vec<Json>,
}

fn layout(widths: [u32; 4]) -> TestResult<mapping::IdocLayout> {
    let mut first = 5;
    let fields = ["Code", "Number", "Amount", "Text"]
        .into_iter()
        .zip(widths)
        .map(|(name, width)| {
            let start = first;
            first += width;
            mapping::IdocFieldLayout::new(
                name,
                start.try_into().unwrap(),
                (first - 1).try_into().unwrap(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(mapping::IdocLayout::new(vec![
        mapping::IdocSegmentLayout::new("DTL", fields)?,
    ])?)
}

fn subjects() -> TestResult<Vec<Subject>> {
    let mut result = Vec::new();
    for (kind, label, header, row, text, output) in [
        (
            EdiBoundaryKind::X12,
            "x12",
            "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261025*1200*^*00501*000000001*0*P*:~",
            "DTL*0001*01.00*1234*A🙂~",
            "A🙂",
            "DTL*1*1*12.34*A🙂~\n",
        ),
        (
            EdiBoundaryKind::Edifact,
            "edifact",
            "UNH+1'",
            "DTL+0001+01.00+1234+A?+🙂'",
            "A+🙂",
            "DTL+1+1+12.34+A?+🙂'\n",
        ),
        (
            EdiBoundaryKind::Hl7,
            "hl7",
            "MSH|^~\\&|APP\r",
            "DTL|0001|01.00|1234|A\\T\\🙂\r",
            "A&🙂",
            "DTL|1|1|12.34|A\\T\\🙂\r",
        ),
        (
            EdiBoundaryKind::Tradacoms,
            "tradacoms",
            "STX=ANA'",
            "DTL=0001+01.00+1234+A?+🙂'",
            "A+🙂",
            "DTL=1+1+12.34+A?+🙂'\n",
        ),
        (
            EdiBoundaryKind::Idoc,
            "idoc",
            "",
            "DTL 000101.001234A🙂   \r\n",
            "A🙂",
            "DTL    1    1   12.34A🙂   \r\n",
        ),
    ] {
        let schema = SchemaNode::group(
            "Document",
            vec![
                SchemaNode::group(
                    "DTL",
                    vec![
                        int("Code"),
                        SchemaNode::scalar("Number", ScalarType::Float),
                        SchemaNode::scalar("Amount", ScalarType::Float),
                        string("Text"),
                    ],
                )
                .repeating(),
            ],
        );
        let mut options = FormatOptions {
            edi_kind: Some(kind),
            lenient_segments: true,
            ..Default::default()
        };
        for (name, width, code) in [
            ("Code", 4, "0001"),
            ("Number", 5, "01.00"),
            ("Amount", 4, "1234"),
            ("Text", text.chars().count() as u32, text),
        ] {
            options.edi_value_constraints.push(
                EdiValueConstraint::new(
                    vec!["DTL".into(), name.into()],
                    width,
                    width,
                    if kind == EdiBoundaryKind::Edifact && name == "Text" {
                        vec![code.into(), "A:B".into(), "C/D".into()]
                    } else {
                        vec![code.into()]
                    },
                )
                .unwrap(),
            );
        }
        options
            .edi_implied_decimals
            .push(mapping::EdiImpliedDecimal::new(vec!["DTL".into(), "Amount".into()], 2).unwrap());
        options.edi_lexical_formats.push(
            mapping::EdiLexicalFormat::new(
                vec!["DTL".into(), "Text".into()],
                mapping::EdiLexicalKind::CompactDate6,
            )
            .unwrap(),
        );
        let mut target_options = FormatOptions {
            edi_kind: Some(kind),
            ..Default::default()
        };
        if kind == EdiBoundaryKind::Idoc {
            options.idoc = Some(layout([4, 5, 4, 8])?);
            target_options.idoc = Some(layout([4, 5, 8, 8])?);
        }
        let mut project = Project {
            source: schema.clone(),
            target: schema,
            source_path: None,
            target_path: None,
            source_options: options,
            target_options,
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Default::default(),
            root: Scope {
                construction: mapping::ScopeConstruction::CopyCurrentSource,
                ..Default::default()
            },
        };
        project.graph.nodes.insert(
            90,
            Node::Const {
                value: Value::String("input.edi".into()),
            },
        );
        let document = mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(90));
        let wire = format!("{header}{row}");
        let invalid = format!("{header}{row}{}", row.replace("0001", "0002"));
        // Issue columns: location, kind, schema field, minimum, maximum,
        // actual character count, allowed-code count. Null means inapplicable.
        let mut cases = vec![
            json!({"id":"valid", "wire":wire, "output":output}),
            json!({"id":"invalid-second-row", "wire":invalid, "issues":[["DTL[2]/Code", "NotAllowed", "DTL/Code", null, null, null, 1]]}),
            json!({"id":"wire-order-report", "wire":format!("{header}{}{}", row.replace("0001", "0002").replace("01.00", "02.00"), row.replace("0001", "0002")), "issues":[["DTL[1]/Code", "NotAllowed", "DTL/Code", null, null, null, 1],["DTL[1]/Number", "NotAllowed", "DTL/Number", null, null, null, 1],["DTL[2]/Code", "NotAllowed", "DTL/Code", null, null, null, 1]]}),
            json!({"id":"valid-after-failure", "wire":wire, "output":output}),
            json!({"id":"float-spelling", "wire":format!("{header}{}", row.replace("01.00", "02.00")), "issues":[["DTL[1]/Number", "NotAllowed", "DTL/Number", null, null, null, 1]]}),
            json!({"id":"implied-spelling", "wire":format!("{header}{}", row.replace("1234", "1235")), "issues":[["DTL[1]/Amount", "NotAllowed", "DTL/Amount", null, null, null, 1]]}),
        ];
        if kind != EdiBoundaryKind::Idoc {
            cases.extend([
                json!({"id":"length-and-code", "wire":format!("{header}{}", row.replace("0001", "1")), "issues":[["DTL[1]/Code", "TooShort", "DTL/Code", 4, null, 1, null],["DTL[1]/Code", "NotAllowed", "DTL/Code", null, null, null, 1]]}),
                json!({"id":"constraint-before-overflow", "wire":format!("{header}{}", row.replace("0001", "99999999999999999999999999")), "issues":[["DTL[1]/Code", "TooLong", "DTL/Code", null, 4, 26, null],["DTL[1]/Code", "NotAllowed", "DTL/Code", null, null, null, 1]]}),
                json!({"id":"absent-optional-rows", "wire":header, "output":""}),
            ]);
        }
        if kind == EdiBoundaryKind::Edifact {
            cases.push(json!({"id":"composite-string-active-interchange-syntax",
                "wire":"UNH+1'DTL+0001+01.00+1234+A:B'UNT+3+1'UNA/+.? !UNH+2!DTL+0001+01.00+1234+C/D!UNT+3+2!",
                "output":"DTL+1+1+12.34+A?:B'\nDTL+1+1+12.34+C/D'\n"}));
        }
        result.push(Subject {
            name: label.into(),
            document,
            cases,
        });
    }
    // A constant output cannot hide invalid source data, even when no source
    // field is reachable from the mapping graph.
    let base = &result[1];
    let mut project = base.document.project().clone();
    project.target = SchemaNode::group(
        "Document",
        vec![SchemaNode::group("ACK", vec![string("Status")])],
    );
    project.graph.nodes.insert(
        91,
        Node::Const {
            value: Value::String("OK".into()),
        },
    );
    project.root = Scope {
        children: vec![Scope {
            target_field: "ACK".into(),
            bindings: vec![Binding {
                target_field: "Status".into(),
                node: 91,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut cases = base.cases.clone();
    for case in &mut cases {
        if case.get("output").is_some() {
            case["output"] = json!("ACK+OK'\n");
        }
    }
    result.push(Subject {
        name: "constant".into(),
        document: mapping::DocumentProject::new(project, base.document.primary_source().clone()),
        cases,
    });
    let base = result.last().unwrap();
    let mut project = base.document.project().clone();
    let qualified = |name: &str, qualifier: &str, field: SchemaNode| {
        let mut kind = string("Kind");
        kind.fixed = Some(qualifier.into());
        SchemaNode::group(name, vec![kind, field])
    };
    project.source = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    qualified("DTL_1", "A", string("Ignored")),
                    qualified("DTL_2", "B", string("Code").repeating()),
                ],
            )
            .repeating(),
        ],
    );
    project.source_options.edi_implied_decimals.clear();
    project.source_options.edi_lexical_formats.clear();
    project.source_options.edi_value_constraints = vec![
        EdiValueConstraint::new(
            vec!["Rows".into(), "DTL_2".into(), "Code".into()],
            2,
            2,
            vec!["OK".into()],
        )
        .unwrap(),
    ];
    result.push(Subject {
        name: "qualified-repetitions".into(),
        document: mapping::DocumentProject::new(project, base.document.primary_source().clone()),
        cases: vec![
            json!({"id":"qualified-valid", "wire":"UNB+UNOC:4'DTL+Z+SKIPPED'DTL+A+IGNORED'DTL+B+OK*OK'", "output":"ACK+OK'\n"}),
            json!({"id":"qualified-invalid", "wire":"UNB+UNOC:4'DTL+A+IGNORED'DTL+B+OK*OK'DTL+A+IGNORED'DTL+B+OK*BAD'", "issues":[["Rows[2]/DTL_2/Code[2]", "TooLong", "Rows/DTL_2/Code", null, 2, 3, null],["Rows[2]/DTL_2/Code[2]", "NotAllowed", "Rows/DTL_2/Code", null, null, null, 1]]}),
            json!({"id":"qualified-valid-after-failure", "wire":"UNB+UNOC:4'DTL+A+IGNORED'DTL+B+OK'", "output":"ACK+OK'\n"}),
        ],
    });
    let base = &result[5];
    let mut project = base.document.project().clone();
    project.source = SchemaNode::group(
        "Document",
        vec![SchemaNode::group(
            "DTL",
            vec![int("Unconstrained"), int("Code")],
        )],
    );
    project.source_options.edi_implied_decimals.clear();
    project.source_options.edi_lexical_formats.clear();
    project.source_options.edi_value_constraints = vec![
        EdiValueConstraint::new(vec!["DTL".into(), "Code".into()], 4, 4, vec!["0001".into()])
            .unwrap(),
    ];
    result.push(Subject {
        name: "conversion-priority".into(),
        document: mapping::DocumentProject::new(project, base.document.primary_source().clone()),
        cases: vec![
            json!({"id":"priority-valid", "wire":"UNH+1'DTL+1+0001'", "output":"ACK+OK'\n"}),
            json!({"id":"earlier-value-error", "wire":"UNH+1'DTL+bad+0002'", "value_error":true}),
            json!({"id":"priority-valid-after-value-error", "wire":"UNH+1'DTL+1+0001'", "output":"ACK+OK'\n"}),
            json!({"id":"same-field-constraint-before-overflow", "wire":"UNH+1'DTL+1+99999999999999999999999999'", "issues":[["DTL/Code", "TooLong", "DTL/Code", null, 4, 26, null],["DTL/Code", "NotAllowed", "DTL/Code", null, null, null, 1]]}),
            json!({"id":"priority-valid-after-constraint", "wire":"UNH+1'DTL+1+0001'", "output":"ACK+OK'\n"}),
        ],
    });
    let base = &result[0];
    let mut project = base.document.project().clone();
    project.source_options.edi_value_constraints[3] = EdiValueConstraint::new(
        vec!["DTL".into(), "Text".into()],
        3,
        3,
        vec!["A:B".into(), "C/D".into()],
    )
    .unwrap();
    project.target = SchemaNode::group(
        "Document",
        vec![SchemaNode::group("ACK", vec![string("Status")])],
    );
    project.graph.nodes.insert(
        91,
        Node::Const {
            value: Value::String("OK".into()),
        },
    );
    project.root = result[5].document.project().root.clone();
    let isa = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261025*1200*^*00501*000000001*0*P*:~";
    let second = isa.replace("000000001", "000000002").replace("*:~", "*/~");
    let wire = format!(
        "{isa}DTL*0001*01.00*1234*A:B~IEA*0*000000001~{second}DTL*0001*01.00*1234*C/D~IEA*0*000000002~"
    );
    result.push(Subject {
        name: "x12-active-component".into(),
        document: mapping::DocumentProject::new(project, base.document.primary_source().clone()),
        cases: vec![
            json!({"id":"x12-changing-component", "wire":wire, "output":"ACK*OK~\n"}),
            json!({"id":"x12-second-interchange-code", "wire":wire.replace("C/D", "C/E"), "issues":[["DTL[2]/Text", "NotAllowed", "DTL/Text", null, null, null, 2]]}),
            json!({"id":"x12-changing-component-after-error", "wire":wire, "output":"ACK*OK~\n"}),
        ],
    });
    Ok(result)
}

fn primary_artifacts(subject: &Subject, language: &str) -> TestResult<codegen::ArtifactSet> {
    let program = codegen::lower_document(&subject.document)?;
    Ok(if language == "rust" {
        codegen_rust::emit_with_primary_edi_documents(
            &program,
            &codegen_rust::Options {
                package_name: format!("edi-constraints-{}", subject.name),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    harness::runtime_path()?.display().to_string(),
                ),
            },
        )?
    } else {
        codegen_csharp::emit_with_primary_edi_documents(&program)?
    })
}

#[test]
fn edi_source_constraints_emit_shared_validation() -> TestResult<()> {
    for subject in subjects()? {
        for language in ["rust", "csharp"] {
            let artifacts = primary_artifacts(&subject, language)?;
            assert_eq!(artifacts, primary_artifacts(&subject, language)?);
            if language == "csharp" {
                assert!(artifacts.files().iter().any(|file| file.path.as_str() == "Runtime/FerruleEdi.SourceConstraints.cs"));
            }
        }
    }
    Ok(())
}

fn check_csharp(subject: &Subject, root: &Path) -> TestResult<()> {
    let directory = root.join(&subject.name);
    harness::write_artifacts(&directory, &primary_artifacts(subject, "csharp")?)?;
    std::fs::create_dir_all(directory.join("Harness"))?;
    std::fs::write(
        directory.join("Harness/Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        directory.join("Harness/Program.cs"),
        include_str!("fixtures/edi_source_constraints_csharp.cs.txt"),
    )?;
    std::fs::write(
        directory.join("cases.json"),
        serde_json::to_vec(&subject.cases)?,
    )?;
    let mut command = dotnet_command(&directory);
    command.current_dir(&directory).args([
        "run",
        "--project",
        "Harness/Harness.csproj",
        "--configuration",
        "Release",
        "-m:1",
        "-p:UseSharedCompilation=false",
        "-p:BuildInParallel=false",
    ]);
    let run = host_policy::recorded_output(&mut command, &directory, "edi-constraints-csharp")?;
    assert!(
        run.status.success(),
        "{}: {}\n{}",
        subject.name,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}

#[test]
#[ignore = "builds emitted public C# EDI adapters with .NET 10"]
fn edi_source_constraints_compiled_csharp() -> TestResult<()> {
    let directory = TempDir::new("edi_constraints_csharp")?;
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
        directory.0.join("global.json"),
    )?;
    for subject in subjects()? {
        check_csharp(&subject, &directory.0)?;
    }
    Ok(())
}

#[test]
#[ignore = "builds the existing emitted static EDIFACT C# text/byte adapters"]
fn edi_source_constraints_static_compiled_csharp() -> TestResult<()> {
    let mut project = subjects()?.remove(1).document.project().clone();
    project.source = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group("UNH", vec![string("Reference")]),
            SchemaNode::group("DTL", vec![int("Code")]),
            SchemaNode::group("UNT", vec![int("Count"), string("Reference")]),
        ],
    );
    project.source_options.edi_implied_decimals.clear();
    project.source_options.edi_lexical_formats.clear();
    project.source_options.edi_value_constraints.truncate(1);
    project.source_options.lenient_segments = false;
    project.extra_sources.push(mapping::NamedSource {
        name: "Reference".into(),
        path: "never-opened.edifact".into(),
        schema: project.source.clone(),
        options: project.source_options.clone(),
        dynamic_path: None,
    });
    project.target = SchemaNode::group("Result", vec![string("Status")]);
    project.target_options = FormatOptions::default();
    project.graph.nodes.insert(
        91,
        Node::Const {
            value: Value::String("OK".into()),
        },
    );
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Status".into(),
            node: 91,
        }],
        ..Default::default()
    };
    let source = codegen::DocumentBoundaryOptions::Edifact(
        codegen::EdifactBoundaryOptions::from_format_options(&project.source_options)?,
    );
    let policy = codegen::StaticDocumentBoundaryPolicy {
        source: source.clone(),
        target: codegen::DocumentBoundaryOptions::Json,
        extra_sources: vec![codegen::NamedDocumentBoundaryOptions {
            name: "Reference".into(),
            options: source,
        }],
        extra_targets: vec![],
    };
    let artifacts =
        codegen_csharp::emit_with_static_document_adapters(&codegen::lower(&project)?, &policy)?;
    let directory = TempDir::new("edi_constraints_static_csharp")?;
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
        directory.0.join("global.json"),
    )?;
    harness::write_artifacts(&directory.0, &artifacts)?;
    std::fs::create_dir_all(directory.0.join("Harness"))?;
    std::fs::write(
        directory.0.join("Harness/Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        directory.0.join("Harness/Program.cs"),
        include_str!("fixtures/edi_source_constraints_static_csharp.cs.txt"),
    )?;
    let mut command = dotnet_command(&directory.0);
    command.current_dir(&directory.0).args([
        "run",
        "--project",
        "Harness/Harness.csproj",
        "--configuration",
        "Release",
        "-m:1",
        "-p:UseSharedCompilation=false",
        "-p:BuildInParallel=false",
    ]);
    let run =
        host_policy::recorded_output(&mut command, &directory.0, "edi-constraints-static-csharp")?;
    assert!(
        run.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}

#[test]
#[ignore = "builds emitted public Rust EDI adapters with Cargo"]
fn edi_source_constraints_compiled_rust() -> TestResult<()> {
    let directory = TempDir::new("edi_constraints_rust")?;
    let subjects = subjects()?;
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    let mut cases = Vec::new();
    for (index, subject) in subjects.iter().enumerate() {
        let model = directory.0.join(format!("model{index}"));
        harness::write_artifacts(&model, &primary_artifacts(subject, "rust")?)?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"edi-constraints-{}\",path=\"../model{index}\"}}\n",
            subject.name
        ));
        dispatch.push_str(&format!(
            "{index} => model{index}::execute_primary_edi_document_bytes(&execution, &host, 1),\n"
        ));
        for mut case in subject.cases.clone() {
            case["model"] = json!(index);
            cases.push(case);
        }
    }
    let host = directory.0.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    std::fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname=\"edi-constraints-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nformat-edi={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&harness::runtime_path()?.display().to_string())?,
            serde_json::to_string(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../format-edi")
                    .canonicalize()?
                    .display()
                    .to_string()
            )?
        ),
    )?;
    std::fs::write(
        host.join("src/main.rs"),
        include_str!("fixtures/edi_source_constraints_rust.rs.txt")
            .replace("__DISPATCH__", &dispatch),
    )?;
    std::fs::write(host.join("cases.json"), serde_json::to_vec(&cases)?)?;
    let mut command = Command::new("cargo");
    command.current_dir(&host).args([
        "run",
        "--offline",
        "--quiet",
        "-j1",
        "--config",
        "profile.dev.debug=0",
        "--config",
        "profile.dev.incremental=false",
    ]);
    let run = host_policy::recorded_output(&mut command, &host, "edi-constraints-rust")?;
    assert!(
        run.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
