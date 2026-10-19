fn closed_projects() -> Vec<(String, Project, Vec<codegen::Diagnostic>)> {
    closed::projects()
        .into_iter()
        .chain(closed_specialized::projects())
        .map(|(id, p, e)| (id.into(), p, e))
        .chain(closed_matrix::projects())
        .collect()
}
fn closed_programs(
    base: &codegen::Program,
    specialized: &codegen::Program,
) -> Vec<(String, codegen::Program, codegen::ProgramValidationError)> {
    closed::programs(base)
        .into_iter()
        .chain(closed_specialized::programs(specialized))
        .map(|(id, p, e)| (id.into(), p, e))
        .chain(closed_matrix::programs(base))
        .collect()
}
fn check_closed_projects(
    evidence: &Evidence,
    controls: &[(String, Project, Vec<codegen::Diagnostic>)],
) -> TestResult<Vec<(String, bool)>> {
    if controls.len() != CLOSED_PROJECTS
        || controls
            .iter()
            .map(|(id, _, _)| id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != CLOSED_PROJECTS
    {
        return Err("65 unique negative Project controls required".into());
    }
    let mut checks = Vec::new();
    for (index, (id, project, expected)) in controls.iter().enumerate() {
        let label = format!("closed/projects/{index:02}");
        evidence.debug(
            &format!("{label}-COMPLETE-AUTHORED.txt"),
            &(id, project, expected),
        )?;
        let before = mapping::project_file::encode_pretty(project)?;
        evidence.write(
            &format!("{label}-COMPLETE-INPUT-BEFORE.json"),
            before.as_bytes(),
        )?;
        let (_, matched) = recorded(
            evidence,
            &label,
            || codegen::lower_with_basic_owned_group_starting_with(project),
            debug_capture,
            |actual| matches!(actual,Err(error) if error.diagnostics() == expected),
        )?;
        let after = mapping::project_file::encode_pretty(project)?;
        evidence.write(
            &format!("{label}-COMPLETE-INPUT-AFTER.json"),
            after.as_bytes(),
        )?;
        checks.push((id.clone(), matched && before == after));
    }
    Ok(checks)
}
fn check_direct_programs(
    evidence: &Evidence,
    gate: &EmissionGate,
    runtime: &Path,
    controls: &[(String, codegen::Program, codegen::ProgramValidationError)],
) -> TestResult<Vec<(String, bool)>> {
    if controls.len() != DIRECT_PROGRAMS
        || controls
            .iter()
            .map(|(id, _, _)| id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != DIRECT_PROGRAMS
    {
        return Err("61 unique direct Programs required".into());
    }
    let options = codegen_rust::Options {
        package_name: "starting443-direct-refusal".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    evidence.debug("closed/COMPLETE-DIRECT-RUST-OPTIONS.txt", &options)?;
    let mut checks = Vec::new();
    for (index, (id, program, expected)) in controls.iter().enumerate() {
        let label = format!("closed/programs/{index:02}");
        let before = program.clone();
        evidence.debug(
            &format!("{label}-COMPLETE-AUTHORED.txt"),
            &(id, program, expected),
        )?;
        // Every surface is an independent captured call. An unexpected success,
        // typed mismatch, invocation panic or observation panic skips none later.
        let (_, v) = recorded(
            evidence,
            &format!("{label}-validator"),
            || codegen::validate_program_with_basic_owned_group_starting_with(program),
            debug_capture,
            |actual| matches!(actual,Err(error) if error == expected),
        )?;
        let vi = input_after(evidence, &format!("{label}-validator"), program, &before)?;
        let (_, e) = recorded(
            evidence,
            &format!("{label}-emission-validator"),
            || codegen::validate_basic_owned_group_starting_with_emission(program),
            debug_capture,
            |actual| matches!(actual,Err(error) if error == expected),
        )?;
        let ei = input_after(
            evidence,
            &format!("{label}-emission-validator"),
            program,
            &before,
        )?;
        let (_, c) = recorded(
            evidence,
            &format!("{label}-catalogue"),
            || {
                codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(
                    program,
                )
            },
            actual_catalogues,
            |actual| matches!(actual,Err(error) if error == expected),
        )?;
        let ci = input_after(evidence, &format!("{label}-catalogue"), program, &before)?;
        gate.check()?;
        let (rust, r) = recorded(
            evidence,
            &format!("{label}-rust"),
            || codegen_rust::emit_with_basic_owned_group_starting_with(program, &options),
            artifact_capture,
            |actual| matches!(actual,Err(codegen_rust::EmitError::InvalidProgram(error)) if error == expected),
        )?;
        let ri = input_after(evidence, &format!("{label}-rust"), program, &before)?;
        if let Some(artifacts) = rust {
            evidence.artifacts(&format!("{label}-UNEXPECTED-RUST-SUCCESS"), &artifacts)?;
        }
        gate.check()?;
        let (cs, s) = recorded(
            evidence,
            &format!("{label}-csharp"),
            || codegen_csharp::emit_with_basic_owned_group_starting_with(program),
            artifact_capture,
            |actual| matches!(actual,Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == expected),
        )?;
        let si = input_after(evidence, &format!("{label}-csharp"), program, &before)?;
        if let Some(artifacts) = cs {
            evidence.artifacts(&format!("{label}-UNEXPECTED-CSHARP-SUCCESS"), &artifacts)?;
        }
        gate.check()?;
        evidence.json(
            &format!("{label}-FIVE-SURFACE-COMPARISONS.json"),
            &json!({"id":id,
            "validator":v,"emission_validator":e,"catalogue":c,"rust":r,"csharp":s,
            "input_after_each":[vi,ei,ci,ri,si]}),
        )?;
        checks.push((
            id.clone(),
            v && e && c && r && s && vi && ei && ci && ri && si,
        ));
    }
    Ok(checks)
}
fn error_capture<E: std::error::Error + 'static>(error: &E) -> Json {
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(error) = source {
        causes.push(json!({"debug":format!("{error:#?}"),"display":error.to_string()}));
        source = error.source();
    }
    json!({"debug":format!("{error:#?}"),"display":error.to_string(),"complete_causes":causes,"terminated":true})
}
fn full_result<T: std::fmt::Debug, E: std::error::Error + 'static>(
    result: &Result<T, E>,
    capture: impl FnOnce(&T) -> Json,
) -> Json {
    match result {
        Ok(value) => json!({"Ok":capture(value)}),
        Err(error) => json!({"Err":error_capture(error)}),
    }
}
fn old_family_controls(
    project: &Project,
    program: &codegen::Program,
    evidence: &Evidence,
    runtime: &Path,
    gate: &EmissionGate,
) -> TestResult<bool> {
    let expected = closed::grouping_error();
    let diagnostic = match &expected {
        codegen::ProgramValidationError::OwnedSource { diagnostic } => {
            codegen::Diagnostic::OwnedSource {
                diagnostic: diagnostic.clone(),
            }
        }
        _ => unreachable!("literal Grouping diagnostic"),
    };
    let options = codegen_rust::Options {
        package_name: "starting443-old-gates".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    evidence.debug(
        "old-families-COMPLETE-AUTHORED.txt",
        &(project, program, &options, &expected, &diagnostic),
    )?;
    let project_before = mapping::project_file::encode_pretty(project)?;
    let before = program.clone();
    gate.check()?;
    // The frozen constructor explicitly calls all 50 old surfaces including
    // Adjacent, distinct default/Basic emitters and all eight lowering routes.
    let invocation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Ok::<_, io::Error>(old_profiles::products(project, program, &options))
    }));
    let observation = codec::retain_outcome(
        invocation,
        |suffix, bytes| evidence.write_io(&format!("old-families-{suffix}"), bytes),
        |products| {
            json!({
                "lowering":products.lowering.iter().map(|r|full_result(r,debug_capture)).collect::<Vec<_>>(),
                "validators":products.validators.iter().map(|r|full_result(r,debug_capture)).collect::<Vec<_>>(),
                "catalogues":products.catalogues.iter().map(|r|full_result(r,actual_catalogues)).collect::<Vec<_>>(),
                "rust":products.rust.iter().map(|r|full_result(r,artifact_capture)).collect::<Vec<_>>(),
                "csharp":products.csharp.iter().map(|r|full_result(r,artifact_capture)).collect::<Vec<_>>()
            })
        },
        |result| {
            let matched = result.as_ref().is_ok_and(|p| {
                p.lowering.len() == 8 && p.validators.len() == 16 && p.catalogues.len() == 8
                    && p.rust.len() == 9 && p.csharp.len() == 9
                    && p.lowering.len()+p.validators.len()+p.catalogues.len()+p.rust.len()+p.csharp.len() == OLD_SURFACES
                    && p.lowering.iter().all(|r|matches!(r,Err(error) if error.diagnostics() == std::slice::from_ref(&diagnostic)))
                    && p.validators.iter().all(|r|matches!(r,Err(error) if *error == expected))
                    && p.catalogues.iter().all(|r|matches!(r,Err(error) if *error == expected))
                    && p.rust.iter().all(|r|matches!(r,Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected))
                    && p.csharp.iter().all(|r|matches!(r,Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected))
            });
            json!({"matched":matched,"required_surfaces":OLD_SURFACES})
        },
    )?;
    evidence.write(
        "old-families-COMPLETE-PROJECT-INPUT-AFTER.json",
        mapping::project_file::encode_pretty(project)?.as_bytes(),
    )?;
    let unchanged = input_after(evidence, "old-families", program, &before)?
        && mapping::project_file::encode_pretty(project)? == project_before;
    gate.check()?;
    Ok(observation.is_some_and(|row| row["matched"] == true) && unchanged)
}
