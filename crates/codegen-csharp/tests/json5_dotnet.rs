use codegen::{
    Binding, Expression, ExpressionNode, Program, RuntimeValue, ScalarFunction, TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn identity(profile: &str) -> Program {
    let (ty, name) = match profile {
        "F" | "N" => (ScalarType::Float, "n"),
        "S" => (ScalarType::String, "n"),
        "B" => (ScalarType::Bool, "n"),
        "U" => (ScalarType::Int, "雪"),
        _ => (ScalarType::Int, "n"),
    };
    let mut field = SchemaNode::scalar(name, ty);
    field.nullable = profile == "N";
    let mut fields = vec![field];
    if profile == "K" {
        fields.push(SchemaNode::scalar("m", ScalarType::Int));
    }
    let expressions = fields
        .iter()
        .enumerate()
        .map(|(index, field)| ExpressionNode {
            id: index as u32 + 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec![field.name.clone()],
            },
        })
        .collect();
    let bindings = fields
        .iter()
        .enumerate()
        .map(|(index, field)| Binding {
            target_field: field.name.clone(),
            expression: index as u32 + 1,
            target_domain: ty.into(),
            repeating: false,
        })
        .collect();
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", fields.clone()),
        extra_sources: vec![],
        target: SchemaNode::group("Output", fields),
        expressions,
        user_functions: vec![],
        failure_rules: vec![],
        extra_targets: vec![],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings,
            children: vec![],
        },
    }
}
fn mapping(fixture: &str) -> Program {
    let mut candidate = identity("I");
    candidate.source = SchemaNode::group("Input", vec![]);
    candidate.expressions.clear();
    candidate.root.bindings[0].expression = 3;
    if fixture == "Context" {
        candidate.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Path", ScalarType::String)],
        );
        candidate.root.bindings[0].target_field = "Path".into();
        candidate.root.bindings[0].target_domain = ScalarType::String.into();
        candidate.expressions.push(ExpressionNode {
            id: 3,
            expression: Expression::RuntimeValue {
                value: RuntimeValue::MappingFilePath,
            },
        });
        return candidate;
    }
    let message = if fixture == "MessageNone" {
        None
    } else {
        Some(5)
    };
    candidate.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::Raise { message },
    });
    if fixture == "MessageEmpty" {
        candidate.expressions.push(ExpressionNode {
            id: 5,
            expression: Expression::Const {
                value: Value::String(String::new()),
            },
        });
        return candidate;
    }
    if fixture == "MessageNone" {
        return candidate;
    }
    candidate.source =
        SchemaNode::group("Input", vec![SchemaNode::scalar("Fail", ScalarType::Bool)]);
    candidate.root.bindings[0].expression = 6;
    candidate.expressions.extend([
        ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Fail".into()],
            },
        },
        ExpressionNode {
            id: 6,
            expression: Expression::If {
                condition: 1,
                then: 3,
                else_: 7,
            },
        },
        ExpressionNode {
            id: 7,
            expression: Expression::Const {
                value: Value::Int(7),
            },
        },
    ]);
    if fixture == "MessageConstant" {
        candidate.expressions.push(ExpressionNode {
            id: 5,
            expression: Expression::Const {
                value: Value::String("stop".into()),
            },
        });
    } else {
        candidate.expressions.extend([
            ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Const {
                    value: Value::Int(0),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Call {
                    function: ScalarFunction::Divide,
                    args: vec![2, 4],
                },
            },
        ]);
    }
    candidate.expressions.sort_by_key(|node| node.id);
    candidate
}

fn retain_command(root: &Path, label: &str, command: &mut Command) -> std::process::Output {
    fs::write(
        root.join(format!("{label}-COMMAND.txt")),
        format!("{command:?}\n"),
    )
    .unwrap();
    let original = command.output();
    fs::write(
        root.join(format!("{label}-RESULT.debug.txt")),
        format!("{original:#?}\n"),
    )
    .unwrap();
    let output = original.expect("command launch failed; full original retained");
    fs::write(root.join(format!("{label}-stdout.bin")), &output.stdout).unwrap();
    fs::write(root.join(format!("{label}-stderr.bin")), &output.stderr).unwrap();
    output
}

