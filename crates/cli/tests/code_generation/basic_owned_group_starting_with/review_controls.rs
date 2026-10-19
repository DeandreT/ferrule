#[test]
fn basic_starting443_noop_runner_and_incomplete_host_packets_are_rejected() -> TestResult<()> {
    for value in ["", "/bin/true"] {
        assert!(
            reject_execution_routes(&BTreeMap::from([(
                "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER".into(),
                value.into()
            )]))
            .is_err()
        );
    }
    let manifest = Path::new("/bound/rust-host/Cargo.toml");
    let target = Path::new("/fresh-native-target");
    assert!(
        rust_artifact_path(b"", manifest, target).is_err(),
        "a no-op build has no executable authority"
    );
    let executable = target.join("debug").join(if cfg!(windows) {
        "starting443-public-host.exe"
    } else {
        "starting443-public-host"
    });
    let event = json!({"reason":"compiler-artifact","manifest_path":manifest,"target":{"name":"starting443-public-host","kind":["bin"]},"profile":{"test":false},"executable":executable});
    let event_line = serde_json::to_string(&event)?;
    assert_eq!(
        rust_artifact_path(event_line.as_bytes(), manifest, target)?,
        executable
    );
    assert!(
        rust_artifact_path(
            format!("{event_line}\n{event_line}\n").as_bytes(),
            manifest,
            target
        )
        .is_err()
    );
    let source = json!(["Int", 1]);
    let result = json!(["Int", 2]);
    let case = json!({"id":"CONTROL","model":0,"source":source,"inputs":[]});
    let calls = [
        json!({"selection":"All","expected":{"Ok":[[null,result]]}}),
        json!({"selection":"Primary","expected":{"Ok":[[null,result]]}}),
    ];
    let expected = calls
        .iter()
        .map(|call| {
            host_call_identity(
                "rust",
                "fresh",
                0,
                &case,
                call,
                &format!("00-{}", call["selection"].as_str().unwrap()),
            )
        })
        .collect::<Vec<_>>();
    let contract = json!({"version":2,"engine":"rust","run_id":"fresh","host_identity":{"program":"/bound/actual-host"}});
    let frozen = b"independent frozen controls";
    let mut files = vec![
        ("RUN-CONTRACT.json".into(), serde_json::to_vec(&contract)?),
        (
            "HOST-IDENTITY.json".into(),
            serde_json::to_vec(&contract["host_identity"])?,
        ),
        ("COMPLETE-FROZEN-CONTROLS.json".into(), frozen.to_vec()),
        (
            "ALL-COMPLETE-COMPARISONS.json".into(),
            serde_json::to_vec(&json!(
                expected
                    .iter()
                    .map(|id| json!({"invocation":id,"matched":true}))
                    .collect::<Vec<_>>()
            ))?,
        ),
    ];
    for identity in &expected {
        let slot = identity["slot"].as_str().unwrap();
        files.push((
            format!("{slot}-COMPLETE-INVOCATION.json"),
            serde_json::to_vec(identity)?,
        ));
        let before = json!({"source":source,"inputs":[]});
        for (suffix, record) in [
            (
                "COMPLETE-INPUT.json",
                json!({"before_raw":before,"source_recipe":source,"inputs_recipe":[]}),
            ),
            (
                "COMPLETE-INPUT-AFTER.json",
                json!({"state":before,"unchanged":true}),
            ),
            (
                "COMPLETE-RAW-PRODUCT.json",
                json!({"Ok":expected_rust_raw(&identity["call"]).unwrap()}),
            ),
            (
                "COMPLETE-OBSERVATION.json",
                json!({"actual_trees":[[null,result]],"matched":true,"input_after_complete":true,"input_after_unchanged":true}),
            ),
        ] {
            files.push((
                format!("{slot}-{suffix}"),
                serde_json::to_vec(&json!({"invocation":identity,"record":record}))?,
            ));
        }
        for suffix in ["COMPLETE-INPUT.txt", "COMPLETE-RAW-PRODUCT.txt"] {
            files.push((
                format!("{slot}-{suffix}"),
                format!(
                    "INVOCATION={}\ncomplete original\n",
                    serde_json::to_string(identity)?
                )
                .into_bytes(),
            ));
        }
    }
    // Fixed synthetic transport controls, not mapping products or campaign calls.
    assert!(verify_complete_host_files(
        &contract, &expected, &files, frozen
    ));
    assert!(
        !verify_complete_host_files(&contract, &expected, &Vec::new(), frozen),
        "exit0/no output is not proof"
    );
    let mut missing = files.clone();
    missing.retain(|(path, _)| path != "00-All-COMPLETE-RAW-PRODUCT.json");
    let mut duplicate = files.clone();
    duplicate
        .iter_mut()
        .find(|(p, _)| p == "ALL-COMPLETE-COMPARISONS.json")
        .unwrap()
        .1 = serde_json::to_vec(
        &json!([{"invocation":expected[0],"matched":true},{"invocation":expected[0],"matched":true}]),
    )?;
    let mut stale = files.clone();
    let entry = stale
        .iter_mut()
        .find(|(p, _)| p == "00-All-COMPLETE-RAW-PRODUCT.json")
        .unwrap();
    let mut value: Json = serde_json::from_slice(&entry.1)?;
    value["invocation"]["run_id"] = json!("stale");
    entry.1 = serde_json::to_vec(&value)?;
    let mut extra = files.clone();
    extra.push(("stale-extra.json".into(), b"{}".to_vec()));
    let mut changed_after = files.clone();
    let entry = changed_after
        .iter_mut()
        .find(|(p, _)| p == "00-All-COMPLETE-INPUT-AFTER.json")
        .unwrap();
    let mut value: Json = serde_json::from_slice(&entry.1)?;
    value["record"]["state"]["source"] = json!(["Int", 999]);
    entry.1 = serde_json::to_vec(&value)?;
    for invalid in [missing, duplicate, stale, extra, changed_after] {
        assert!(!verify_complete_host_files(
            &contract, &expected, &invalid, frozen
        ));
    }
    let mut duplicated_paths = files.clone();
    duplicated_paths.push(files[0].clone());
    assert!(!verify_complete_host_files(
        &contract,
        &expected,
        &duplicated_paths,
        frozen
    ));
    Ok(())
}

