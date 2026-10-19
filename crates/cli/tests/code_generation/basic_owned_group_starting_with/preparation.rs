fn emitted_runtime_bound(
    artifacts: &codegen::ArtifactSet,
    gate: &EmissionGate,
    historical: &[legacy_artifact_policy::ManifestRow],
) -> bool {
    let actual = artifact_manifest(artifacts);
    // Full80 path/order/duplicate validation, with all original runtime rows and
    // both fixed N additions. Only ordinary baseline compares mapping bytes.
    legacy_artifact_policy::compare_starting_model_merged_json42686c55(
        historical,
        &actual,
        &gate.reviewed,
    )
    .accepts_model_runtime(
        legacy_artifact_policy::Policy::ReviewedStartingRuntimeWithMergedJson42686c55,
    )
}
fn emitted_runtime_report(
    artifacts: &codegen::ArtifactSet,
    gate: &EmissionGate,
    historical: &[legacy_artifact_policy::ManifestRow],
) -> Json {
    let actual = artifact_manifest(artifacts);
    let comparison = legacy_artifact_policy::compare_starting_model_merged_json42686c55(
        historical,
        &actual,
        &gate.reviewed,
    );
    json!({"policy":"ReviewedStartingRuntimeWithMergedJson42686c55","scope":"model-runtime-conservation",
        "complete_unmodified_actual_manifest":actual,"paths_exact":comparison.paths_exact,"historical_exact":comparison.historical_exact,
        "historical_runtime_conserved":comparison.historical_runtime_conserved,"original_78_conserved":comparison.original_78_conserved,
        "ordinary_mapping_bytes_exact":comparison.ordinary_mapping_bytes_exact,"fixed_merged_additions_exact":comparison.fixed_merged_additions_exact,
        "reviewed_three_sources_exact":comparison.runtime_sources_exact,"accepted":comparison.accepts_model_runtime(legacy_artifact_policy::Policy::ReviewedStartingRuntimeWithMergedJson42686c55)})
}
fn lower_base(
    evidence: &Evidence,
    label: &str,
    project: &Project,
) -> TestResult<Option<codegen::Program>> {
    let before = mapping::project_file::encode_pretty(project)?;
    evidence.write(
        &format!("{label}-COMPLETE-PROJECT-BEFORE.json"),
        before.as_bytes(),
    )?;
    let (program, matched) = recorded(
        evidence,
        label,
        || codegen::lower_with_basic_owned_group_starting_with(project),
        debug_capture,
        Result::is_ok,
    )?;
    let after = mapping::project_file::encode_pretty(project)?;
    evidence.write(
        &format!("{label}-COMPLETE-PROJECT-AFTER.json"),
        after.as_bytes(),
    )?;
    Ok(if matched && before == after {
        program
    } else {
        None
    })
}

