use super::*;

fn casing_project() -> Project {
    let mut nodes = BTreeMap::new();
    nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Text".into()],
            frame: None,
        },
    );
    for (id, function) in [(2, "upper"), (3, "lower")] {
        nodes.insert(
            id,
            Node::Call {
                function: function.into(),
                args: vec![1],
            },
        );
    }
    Project {
        execution_contract: None,
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Cases", vec![string("Text")]).repeating()],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::group("Cases", vec![string("Upper"), string("Lower")]).repeating()],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Cases".into(),
                iteration: ScopeIteration::Source(vec!["Cases".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Upper".into(),
                        node: 2,
                    },
                    Binding {
                        target_field: "Lower".into(),
                        node: 3,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn generated_unicode_casing_matches_interpreter_in_rust_and_csharp() -> TestResult<()> {
    let project = casing_project();
    assert!(engine::validate(&project).is_empty());
    // Independently authored (input, upper, lower) vectors: length-changing
    // expansions, Final_Sigma context, titlecase digraphs, language-neutral
    // dotted/dotless I and supplementary scalars, including Unicode 17 letters.
    let cases = [
        ("straße", "STRASSE", "straße"),
        ("ẞ", "ẞ", "ß"),
        ("ﬁ ŉ ǰ", "FI \u{2BC}N J\u{30C}", "ﬁ ŉ ǰ"),
        ("ΐ ᾳ", "\u{399}\u{308}\u{301} ΑΙ", "ΐ ᾳ"),
        ("ǅ", "Ǆ", "ǆ"),
        ("İ ı I i", "İ I I I", "i\u{307} ı i i"),
        ("ΟΔΟΣ ΣΑ", "ΟΔΟΣ ΣΑ", "οδος σα"),
        ("AΣ'B A'Σ", "AΣ'B A'Σ", "aσ'b a'ς"),
        ("ʰΣ AʰΣ", "ʰΣ AʰΣ", "ʰσ aʰς"),
        ("𐐀Σ ΑΣ𐐨", "𐐀Σ ΑΣ𐐀", "𐐨ς ασ𐐨"),
        (
            "\u{16EBB}\u{A7CE}",
            "\u{16EA0}\u{A7CE}",
            "\u{16EBB}\u{A7CF}",
        ),
        ("\u{212A}µ🙂", "\u{212A}Μ🙂", "kµ🙂"),
    ];
    let input = serde_json::json!({
        "Cases": cases.iter().map(|&(text, _, _)| serde_json::json!({"Text": text})).collect::<Vec<_>>()
    })
    .to_string();
    let source = format_json::from_str(&input, &project.source)?;
    let output = engine::run(&project, &source)?;
    let expected: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &output)?)?;
    let rows = expected["Cases"].as_array().ok_or("expected output rows")?;
    assert_eq!(rows.len(), cases.len());
    for (row, &(text, upper, lower)) in rows.iter().zip(&cases) {
        assert_eq!(row["Upper"], upper, "interpreter upper({text:?})");
        assert_eq!(row["Lower"], lower, "interpreter lower({text:?})");
    }
    super::json_text_boundaries::run_generated_boundary_cases(
        &project,
        &[serde_json::json!({"input": input, "expected": expected})],
        "unicode_casing",
    )
}

// Keep complete generated sources, literal expectations and process output on
// failure. Successful runs use the suite's ordinary opt-in retention policy.
struct CallerCultureArtifacts(Option<TempDir>);

impl Drop for CallerCultureArtifacts {
    fn drop(&mut self) {
        if let Some(directory) = self.0.take() {
            eprintln!(
                "Retained caller-culture test artifacts: {}",
                directory.0.display()
            );
            std::mem::forget(directory);
        }
    }
}

fn recorded_caller_culture_command(
    command: &mut Command,
    directory: &Path,
    name: &str,
) -> io::Result<Output> {
    std::fs::write(
        directory.join(format!("{name}-command.txt")),
        format!("{command:?}\n"),
    )?;
    let output = match command.isolated_output() {
        Ok(output) => output,
        Err(error) => {
            std::fs::write(
                directory.join(format!("{name}-spawn-error.txt")),
                format!("{error:?}\n"),
            )?;
            return Err(error);
        }
    };
    std::fs::write(directory.join(format!("{name}-stdout.txt")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.txt")), &output.stderr)?;
    std::fs::write(
        directory.join(format!("{name}-status.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "success": output.status.success(),
            "code": output.status.code(),
            "status": format!("{:?}", output.status),
        }))?,
    )?;
    assert!(
        output.status.success(),
        "caller-culture {name} failed ({}), artifacts at {}:\nstdout:\n{}\nstderr:\n{}",
        output.status,
        directory.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(output)
}

#[test]
fn generated_csharp_unicode_casing_is_independent_of_named_caller_cultures() -> TestResult<()> {
    let directory = TempDir::new("unicode_casing_caller_cultures")?;
    let path = directory.0.clone();
    let mut artifacts = CallerCultureArtifacts(Some(directory));
    let project = casing_project();
    assert!(engine::validate(&project).is_empty());
    let project_path = path.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    generate_project(
        &project_path,
        &path.join("generated"),
        GenerateTarget::CSharp,
    )?;
    // The sandbox is outside the checkout: retain the repository SDK pin here.
    std::fs::write(
        path.join("global.json"),
        include_bytes!("../../../../global.json"),
    )?;
    std::fs::write(
        path.join("cases.json"),
        include_bytes!("unicode_casing/caller_culture_cases.json"),
    )?;
    let caller = path.join("Caller");
    std::fs::create_dir(&caller)?;
    std::fs::write(
        caller.join("CallerCulture.csproj"),
        include_str!("unicode_casing/CallerCulture.csproj"),
    )?;
    std::fs::write(
        caller.join("Program.cs"),
        include_str!("unicode_casing/CallerCulture.cs"),
    )?;

    let build_output = path.join("artifacts");
    let mut build = dotnet_command(&path);
    build
        .args([
            "build",
            "Caller/CallerCulture.csproj",
            "--configuration",
            "Release",
            "--artifacts-path",
        ])
        .arg(&build_output)
        .args([
            "--verbosity",
            "quiet",
            "-maxCpuCount:1",
            "-nodeReuse:false",
            "-p:UseSharedCompilation=false",
        ])
        .current_dir(&path);
    recorded_caller_culture_command(&mut build, &path, "build")?;

    let binary = build_output.join("bin/CallerCulture/release/CallerCulture.dll");
    let runtime_config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(binary.with_extension("runtimeconfig.json"))?)?;
    let properties = &runtime_config["runtimeOptions"]["configProperties"];
    assert_eq!(properties["System.Globalization.Invariant"], false);
    assert_eq!(
        properties["System.Globalization.PredefinedCulturesOnly"],
        true
    );

    let cases: serde_json::Value =
        serde_json::from_str(include_str!("unicode_casing/caller_culture_cases.json"))?;
    let rows = cases.as_array().ok_or("expected literal casing cases")?;
    let expected = serde_json::json!({
        "Cases": rows.iter().map(|row| serde_json::json!({
            "Upper": row["upper"], "Lower": row["lower"],
        })).collect::<Vec<_>>()
    });
    for culture in ["en-US", "tr-TR", "az-Latn-AZ", "lt-LT", "invariant"] {
        let mut command = dotnet_command(&path);
        command
            .arg(&binary)
            .arg(culture)
            .arg(path.join("cases.json"))
            // Override only this child. Predefined-only construction fails
            // closed if invariant mode is accidentally enabled elsewhere.
            .env("DOTNET_SYSTEM_GLOBALIZATION_INVARIANT", "0")
            .env("COMPlus_SYSTEM_GLOBALIZATION_INVARIANT", "0")
            .env("DOTNET_SYSTEM_GLOBALIZATION_PREDEFINED_CULTURES_ONLY", "1")
            .env("COMPlus_SYSTEM_GLOBALIZATION_PREDEFINED_CULTURES_ONLY", "1")
            .current_dir(&path);
        let output = recorded_caller_culture_command(&mut command, &path, culture)?;
        let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let culture_name = if culture == "invariant" { "" } else { culture };
        assert_eq!(report["culture"], culture_name, "{culture}: CurrentCulture");
        assert_eq!(
            report["uiCulture"], culture_name,
            "{culture}: CurrentUICulture"
        );
        for boundary in ["direct", "generated", "generatedBytes"] {
            assert_eq!(
                report[boundary],
                expected,
                "{culture}: {boundary}; complete artifacts at {}",
                path.display(),
            );
        }
    }
    drop(artifacts.0.take());
    Ok(())
}
