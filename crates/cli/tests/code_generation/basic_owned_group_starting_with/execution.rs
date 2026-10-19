fn qualify_rust_host(
    evidence: &Evidence,
    controls: &Json,
    legacy: &Json,
    commands: &Json,
    inputs: &mut PreparedInputFence,
) -> TestResult<bool> {
    let directory = &evidence.0;
    let route = &commands["execution_routes"]["cargo"];
    let cargo = PathBuf::from(
        route["invocation_path"]
            .as_str()
            .ok_or("bound Cargo route")?,
    );
    let target = directory.join("rust-build-target");
    fs::create_dir(&target)?;
    let manifest = directory.join("rust-host/Cargo.toml");
    let mut environment = child_environment(directory, false)?;
    environment.insert("RUSTFLAGS".into(), "-D warnings".into());
    for name in ["CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"] {
        environment.insert(name.into(), target.display().to_string());
    }
    environment.insert("CARGO_BUILD_JOBS".into(), "1".into());
    environment.insert("CARGO_INCREMENTAL".into(), "0".into());
    let mut build = command_with_bound_environment(&cargo, directory, &environment);
    build
        .args([
            "build",
            "--offline",
            "--jobs",
            "1",
            "--message-format=json",
            "--bin",
            "starting443-public-host",
            "--manifest-path",
        ])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target);
    let mut sources = vec![
        PathBuf::from("rust-host"),
        PathBuf::from("rust-legacy"),
        PathBuf::from("controls.json"),
        PathBuf::from("legacy-controls.json"),
        PathBuf::from(SUCCESS),
    ];
    sources.extend((0..MODELS).map(|index| PathBuf::from(format!("rust-model{index}"))));
    let built = record_bound_process(
        evidence,
        "starting443-rust-build",
        &mut build,
        &sources,
        inputs,
    )?;
    let selected = rust_artifact_path(&built.stdout, &manifest, &target);
    evidence.debug("RUST-COMPLETE-COMPILER-ARTIFACT-SELECTION.txt", &selected)?;
    if !built.status.success() {
        return Ok(false);
    }
    let executable = match selected {
        Ok(path) => path,
        Err(_) => return Ok(false),
    };
    let executable_binding = match bound_file(&executable) {
        Ok(binding) => binding,
        Err(error) => {
            evidence.error(
                "RUST-COMPLETE-EXECUTABLE-BINDING-FAILURE.txt",
                error.as_ref(),
            )?;
            return Ok(false);
        }
    };
    evidence.json("RUST-COMPLETE-PRE-RUN-EXECUTABLE.json", &executable_binding)?;
    // No `cargo run`, target runner, shell, PATH lookup or inherited loader hook.
    let output = directory.join("rust-actual");
    let contract_path = directory.join("run-contracts/rust.json");
    let argv = vec![
        directory.join("controls.json"),
        output.clone(),
        contract_path.clone(),
    ];
    let environment = child_environment(directory, false)?;
    let contract = make_host_contract(
        evidence,
        "rust",
        "rust",
        &executable_binding,
        None,
        &argv,
        &environment,
        &output,
    )?;
    let expected = expected_host_calls(
        controls,
        legacy,
        "rust",
        None,
        true,
        contract["run_id"].as_str().unwrap(),
    );
    if expected.len() != CORE_CALLS + LEGACY_CALLS {
        return Err("independent Rust invocation inventory is not 383".into());
    }
    evidence.json("run-contracts/rust.json", &contract)?;
    sources.push(PathBuf::from("run-contracts/rust.json"));
    let configuration = configuration_rows(directory)?;
    let mut run = command_with_bound_environment(&executable, directory, &environment);
    run.args(&argv);
    let ran = record_bound_process(
        evidence,
        "starting443-rust-host",
        &mut run,
        &sources,
        inputs,
    )?;
    // Inspect every original expected call even when the process fails or exits0
    // without running. Missing/partial/duplicate/stale/extra packets fail here.
    let proof = verify_host_directory(
        evidence,
        "rust",
        &contract,
        &expected,
        &output,
        &fs::read(directory.join("controls.json"))?,
    )?;
    let after = bound_file(&executable)?;
    evidence.json("RUST-COMPLETE-POST-RUN-EXECUTABLE.json", &after)?;
    Ok(ran.status.success()
        && proof
        && after == executable_binding
        && configuration == configuration_rows(directory)?)
}
fn qualify_csharp_host(
    evidence: &Evidence,
    controls: &Json,
    legacy: &Json,
    commands: &Json,
    index: usize,
    inputs: &mut PreparedInputFence,
) -> TestResult<bool> {
    let directory = &evidence.0;
    let ordinary = index == MODELS;
    let label = if ordinary {
        "legacy".to_owned()
    } else {
        format!("model{index}")
    };
    let host = if ordinary {
        "csharp-legacy-host".to_owned()
    } else {
        format!("csharp-host{index}")
    };
    let artifacts = directory.join(format!("dotnet-artifacts-{label}"));
    fs::create_dir(&artifacts)?;
    let route = &commands["execution_routes"]["dotnet"];
    let dotnet = PathBuf::from(
        route["invocation_path"]
            .as_str()
            .ok_or("bound dotnet route")?,
    );
    let program = bound_file(Path::new(
        route["resolved_path"].as_str().ok_or("resolved dotnet")?,
    ))?;
    let environment = child_environment(directory, true)?;
    let mut sources = vec![
        PathBuf::from(&host),
        PathBuf::from(format!("csharp-{label}")),
        PathBuf::from(SUCCESS),
        PathBuf::from("controls.json"),
        PathBuf::from("legacy-controls.json"),
    ];
    if ordinary {
        sources.extend([
            PathBuf::from("legacy-input.json"),
            PathBuf::from("legacy-expected.json"),
        ]);
    }
    let mut build = command_with_bound_environment(&dotnet, directory, &environment);
    build
        .args([
            "build",
            "--configuration",
            "Release",
            "--nologo",
            "--verbosity",
            "quiet",
            "--disable-build-servers",
            "-m:1",
            "-p:UseSharedCompilation=false",
            "--artifacts-path",
        ])
        .arg(&artifacts)
        .arg(directory.join(&host).join("Host.csproj"));
    let built = record_bound_process(
        evidence,
        &format!("starting443-csharp-{label}-build"),
        &mut build,
        &sources,
        inputs,
    )?;
    if !built.status.success() {
        return Ok(false);
    }
    let assembly = artifacts.join("bin/Host/release/Host.dll");
    let assembly_binding = match bound_file(&assembly) {
        Ok(binding) => binding,
        Err(error) => {
            evidence.error(
                &format!("csharp-{label}-COMPLETE-ASSEMBLY-BINDING-FAILURE.txt"),
                error.as_ref(),
            )?;
            return Ok(false);
        }
    };
    let binaries = hashes(artifact_files(&artifacts.join("bin"))?);
    evidence.json(&format!("csharp-{label}-PRE-RUN-BINARIES.json"), &binaries)?;
    let output = directory.join(format!("csharp-{label}-actual"));
    let contract_path = directory.join(format!("run-contracts/csharp-{label}.json"));
    let argv = if ordinary {
        vec![
            directory.join("legacy-input.json"),
            directory.join("legacy-expected.json"),
            output.clone(),
            contract_path.clone(),
        ]
    } else {
        vec![
            directory.join("controls.json"),
            PathBuf::from(index.to_string()),
            output.clone(),
            contract_path.clone(),
        ]
    };
    let contract = make_host_contract(
        evidence,
        &label,
        "csharp",
        &program,
        Some(&assembly_binding),
        &argv,
        &environment,
        &output,
    )?;
    let expected = if ordinary {
        expected_host_calls(
            &json!({"cases":[]}),
            legacy,
            "csharp",
            None,
            true,
            contract["run_id"].as_str().unwrap(),
        )
    } else {
        expected_host_calls(
            controls,
            legacy,
            "csharp",
            Some(index),
            false,
            contract["run_id"].as_str().unwrap(),
        )
    };
    if expected.is_empty() || ordinary && expected.len() != LEGACY_CALLS {
        return Err("independent C# host invocation inventory differs".into());
    }
    evidence.json(&format!("run-contracts/csharp-{label}.json"), &contract)?;
    sources.push(PathBuf::from(format!("run-contracts/csharp-{label}.json")));
    let configuration = configuration_rows(directory)?;
    let mut run = command_with_bound_environment(&dotnet, directory, &environment);
    run.arg(&assembly).args(&argv);
    let ran = record_bound_process(
        evidence,
        &format!("starting443-csharp-{label}-host"),
        &mut run,
        &sources,
        inputs,
    )?;
    let frozen = fs::read(directory.join(if ordinary {
        "legacy-controls.json"
    } else {
        "controls.json"
    }))?;
    let proof = verify_host_directory(
        evidence,
        &format!("csharp-{label}"),
        &contract,
        &expected,
        &output,
        &frozen,
    )?;
    let after = hashes(artifact_files(&artifacts.join("bin"))?);
    evidence.json(&format!("csharp-{label}-POST-RUN-BINARIES.json"), &after)?;
    Ok(ran.status.success()
        && proof
        && after == binaries
        && bound_file(&assembly)? == assembly_binding
        && bound_file(Path::new(route["resolved_path"].as_str().unwrap()))? == program
        && configuration == configuration_rows(directory)?)
}
#[test]
#[ignore = "unchanged successful preparation, independent v2 helper/runtime acceptance and serialized compiler/native authority required"]
fn basic_starting443_qualify_native_and_public_consumers() -> TestResult<()> {
    let directory = PathBuf::from(
        std::env::var_os(ENVIRONMENT).ok_or("successful preparation directory required")?,
    );
    if directory != bound_execution_root()? || !directory.is_dir() {
        return Err("reviewed successful preparation root required".into());
    }
    let success_bytes = fs::read(directory.join(SUCCESS))?;
    let success: Json = serde_json::from_slice(&success_bytes)?;
    if success != binding(&directory)? {
        return Err("prepared original/source/tool/configuration binding changed".into());
    }
    let (controls, projects, _) = corpus()?;
    let (legacy, _) = legacy_native_controls()?;
    if fs::read(directory.join("controls.json"))? != serde_json::to_vec_pretty(&controls)?
        || fs::read(directory.join("legacy-controls.json"))? != serde_json::to_vec_pretty(&legacy)?
    {
        return Err("frozen literal controls differ".into());
    }
    let evidence = Evidence(directory.clone());
    let mut inputs = PreparedInputFence::new(&success, &success_bytes)?;
    inputs.check(&evidence, "execution-entry", false)?;
    for relative in [".runtime-temp", ".dotnet-home", ".nuget-packages"] {
        fs::create_dir(directory.join(relative))?;
    }
    let native = run_native(&controls, &projects, &directory)?;
    let ordinary_native = legacy_native(&evidence)?;
    evidence.json("ALL-COMPLETE-NATIVE-COMPARISONS.json", &native)?;
    evidence.json(
        "ALL-COMPLETE-LEGACY-NATIVE-COMPARISONS.json",
        &ordinary_native,
    )?;
    let mut statuses = Vec::new();
    statuses.push((
        "rust".to_owned(),
        qualify_rust_host(
            &evidence,
            &controls,
            &legacy,
            &success["commands"],
            &mut inputs,
        )?,
    ));
    for index in 0..=MODELS {
        statuses.push((
            format!("csharp-{index}"),
            qualify_csharp_host(
                &evidence,
                &controls,
                &legacy,
                &success["commands"],
                index,
                &mut inputs,
            )?,
        ));
    }
    evidence.debug("ALL-GENERATED-EXECUTION-STATUSES.txt", &statuses)?;
    inputs.check(&evidence, "execution-final", false)?;
    verify_original_evidence(&directory, &success["evidence"])?;
    if success["sources"] != serde_json::to_value(source_bindings()?)?
        || success["commands"] != command_runtime_binding()?
    {
        return Err(
            "source/compiler/native-binary/configuration closure changed during qualification"
                .into(),
        );
    }
    if native.len() != CORE_CALLS
        || ordinary_native.len() != LEGACY_CALLS
        || native
            .iter()
            .chain(&ordinary_native)
            .any(|row| row["matched"] != true)
        || statuses.len() != MODELS + 2
        || statuses.iter().any(|(_, matched)| !*matched)
    {
        return Err("Starting qualification differs; complete raw/independent post-call evidence retained; exit alone never qualifies".into());
    }
    Ok(())
}