#[test]
fn basic_starting443_input_after_survives_bad_output_observer_invocation_and_snapshot_failure()
-> TestResult<()> {
    let temp = TempDir::new("starting443-post-call")?;
    let evidence = Evidence(temp.0.clone());
    let live = std::cell::RefCell::new(Instance::Scalar(Value::Int(1)));
    for (label, output) in [
        (
            "mapped",
            Instance::MappedSequence(vec![Instance::Scalar(Value::Int(7))]),
        ),
        (
            "documents",
            Instance::DocumentSet(vec![
                ir::DocumentMember::new("retained.xml", Instance::Scalar(Value::Int(7))).unwrap(),
            ]),
        ),
    ] {
        let before = codec::raw_tree(&live.borrow());
        let invocation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            *live.borrow_mut() = Instance::Scalar(Value::Int(2));
            Ok::<_, io::Error>(output)
        }));
        let observed = codec::retain_outcome_with_input_after(
            invocation,
            |s, b| evidence.write_io(&format!("{label}-{s}"), b),
            codec::raw_tree,
            || {
                let state = codec::raw_tree(&live.borrow());
                json!({"unchanged":state==before,"state":state})
            },
            |actual| {
                codec::encode(actual.as_ref().unwrap());
                json!({"matched":true})
            },
        )?;
        assert!(observed.is_none());
        let retained: Json = serde_json::from_slice(&fs::read(
            temp.0.join(format!("{label}-COMPLETE-INPUT-AFTER.json")),
        )?)?;
        assert_eq!(retained["state"], json!(["Int", 2]));
        assert!(
            temp.0
                .join(format!("{label}-COMPLETE-RAW-PRODUCT.json"))
                .is_file()
        );
        *live.borrow_mut() = Instance::Scalar(Value::Int(1));
    }
    let invocation: std::thread::Result<Result<Instance, io::Error>> =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            *live.borrow_mut() = Instance::Scalar(Value::Int(3));
            panic!("original invocation sentinel")
        }));
    assert!(
        codec::retain_outcome_with_input_after(
            invocation,
            |s, b| evidence.write_io(&format!("panic-{s}"), b),
            codec::raw_tree,
            || json!({"state":codec::raw_tree(&live.borrow()),"unchanged":false}),
            |_| panic!("must not observe invocation panic")
        )?
        .is_none()
    );
    let after: Json =
        serde_json::from_slice(&fs::read(temp.0.join("panic-COMPLETE-INPUT-AFTER.json"))?)?;
    assert_eq!(after["state"], json!(["Int", 3]));
    assert!(temp.0.join("panic-COMPLETE-INVOCATION-PANIC.txt").is_file());
    for (label, failed_capture) in [("observer", false), ("raw-capture", true)] {
        *live.borrow_mut() = Instance::Scalar(Value::Int(8));
        let observed = codec::retain_outcome_with_input_after(
            Ok(Ok::<_, io::Error>(Instance::Scalar(Value::Int(4)))),
            |s, b| evidence.write_io(&format!("{label}-{s}"), b),
            |actual| {
                if failed_capture {
                    panic!("raw capture sentinel");
                }
                codec::raw_tree(actual)
            },
            || json!({"state":codec::raw_tree(&live.borrow()),"unchanged":false}),
            |_| panic!("observer sentinel"),
        )?;
        assert!(observed.is_none());
        let after: Json = serde_json::from_slice(&fs::read(
            temp.0.join(format!("{label}-COMPLETE-INPUT-AFTER.json")),
        )?)?;
        assert_eq!(after["state"], json!(["Int", 8]));
        assert!(
            temp.0
                .join(format!("{label}-COMPLETE-RAW-PRODUCT.txt"))
                .is_file()
        );
        assert!(
            temp.0
                .join(format!(
                    "{label}-{}",
                    if failed_capture {
                        "COMPLETE-RAW-CAPTURE-FAILURE.txt"
                    } else {
                        "COMPLETE-OBSERVATION-FAILURE.txt"
                    }
                ))
                .is_file()
        );
    }
    let observed = codec::retain_outcome_with_input_after(
        Ok(Err::<Instance, _>(io::Error::other(
            "original invocation error",
        ))),
        |s, b| evidence.write_io(&format!("invocation-error-{s}"), b),
        codec::raw_tree,
        || json!({"state":codec::raw_tree(&live.borrow()),"unchanged":false}),
        |_| json!({"matched":true}),
    )?;
    assert!(observed.is_some_and(|r| r["matched"] == false));
    let after: Json = serde_json::from_slice(&fs::read(
        temp.0.join("invocation-error-COMPLETE-INPUT-AFTER.json"),
    )?)?;
    assert_eq!(after["state"], json!(["Int", 8]));
    assert!(
        fs::read_to_string(temp.0.join("invocation-error-COMPLETE-RAW-PRODUCT.txt"))?
            .contains("original invocation error")
    );
    let observed = codec::retain_outcome_with_input_after(
        Ok(Ok::<_, io::Error>(Instance::Scalar(Value::Int(4)))),
        |s, b| evidence.write_io(&format!("snapshot-{s}"), b),
        codec::raw_tree,
        || panic!("input-after capture sentinel"),
        |_| json!({"matched":true}),
    )?;
    assert!(observed.is_some_and(|r| r["matched"] == false));
    assert!(temp.0.join("snapshot-COMPLETE-RAW-PRODUCT.json").is_file());
    assert!(
        temp.0
            .join("snapshot-COMPLETE-INPUT-AFTER-FAILURE.txt")
            .is_file()
    );
    let observed = std::cell::Cell::new(false);
    let error = codec::retain_outcome_with_input_after(
        Ok(Ok::<_, io::Error>(Instance::Scalar(Value::Int(5)))),
        |s, b| {
            if s == "COMPLETE-INPUT-AFTER.json" {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "retention sentinel",
                ));
            }
            evidence.write_io(&format!("io-{s}"), b)
        },
        codec::raw_tree,
        || json!({"state":codec::raw_tree(&live.borrow()),"unchanged":true}),
        |_| {
            observed.set(true);
            json!({"matched":true})
        },
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(temp.0.join("io-COMPLETE-RAW-PRODUCT.json").is_file());
    assert!(!observed.get());
    Ok(())
}