#[test]
#[ignore = "explicit package-free C# JSON5 cohort; root runs serial after source admission"]
fn generated_json5_all_four_routes_match17_literals_and6_mapping_context_cases() {
    let root = std::env::temp_dir().join(format!(
        "ferrule_json5_cohort_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("CASES.json"), include_bytes!("json5/CASES.json")).unwrap();
    let fixtures = [
        "I",
        "F",
        "S",
        "N",
        "U",
        "MessageError",
        "MessageConstant",
        "MessageNone",
        "MessageEmpty",
        "Context",
    ];
    let mut projects = String::new();
    for fixture in fixtures {
        let candidate = if fixture.starts_with("Message") || fixture == "Context" {
            mapping(fixture)
        } else {
            identity(fixture)
        };
        fs::write(
            root.join(format!("{fixture}-PROGRAM.debug.txt")),
            format!("{candidate:#?}\n"),
        )
        .unwrap();
        let result = codegen_csharp::emit_with_json5(&candidate);
        fs::write(
            root.join(format!("{fixture}-EMIT_RESULT.debug.txt")),
            format!("{result:#?}\n"),
        )
        .unwrap();
        let artifacts = result.expect("optional emission failed; complete input/result retained");
        for file in artifacts.files() {
            let path = root.join(fixture).join(file.path.as_str());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &file.contents).unwrap();
        }
        projects.push_str(&format!(
            "    <Projects Include=\"{fixture}/Ferrule.Generated.csproj\" />\n"
        ));
    }
    fs::create_dir(root.join("Host")).unwrap();
    fs::write(root.join("Host/Host.csproj"), HOST_PROJECT).unwrap();
    fs::write(root.join("Host/Program.cs"), include_str!("json5/Host.cs")).unwrap();
    projects.push_str("    <Projects Include=\"Host/Host.csproj\" />\n");
    fs::write(root.join("Cohort.proj"), format!("<Project>\n  <ItemGroup>\n{projects}  </ItemGroup>\n  <Target Name=\"Build\">\n    <MSBuild Projects=\"@(Projects)\" Targets=\"Restore;Build\" BuildInParallel=\"false\" />\n  </Target>\n</Project>\n")).unwrap();
    // These complete authored/generated inputs remain unchanged even on an ordinary
    // build/host assertion panic. The root separately owns SDK/source/ELF guards.
    let mut selected = vec![
        root.join("CASES.json"),
        root.join("Cohort.proj"),
        root.join("Host/Host.csproj"),
        root.join("Host/Program.cs"),
    ];
    for fixture in fixtures {
        collect_sources(&root.join(fixture), &mut selected);
    }
    selected.sort();
    let before: Vec<_> = selected
        .iter()
        .map(|path| (path.clone(), fs::read(path).unwrap()))
        .collect();
    fs::write(
        root.join("COMPLETE_INPUT_SOURCE_PATHS.debug.txt"),
        format!("{selected:#?}\n"),
    )
    .unwrap();
    let original = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut build = Command::new("dotnet");
        build
            .args([
                "msbuild",
                "Cohort.proj",
                "-t:Build",
                "-nologo",
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:NuGetAudit=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                "-p:AutomaticallyUseReferenceAssemblyPackages=false",
            ])
            .current_dir(&root)
            .env("DOTNET_CLI_HOME", root.join(".dotnet-home"))
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1");
        let built = retain_command(&root, "BUILD", &mut build);
        assert!(
            built.status.success(),
            "complete C# build originals at {}",
            root.display()
        );
        let mut run = Command::new("dotnet");
        run.arg(root.join("Host/bin/Debug/net10.0/Host.dll"))
            .arg(&root)
            .current_dir(&root);
        let observed = retain_command(&root, "HOST", &mut run);
        assert!(
            observed.status.success(),
            "complete C# host originals at {}",
            root.display()
        );
    }));
    let after: Vec<_> = before
        .iter()
        .map(|(path, expected)| {
            (
                path.clone(),
                fs::read(path).map(|actual| actual == *expected),
            )
        })
        .collect();
    fs::write(
        root.join("COMPLETE_INPUT_SOURCE_AFTER_RESULTS.debug.txt"),
        format!("{after:#?}\n"),
    )
    .unwrap();
    match original {
        Ok(()) => assert!(
            after.iter().all(|(_, result)| matches!(result, Ok(true))),
            "complete source guards retained"
        ),
        Err(original) => {
            let message = original
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| original.downcast_ref::<&str>().copied())
                .unwrap_or("non-string original panic payload");
            fs::write(root.join("ORIGINAL_PANIC.txt"), format!("{message}\n")).unwrap();
            std::panic::resume_unwind(original);
        }
    }
    // Always retain this finite cohort and original logs; root owns later cleanup.
}

fn collect_sources(root: &Path, selected: &mut Vec<std::path::PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_sources(&path, selected);
        } else {
            selected.push(path);
        }
    }
}

const HOST_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
</Project>
"#;
