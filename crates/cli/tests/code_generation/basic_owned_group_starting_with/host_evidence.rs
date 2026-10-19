// Expected invocations come only from the independently frozen controls. A host
// exit code, self-reported count, subset or stale packet can never establish them.
fn expected_host_calls(
    controls: &Json,
    legacy: &Json,
    engine: &str,
    model: Option<usize>,
    ordinary: bool,
    run_id: &str,
) -> Vec<Json> {
    let mut expected = Vec::new();
    for (ordinal, case) in controls["cases"].as_array().unwrap().iter().enumerate() {
        if model.is_some_and(|model| case["model"] != model) {
            continue;
        }
        for call in case["calls"].as_array().unwrap() {
            let slot = format!("{ordinal:02}-{}", call["selection"].as_str().unwrap());
            expected.push(host_call_identity(
                engine, run_id, ordinal, case, call, &slot,
            ));
        }
    }
    if ordinary {
        let case = &legacy["cases"][0];
        for call in case["calls"].as_array().unwrap() {
            let slot = format!("LEGACY-{}", call["selection"].as_str().unwrap());
            expected.push(host_call_identity(engine, run_id, CASES, case, call, &slot));
        }
    }
    expected
}
fn host_call_identity(
    engine: &str,
    run_id: &str,
    ordinal: usize,
    case: &Json,
    call: &Json,
    slot: &str,
) -> Json {
    let ordinary = case["id"] == "L01-ordinary-starting";
    let all = call["selection"] == "All";
    let api = match (engine, ordinary, all) {
        ("rust", false, true) => "execute_with_basic_owned_sources",
        ("rust", false, false) => "execute_selected_with_basic_owned_sources",
        ("rust", true, true) => "execute_json_outputs",
        ("rust", true, false) => "execute_json_selected_target",
        ("csharp", false, true) => "ExecuteWithBasicOwnedSources",
        ("csharp", false, false) => "ExecuteSelectedWithBasicOwnedSources",
        ("csharp", true, true) => "ExecuteJsonOutputs",
        ("csharp", true, false) => "ExecuteJsonSelectedTarget",
        _ => "INVALID",
    };
    json!({"version":2,"run_id":run_id,"engine":engine,"slot":slot,"ordinal":ordinal,
        "case":case["id"],"model":case["model"],"selection":call["selection"],
        "api":api,"call":call,"source":case["source"],"inputs":case["inputs"]})
}
fn unique_json_calls(calls: &[Json]) -> bool {
    let mut slots = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    calls.iter().all(|call| {
        call["slot"]
            .as_str()
            .is_some_and(|slot| slots.insert(slot.to_owned()))
            && ids.insert((call["case"].to_string(), call["selection"].to_string()))
    })
}
fn host_proof_suffixes(engine: &str) -> Vec<&'static str> {
    let mut names = vec![
        "COMPLETE-INVOCATION.json",
        "COMPLETE-INPUT.json",
        "COMPLETE-RAW-PRODUCT.txt",
        "COMPLETE-RAW-PRODUCT.json",
        "COMPLETE-INPUT-AFTER.json",
        "COMPLETE-OBSERVATION.json",
    ];
    names.push(if engine == "rust" {
        "COMPLETE-INPUT.txt"
    } else {
        "COMPLETE-EXCEPTION.txt"
    });
    names
}
fn parse_evidence_json(files: &BTreeMap<String, Vec<u8>>, path: &str) -> Option<Json> {
    serde_json::from_slice(files.get(path)?).ok()
}
fn stamped_record(
    files: &BTreeMap<String, Vec<u8>>,
    identity: &Json,
    suffix: &str,
) -> Option<Json> {
    let path = format!("{}-{suffix}", identity["slot"].as_str()?);
    let envelope = parse_evidence_json(files, &path)?;
    if envelope.as_object()?.len() != 2 || envelope["invocation"] != *identity {
        return None;
    }
    envelope.get("record").cloned()
}
fn stamped_text(files: &BTreeMap<String, Vec<u8>>, identity: &Json, suffix: &str) -> bool {
    let Some(slot) = identity["slot"].as_str() else {
        return false;
    };
    let Some(bytes) = files.get(&format!("{slot}-{suffix}")) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Some((header, body)) = text.split_once('\n') else {
        return false;
    };
    header
        .strip_prefix("INVOCATION=")
        .and_then(|v| serde_json::from_str::<Json>(v).ok())
        .as_ref()
        == Some(identity)
        && !body.is_empty()
}
fn expected_rust_raw(call: &Json) -> Option<Json> {
    let outputs = call["expected"]["Ok"].as_array()?;
    let first = outputs.first()?;
    let primary = codec::raw_tree(&codec::decode(&first[1]));
    Some(if call["selection"] == "All" {
        json!({"variant":"All","primary":primary,
        "extras":outputs.iter().skip(1).map(|output|json!({"name":output[0],"instance":codec::raw_tree(&codec::decode(&output[1]))})).collect::<Vec<_>>()})
    } else if call["selection"] == "Primary" {
        json!({"variant":"Primary","instance":primary})
    } else {
        json!({"variant":"Named","name":first[0],"instance":primary})
    })
}
fn verify_complete_host_files(
    contract: &Json,
    expected: &[Json],
    actual: &ArtifactFiles,
    frozen_controls: &[u8],
) -> bool {
    let engine = contract["engine"].as_str().unwrap_or("");
    if !["rust", "csharp"].contains(&engine) || expected.is_empty() || !unique_json_calls(expected)
    {
        return false;
    }
    let mut files = BTreeMap::new();
    for (path, bytes) in actual {
        if campaign_insert_path(&mut files, path.clone(), bytes.clone()).is_err() {
            return false;
        }
    }
    let Ok(mut paths) = campaign_path_set(
        [
            "RUN-CONTRACT.json",
            "HOST-IDENTITY.json",
            "COMPLETE-FROZEN-CONTROLS.json",
            "ALL-COMPLETE-COMPARISONS.json",
        ]
        .into_iter()
        .map(str::to_owned),
    ) else {
        return false;
    };
    for identity in expected {
        for suffix in host_proof_suffixes(engine) {
            if campaign_insert_path_set(
                &mut paths,
                format!("{}-{suffix}", identity["slot"].as_str().unwrap()),
            )
            .is_err()
            {
                return false;
            }
        }
    }
    if paths != files.keys().cloned().collect() {
        return false;
    }
    if parse_evidence_json(&files, "RUN-CONTRACT.json").as_ref() != Some(contract)
        || parse_evidence_json(&files, "HOST-IDENTITY.json").as_ref()
            != Some(&contract["host_identity"])
        || files
            .get("COMPLETE-FROZEN-CONTROLS.json")
            .map(Vec::as_slice)
            != Some(frozen_controls)
    {
        return false;
    }
    let summary = json!(
        expected
            .iter()
            .map(|identity| json!({"invocation":identity,"matched":true}))
            .collect::<Vec<_>>()
    );
    if parse_evidence_json(&files, "ALL-COMPLETE-COMPARISONS.json") != Some(summary) {
        return false;
    }
    for identity in expected {
        let slot = identity["slot"].as_str().unwrap();
        if parse_evidence_json(&files, &format!("{slot}-COMPLETE-INVOCATION.json")).as_ref()
            != Some(identity)
        {
            return false;
        }
        let Some(input) = stamped_record(&files, identity, "COMPLETE-INPUT.json") else {
            return false;
        };
        let Some(after) = stamped_record(&files, identity, "COMPLETE-INPUT-AFTER.json") else {
            return false;
        };
        let Some(raw) = stamped_record(&files, identity, "COMPLETE-RAW-PRODUCT.json") else {
            return false;
        };
        let Some(observation) = stamped_record(&files, identity, "COMPLETE-OBSERVATION.json")
        else {
            return false;
        };
        if !stamped_text(&files, identity, "COMPLETE-RAW-PRODUCT.txt")
            || !stamped_text(
                &files,
                identity,
                if engine == "rust" {
                    "COMPLETE-INPUT.txt"
                } else {
                    "COMPLETE-EXCEPTION.txt"
                },
            )
        {
            return false;
        }
        if input["source_recipe"] != identity["source"]
            || input["inputs_recipe"] != identity["inputs"]
            || input.get("before_raw").is_none()
            || after["state"] != input["before_raw"]
            || after["unchanged"] != true
            || observation["matched"] != true
            || observation["input_after_complete"] != true
            || observation["input_after_unchanged"] != true
        {
            return false;
        }
        let ordinary = identity["case"] == "L01-ordinary-starting";
        if ordinary {
            if input["before_raw"] != json!({"source_text":contract["ordinary_input"],"inputs":[]})
            {
                return false;
            }
            let result = if engine == "rust" { &raw["Ok"] } else { &raw };
            let expected_raw = if identity["selection"] == "All" {
                json!({"variant":"All","primary":contract["ordinary_expected"],"extras":[]})
            } else {
                json!({"variant":"Primary","document":contract["ordinary_expected"]})
            };
            if result != &expected_raw {
                return false;
            }
        } else if engine == "rust" {
            if input["before_raw"]
                != json!({"source":identity["source"],"inputs":identity["inputs"]})
            {
                return false;
            }
            if identity["call"]["expected"].get("Ok").is_some() {
                if observation["actual_trees"] != identity["call"]["expected"]["Ok"]
                    || expected_rust_raw(&identity["call"]).as_ref() != raw.get("Ok")
                {
                    return false;
                }
            } else if raw.get("Err").is_none() || raw["Err"] != observation["actual_error"] {
                return false;
            }
        } else {
            if input["before"] != json!({"source":identity["source"],"inputs":identity["inputs"]})
                || raw != observation["raw_product"]
            {
                return false;
            }
            if identity["call"]["expected"].get("Ok").is_some() {
                if observation["actual"]["Ok"] != identity["call"]["expected"]["Ok"] {
                    return false;
                }
            } else if raw["variant"] != "Exception" || raw["error"] != observation["actual"]["Err"]
            {
                return false;
            }
        }
    }
    true
}
fn verify_host_directory(
    evidence: &Evidence,
    label: &str,
    contract: &Json,
    expected: &[Json],
    directory: &Path,
    controls: &[u8],
) -> TestResult<bool> {
    // A host proof is deliberately flat. Do not follow a forged symlink or read
    // arbitrary nested data while diagnosing a stale/extra evidence entry.
    let mut actual = Vec::new();
    let root_valid = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata.is_dir() && !metadata.file_type().is_symlink(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => return Err(error.into()),
    };
    if !root_valid {
        evidence.json(&format!("{label}-COMPLETE-HOST-EVIDENCE-COMPARISON.json"),&json!({"matched":false,"reason":"redirected/non-directory output root","expected_invocations":expected}))?;
        return Ok(false);
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => Some(entries),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(entries) = entries {
        for entry in entries {
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = campaign_path_key(Path::new(&entry.file_name()))?;
            if !kind.is_file() || kind.is_symlink() {
                actual.push((
                    format!("UNEXPECTED-NONREGULAR-ENTRY-{name}"),
                    format!("{kind:?}").into_bytes(),
                ));
            } else {
                actual.push((name, fs::read(entry.path())?));
            }
        }
    }
    actual.sort_by(|a, b| a.0.cmp(&b.0));
    evidence.json(
        &format!("{label}-COMPLETE-HOST-EVIDENCE-INDEX.json"),
        &hashes(actual.clone()),
    )?;
    let matched = verify_complete_host_files(contract, expected, &actual, controls);
    evidence.json(
        &format!("{label}-COMPLETE-HOST-EVIDENCE-COMPARISON.json"),
        &json!({"matched":matched,"expected_invocations":expected,"exit_code_is_not_proof":true}),
    )?;
    Ok(matched)
}