#[test]
fn basic_starting443_csharp_also_requires_complete_fresh_per_call_products() -> TestResult<()> {
    let case = json!({"id":"CS-CONTROL","model":0,"source":["Int",1],"inputs":[]});
    let call = json!({"selection":"Primary","expected":{"Ok":[[null,["Int",2]]]}});
    let identity = host_call_identity("csharp", "new-run", 0, &case, &call, "00-Primary");
    let expected = vec![identity.clone()];
    let contract = json!({"engine":"csharp","host_identity":{"program":"/bound/dotnet","assembly":"/fresh/Host.dll"}});
    let frozen = b"fixed C# transport controls";
    let before = json!({"source":{"kind":"Scalar","tag":"Int64","value":1},"inputs":[]});
    let raw = json!({"variant":"Primary","instance":{"kind":"Scalar","tag":"Int64","value":2}});
    let mut files = vec![
        ("RUN-CONTRACT.json".into(), serde_json::to_vec(&contract)?),
        (
            "HOST-IDENTITY.json".into(),
            serde_json::to_vec(&contract["host_identity"])?,
        ),
        ("COMPLETE-FROZEN-CONTROLS.json".into(), frozen.to_vec()),
        (
            "ALL-COMPLETE-COMPARISONS.json".into(),
            serde_json::to_vec(&json!([{"invocation":identity,"matched":true}]))?,
        ),
        (
            "00-Primary-COMPLETE-INVOCATION.json".into(),
            serde_json::to_vec(&identity)?,
        ),
    ];
    for (suffix, record) in [
        (
            "COMPLETE-INPUT.json",
            json!({"source_recipe":["Int",1],"inputs_recipe":[],"before_raw":before,"before":{"source":["Int",1],"inputs":[]}}),
        ),
        (
            "COMPLETE-INPUT-AFTER.json",
            json!({"state":before,"unchanged":true}),
        ),
        ("COMPLETE-RAW-PRODUCT.json", raw.clone()),
        (
            "COMPLETE-OBSERVATION.json",
            json!({"actual":{"Ok":[[null,["Int",2]]]},"raw_product":raw,"matched":true,"input_after_complete":true,"input_after_unchanged":true}),
        ),
    ] {
        files.push((
            format!("00-Primary-{suffix}"),
            serde_json::to_vec(&json!({"invocation":identity,"record":record}))?,
        ));
    }
    for suffix in ["COMPLETE-RAW-PRODUCT.txt", "COMPLETE-EXCEPTION.txt"] {
        files.push((
            format!("00-Primary-{suffix}"),
            format!(
                "INVOCATION={}\ncomplete original\n",
                serde_json::to_string(&identity)?
            )
            .into_bytes(),
        ));
    }
    assert!(verify_complete_host_files(
        &contract, &expected, &files, frozen
    ));
    assert!(!verify_complete_host_files(
        &contract,
        &expected,
        &Vec::new(),
        frozen
    ));
    let mut missing = files.clone();
    missing.retain(|(p, _)| p != "00-Primary-COMPLETE-INPUT-AFTER.json");
    let mut duplicate = files.clone();
    duplicate
        .iter_mut()
        .find(|(p, _)| p == "ALL-COMPLETE-COMPARISONS.json")
        .unwrap()
        .1 = serde_json::to_vec(
        &json!([{"invocation":identity,"matched":true},{"invocation":identity,"matched":true}]),
    )?;
    let mut stale = files.clone();
    let entry = stale
        .iter_mut()
        .find(|(p, _)| p == "00-Primary-COMPLETE-OBSERVATION.json")
        .unwrap();
    let mut value: Json = serde_json::from_slice(&entry.1)?;
    value["invocation"]["run_id"] = json!("old-run");
    entry.1 = serde_json::to_vec(&value)?;
    let mut extra = files.clone();
    extra.push(("stale-extra.json".into(), b"{}".to_vec()));
    for invalid in [missing, duplicate, stale, extra] {
        assert!(!verify_complete_host_files(
            &contract, &expected, &invalid, frozen
        ));
    }
    Ok(())
}