#[test]
#[ignore = "future companion APIs, independently accepted candidate source/build review and fresh absent evidence root required"]
fn basic_starting443_prepare_public_projects_and_callers() -> TestResult<()> {
    let evidence = Evidence::new()?;
    if evidence.0 != bound_execution_root()? {
        return Err("preparation root must be the independently reviewed execution root".into());
    }
    let fence = source_bindings()?;
    let commands = command_runtime_binding()?;
    evidence.json("COMPLETE-PREPARATION-SOURCE-BINDINGS.json", &fence)?;
    evidence.json("COMPLETE-PREPARATION-COMMAND-BINDING.json", &commands)?;
    let (controls, models, profiles) = corpus()?;
    let (legacy_controls, legacy_project) = legacy_native_controls()?;
    let closed = closed_projects();
    if closed.len() != CLOSED_PROJECTS {
        return Err("negative Project inventory differs".into());
    }
    let functional_base = projects::base();
    let specialized_base = projects::refusal_base();
    let s01_project = projects::project("S01");
    // All independent public inputs, expected trees/errors/catalogues/selectors,
    // refusal Projects and both distinct base Projects precede all lowering.
    evidence.json("controls.json", &controls)?;
    evidence.json(
        "legacy-native-COMPLETE-AUTHORED-CONTROLS.json",
        &legacy_controls,
    )?;
    evidence.json("legacy-controls.json", &legacy_controls)?;
    evidence.debug(
        "legacy-native-COMPLETE-AUTHORED-PROJECT.txt",
        &legacy_project,
    )?;
    evidence.debug("ALL-COMPLETE-PUBLIC-PROJECTS.txt", &models)?;
    evidence.debug("closed/ALL-COMPLETE-AUTHORED-PROJECTS.txt", &closed)?;
    evidence.debug(
        "closed/COMPLETE-AUTHORED-DISTINCT-BASE-PROJECTS.txt",
        &(&functional_base, &specialized_base),
    )?;
    evidence.debug(
        "old-families-COMPLETE-AUTHORED-S01-PROJECT.txt",
        &s01_project,
    )?;
    let old_expected = closed::grouping_error();
    let old_diagnostic = match &old_expected {
        codegen::ProgramValidationError::OwnedSource { diagnostic } => {
            codegen::Diagnostic::OwnedSource {
                diagnostic: diagnostic.clone(),
            }
        }
        _ => unreachable!("literal old-family Grouping refusal"),
    };
    evidence.debug(
        "old-families-COMPLETE-AUTHORED-EXPECTED.txt",
        &(&old_expected, &old_diagnostic),
    )?;
    evidence.json(
        "ALL-COMPLETE-AUTHORED-CATALOGUES.json",
        &profiles
            .iter()
            .map(|profile| json!({"profile":profile,"expected":catalogues::expected(profile)}))
            .collect::<Vec<_>>(),
    )?;
    for (index, project) in models.iter().enumerate() {
        evidence.write(
            &format!("model{index}-project.json"),
            mapping::project_file::encode_pretty(project)?.as_bytes(),
        )?;
    }
    let runtime = runtime_path()?;
    let baseline = retain_historical(&evidence, &runtime)?;
    let authored_fence = serde_json::to_value(hashes(artifact_files(&evidence.0)?))?;
    evidence.json(
        "COMPLETE-AUTHORED-INPUT-AND-ORACLE-FENCE.json",
        &authored_fence,
    )?;
    // Fail closed, before any emitter (including negative/old-family emitters).
    // This is an external independent review gate, never an auto-approval path.
    let gate = accepted_emission_gate(&evidence)?;
    let base = lower_base(&evidence, "closed/functional-base", &functional_base)?;
    let specialized = lower_base(&evidence, "closed/specialized-base", &specialized_base)?;
    let s01 = lower_base(&evidence, "old-families/S01-new-companion", &s01_project)?;
    let direct = match (&base, &specialized) {
        (Some(base), Some(specialized)) => {
            let (products, _) = recorded(
                &evidence,
                "closed/direct-constructor",
                || Ok::<_, io::Error>(closed_programs(base, specialized)),
                debug_capture,
                Result::is_ok,
            )?;
            products
        }
        _ => None,
    };
    // Retain direct Program recipes and literal expected diagnostics before ANY
    // candidate emission. A successful lowered base never supplies an oracle.
    evidence.debug("closed/ALL-COMPLETE-DIRECT-PROGRAMS-AUTHORED.txt", &direct)?;
    let direct_fence = serde_json::to_value(hashes(artifact_files(&evidence.0)?))?;
    evidence.json("COMPLETE-PRE-EMISSION-ORIGINAL-FENCE.json", &direct_fence)?;
    let direct_checks = if let Some(controls) = &direct {
        check_direct_programs(&evidence, &gate, &runtime, controls)?
    } else {
        evidence.write("closed/DIRECT-CONTROLS-BLOCKED.txt",b"A retained base lowering or the immutable aggregate constructor failed/panicked. No partial direct controls were invented; later models and negative Projects still attempted.\n")?;
        vec![(
            "direct constructor or distinct bases unavailable".into(),
            false,
        )]
    };
    let old_families = match &s01 {
        Some(program) => old_family_controls(&s01_project, program, &evidence, &runtime, &gate)?,
        None => false,
    };
    let mut statuses = Vec::new();
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, project) in models.iter().enumerate() {
        let label = format!("model{index}");
        let options = codegen_rust::Options {
            package_name: format!("starting443-model{index}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        evidence.debug(&format!("{label}-COMPLETE-RUST-OPTIONS.txt"), &options)?;
        let project_before = mapping::project_file::encode_pretty(project)?;
        let (_, native_valid) = recorded(
            &evidence,
            &format!("{label}-validation"),
            || Ok::<_, io::Error>(engine::validate(project)),
            debug_capture,
            |actual| actual.as_ref().is_ok_and(Vec::is_empty),
        )?;
        let validation_after = mapping::project_file::encode_pretty(project)?;
        evidence.write(
            &format!("{label}-validation-COMPLETE-INPUT-AFTER.json"),
            validation_after.as_bytes(),
        )?;
        let (program, lower_ok) = recorded(
            &evidence,
            &format!("{label}-lowering"),
            || codegen::lower_with_basic_owned_group_starting_with(project),
            debug_capture,
            Result::is_ok,
        )?;
        let lower_after = mapping::project_file::encode_pretty(project)?;
        evidence.write(
            &format!("{label}-lowering-COMPLETE-INPUT-AFTER.json"),
            lower_after.as_bytes(),
        )?;
        let mut matched = native_valid
            && lower_ok
            && validation_after == project_before
            && lower_after == project_before;
        if let Some(program) = program {
            let before = program.clone();
            let (_, catalogue) = recorded(
                &evidence,
                &format!("{label}-catalogue"),
                || {
                    codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(
                        &program,
                    )
                },
                actual_catalogues,
                |actual| {
                    actual.as_ref().is_ok_and(|plan| {
                        actual_catalogues(plan) == catalogues::expected(&profiles[index])
                    })
                },
            )?;
            let ci = input_after(&evidence, &format!("{label}-catalogue"), &program, &before)?;
            let (_, valid) = recorded(
                &evidence,
                &format!("{label}-validator"),
                || codegen::validate_program_with_basic_owned_group_starting_with(&program),
                debug_capture,
                Result::is_ok,
            )?;
            let vi = input_after(&evidence, &format!("{label}-validator"), &program, &before)?;
            let (_, emission) = recorded(
                &evidence,
                &format!("{label}-emission-validator"),
                || codegen::validate_basic_owned_group_starting_with_emission(&program),
                debug_capture,
                Result::is_ok,
            )?;
            let ei = input_after(
                &evidence,
                &format!("{label}-emission-validator"),
                &program,
                &before,
            )?;
            gate.check()?;
            let (rust, rust_ok) = recorded(
                &evidence,
                &format!("{label}-rust"),
                || codegen_rust::emit_with_basic_owned_group_starting_with(&program, &options),
                artifact_capture,
                Result::is_ok,
            )?;
            let ri = input_after(&evidence, &format!("{label}-rust"), &program, &before)?;
            if let Some(artifacts) = rust {
                evidence.artifacts(&format!("rust-model{index}"), &artifacts)?;
            }
            gate.check()?;
            let (cs, cs_ok) = recorded(
                &evidence,
                &format!("{label}-csharp"),
                || codegen_csharp::emit_with_basic_owned_group_starting_with(&program),
                artifact_capture,
                Result::is_ok,
            )?;
            let si = input_after(&evidence, &format!("{label}-csharp"), &program, &before)?;
            let embedded = cs
                .as_ref()
                .is_some_and(|a| emitted_runtime_bound(a, &gate, &baseline.1));
            if let Some(artifacts) = &cs {
                evidence.json(
                    &format!("{label}-COMPLETE-FULL80-RUNTIME-CONSERVATION.json"),
                    &emitted_runtime_report(artifacts, &gate, &baseline.1),
                )?;
            }
            if let Some(artifacts) = cs {
                evidence.artifacts(&format!("csharp-model{index}"), &artifacts)?;
            }
            gate.check()?;
            evidence.json(&format!("{label}-COMPLETE-COMPARISONS.json"),&json!({"catalogue":catalogue,
                "validator":valid,"emission_validator":emission,"rust":rust_ok,"csharp":cs_ok,
                "emitted_runtime_bound_to_independent_review":embedded,"input_after_each":[ci,vi,ei,ri,si]}))?;
            matched &= catalogue
                && valid
                && emission
                && rust_ok
                && cs_ok
                && embedded
                && ci
                && vi
                && ei
                && ri
                && si;
        } else {
            matched = false;
        }
        statuses.push((index, matched));
        // Controls, not a fixed calls/model heuristic, determine each host total.
        let calls = controls["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["model"] == index)
            .map(|c| c["calls"].as_array().unwrap().len())
            .sum::<usize>();
        evidence.write(
            &format!("csharp-host{index}/Host.csproj"),
            host_project(&format!("model{index}")).as_bytes(),
        )?;
        evidence.write(
            &format!("csharp-host{index}/Program.cs"),
            csharp_host(calls)?.as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"starting443-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!("{index}=>invoke!(model{index},selection,&source,&inputs,&execution,limits,&directory,&prefix,call,&before,&named,&identity,&raw_before),\n"));
    }
    let closed_checks = check_closed_projects(&evidence, &closed)?;
    let legacy = prepare_legacy(&evidence, &runtime, &gate, &baseline)?;
    evidence.debug(
        "ALL-COMPLETE-PREPARATION-COMPARISONS.txt",
        &(
            &statuses,
            &direct_checks,
            &closed_checks,
            old_families,
            legacy,
        ),
    )?;
    evidence.write(
        "rust-host/src/main.rs",
        rust_host(&dispatch, CORE_CALLS)?.as_bytes(),
    )?;
    evidence.write(
        "rust-host/src/codec.rs",
        include_bytes!("../basic_owned_sequence_item_at/codec.rs"),
    )?;
    evidence.write("rust-host/src/proof.rs", include_bytes!("HostProof.rs.txt"))?;
    evidence.write("rust-host/Cargo.toml",format!("[package]\nname=\"starting443-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\nlegacy0={{package=\"ferrule-generated-mapping\",path=\"../rust-legacy\"}}\n{dependencies}",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;
    // Required evidence writes and source-review/fence failures are fatal.
    // Typed/mapping/observer mismatches instead reach this final aggregate gate.
    verify_original_evidence(&evidence.0, &authored_fence)?;
    verify_original_evidence(&evidence.0, &direct_fence)?;
    gate.check()?;
    if fence != source_bindings()? || commands != command_runtime_binding()? {
        return Err("source/build closure changed during preparation".into());
    }
    if statuses.len() != MODELS
        || statuses.iter().any(|(_, ok)| !*ok)
        || direct_checks.len() != DIRECT_PROGRAMS
        || direct_checks.iter().any(|(_, ok)| !*ok)
        || closed_checks.len() != CLOSED_PROJECTS
        || closed_checks.iter().any(|(_, ok)| !*ok)
        || !old_families
        || !legacy
    {
        return Err("starting443 preparation differs; all originals retained; no compiler/native evaluator started".into());
    }
    evidence.json(SUCCESS, &binding(&evidence.0)?)?;
    Ok(())
}