#[test]
fn basic_starting443_old_retention_api_keeps_original_format_without_opt_in() -> TestResult<()> {
    let temp = TempDir::new("starting443-legacy-retention")?;
    let evidence = Evidence(temp.0.clone());
    for opt_in in [false, true] {
        let invocation = Ok(Ok::<_, io::Error>(Instance::Scalar(Value::Int(7))));
        let writer =
            |suffix: &str, bytes: &[u8]| evidence.write_io(&format!("{opt_in}-{suffix}"), bytes);
        if opt_in {
            codec::retain_outcome_with_input_after(
                invocation,
                writer,
                codec::raw_tree,
                || json!({"state":["Int",7],"unchanged":true}),
                |_| json!({"matched":true}),
            )?;
        } else {
            codec::retain_outcome(
                invocation,
                writer,
                codec::raw_tree,
                |_| json!({"matched":true}),
            )?;
        }
    }
    for suffix in ["COMPLETE-RAW-PRODUCT.txt", "COMPLETE-RAW-PRODUCT.json"] {
        assert_eq!(
            fs::read(temp.0.join(format!("false-{suffix}")))?,
            fs::read(temp.0.join(format!("true-{suffix}")))?
        );
    }
    assert!(!temp.0.join("false-COMPLETE-INPUT-AFTER.json").exists());
    let old: Json =
        serde_json::from_slice(&fs::read(temp.0.join("false-COMPLETE-OBSERVATION.json"))?)?;
    assert_eq!(old, json!({"matched":true}));
    Ok(())
}

#[test]
fn basic_starting443_malformed_ok_program_constructor_panic_does_not_skip_independent_lanes()
-> TestResult<()> {
    let temp = TempDir::new("starting443-constructor-panic")?;
    let evidence = Evidence(temp.0.clone());
    let mut malformed = codegen::lower_with_basic_owned_group_starting_with(&projects::base())?;
    let specialized =
        codegen::lower_with_basic_owned_group_starting_with(&projects::refusal_base())?;
    malformed.root.children.clear();
    let (products, matched) = recorded(
        &evidence,
        "direct-constructor",
        || Ok::<_, io::Error>(closed_programs(&malformed, &specialized)),
        debug_capture,
        Result::is_ok,
    )?;
    assert!(products.is_none() && !matched);
    assert!(
        temp.0
            .join("direct-constructor-COMPLETE-INVOCATION-PANIC.txt")
            .is_file()
    );
    let mut reached = Vec::new();
    for lane in ["models", "negative-projects"] {
        let (_, ok) = recorded(
            &evidence,
            lane,
            || {
                reached.push(lane);
                Ok::<_, io::Error>(())
            },
            debug_capture,
            Result::is_ok,
        )?;
        assert!(ok);
    }
    assert_eq!(reached, ["models", "negative-projects"]);
    Ok(())
}
