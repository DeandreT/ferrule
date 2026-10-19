const HISTORICAL_MANIFEST_SHA256: &str =
    "8fceace43e25a4c66282e369be054f26e029b62567b75505bfe0415b354e03a0";
const HISTORICAL_PACKAGE_SHA256: &str =
    "303095b71ebd4ccc3aaf64120af9ebea5432dcde1dc968cf15641deec6e47731";
const HISTORICAL_MANIFEST: &[u8] =
    include_bytes!("historical-package/legacy-baseline/manifest.json");
const HISTORICAL_CARGO: &[u8] =
    include_bytes!("historical-package/legacy-baseline/Cargo.toml.historical");
const HISTORICAL_CARGO_TEMPLATE: &str =
    include_str!("historical-package/legacy-baseline/Cargo.toml.template");
const HISTORICAL_LIB: &[u8] =
    include_bytes!("historical-package/legacy-baseline/lib.rs.historical");
fn fixture_root() -> TestResult<PathBuf> {
    Ok(root()?.join("crates/cli/tests/code_generation/basic_owned_group_starting_with"))
}
fn historical_cargo_manifest(runtime: &str) -> TestResult<Vec<u8>> {
    if HISTORICAL_CARGO_TEMPLATE
        .matches("@RUNTIME_PATH_JSON@")
        .count()
        != 1
        || !runtime.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err("one exact printable runtime-path JSON operand required".into());
    }
    Ok(HISTORICAL_CARGO_TEMPLATE
        .replace("@RUNTIME_PATH_JSON@", &serde_json::to_string(runtime)?)
        .into_bytes())
}
fn historical_inventory() -> TestResult<(
    Json,
    Vec<legacy_artifact_policy::ManifestRow>,
    Vec<legacy_artifact_policy::ManifestRow>,
)> {
    if digest(HISTORICAL_MANIFEST) != HISTORICAL_MANIFEST_SHA256
        || digest(include_bytes!("historical-package/PACKET.sha256")) != HISTORICAL_PACKAGE_SHA256
        || fs::read(fixture_root()?.join("historical-package/PACKET.sha256"))?
            != include_bytes!("historical-package/PACKET.sha256")
    {
        return Err("accepted exact-base historical manifest/package identity changed".into());
    }
    verify_archival_manifest(
        &fixture_root()?.join("historical-package"),
        include_str!("historical-package/PACKET.sha256"),
        "./",
        &["PACKET.sha256"],
    )?;
    let capture_manifest = include_str!("historical-capture.ORIGINAL.sha256");
    if digest(capture_manifest.as_bytes())
        != "1c228019105d4bc6d380e05b56d84f379a65a3ca19f314f1b71e8b0ea09eecbf"
        || fs::read(fixture_root()?.join("historical-capture.ORIGINAL.sha256"))?
            != capture_manifest.as_bytes()
    {
        return Err("original historical capture manifest changed".into());
    }
    verify_archival_manifest(
        &fixture_root()?.join("historical-capture"),
        capture_manifest,
        "/home/deandre/.cache/ferrule-qualification/starting443-historical-artifacts-v1/",
        &[],
    )?;
    let manifest: Json = serde_json::from_slice(HISTORICAL_MANIFEST)?;
    let p = &manifest["provenance"];
    if manifest["version"] != 1
        || p["source_commit"] != "f30f6ca1c56a533bd4ccb4e4f4bfb821d939e971"
        || p["source_tree"] != "2748a46873ef858c9c498d49f8d865e005b3366a"
        || p["source_archive_sha256"]
            != "f078fffe8ee307450b44241e88c914b62c6410a5cf70246936b5a4c37671f2d6"
        || p["generator_sha256"]
            != "bc68d9621b93455355a1b131c64178c3b6b2128b4aaa868433cef719401922bc"
        || p["lowering_api"] != "codegen::lower"
        || p["rust_api"] != "codegen_rust::emit"
        || p["csharp_api"] != "codegen_csharp::emit"
        || p["adapter_opt_ins"] != json!([])
        || p["rust_package_name"] != "ferrule-generated-mapping"
        || p["rust_runtime_dependency"] != "Path"
    {
        return Err(
            "Starting historical provenance/options changed; no Adjacent bridge is applicable"
                .into(),
        );
    }
    for (field, bytes) in [
        ("project_sha256",include_bytes!("../../../../../conformance/basic-owned-group-starting-with/legacy-project.json").as_slice()),
        ("input_sha256",include_bytes!("../../../../../conformance/basic-owned-group-starting-with/legacy-input.json").as_slice()),
        ("expected_output_sha256",include_bytes!("../../../../../conformance/basic-owned-group-starting-with/legacy-expected.json").as_slice()),
        ("input_tree_sha256",include_bytes!("../../../../../conformance/basic-owned-group-starting-with/legacy-input.tree.json").as_slice()),
        ("expected_tree_sha256",include_bytes!("../../../../../conformance/basic-owned-group-starting-with/legacy-expected.tree.json").as_slice()),
    ] { if p[field] != digest(bytes) { return Err(format!("immutable Starting baseline differs: {field}").into()); } }
    let rust: Vec<legacy_artifact_policy::ManifestRow> =
        serde_json::from_value(manifest["rust"].clone())?;
    let cs: Vec<legacy_artifact_policy::ManifestRow> =
        serde_json::from_value(manifest["csharp"].clone())?;
    if rust.len() != 2
        || cs.len() != 78
        || rust[0] != legacy_artifact_policy::row("Cargo.toml", HISTORICAL_CARGO)
        || rust[1] != legacy_artifact_policy::row("src/lib.rs", HISTORICAL_LIB)
    {
        return Err("historical 2 Rust/78 C# exact inventories differ".into());
    }
    for rows in [&rust, &cs] {
        if rows.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err("historical paths not sorted unique".into());
        }
        for (path, len, sha) in rows {
            codegen::ArtifactPath::new(path.as_str())?;
            if *len == 0 || sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("invalid historical row".into());
            }
        }
    }
    // Revalidate all eighty actual bodies, not just the three prospective deltas.
    if hashes(artifact_files(
        &fixture_root()?.join("historical-capture/rust"),
    )?) != rust
        || hashes(artifact_files(
            &fixture_root()?.join("historical-capture/csharp"),
        )?) != cs
    {
        return Err("immutable historical artifact bodies differ from accepted rows".into());
    }
    let old_operand = p["historical_runtime_path"]
        .as_str()
        .ok_or("historical operand")?;
    if historical_cargo_manifest(old_operand)?.as_slice() != HISTORICAL_CARGO {
        return Err("complete Cargo template reconstruction failed".into());
    }
    Ok((manifest, rust, cs))
}
fn verify_archival_manifest(
    root: &Path,
    manifest: &str,
    prefix: &str,
    excluded: &[&str],
) -> TestResult<()> {
    let mut expected = BTreeMap::new();
    for line in manifest.lines() {
        let (sha, path) = line.split_once("  ").ok_or("archival manifest row")?;
        let path = path
            .strip_prefix(prefix)
            .ok_or("archival manifest original prefix")?;
        if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("archival manifest duplicate/invalid row".into());
        }
        campaign_insert_path(&mut expected, path.to_owned(), sha.to_owned())?;
    }
    let mut actual = BTreeMap::new();
    for (path, bytes) in artifact_files(root)? {
        if !excluded.contains(&path.as_str()) {
            campaign_insert_path(&mut actual, path, digest(&bytes))?;
        }
    }
    if expected != actual {
        return Err("archival full pathset/body/provenance differs".into());
    }
    Ok(())
}
fn verify_frozen_corpus_and_borrowed_helpers() -> TestResult<()> {
    const ACCEPTED: &str = "7efb2541231587ef1afed0f3511654c7a068964a4705507c20ee8178a2ca31d6";
    let manifest = include_str!("accepted-corpus-v2/PACKET.sha256");
    if digest(manifest.as_bytes()) != ACCEPTED {
        return Err("accepted corpus v2 manifest identity changed".into());
    }
    let accepted = fixture_root()?.join("accepted-corpus-v2");
    if fs::read(accepted.join("PACKET.sha256"))? != manifest.as_bytes()
        || fs::read_to_string(accepted.join("PACKET.sha256.sha256"))?
            != format!("{ACCEPTED}  PACKET.sha256\n")
    {
        return Err("accepted corpus seal files differ from original identity".into());
    }
    verify_archival_manifest(
        &accepted,
        manifest,
        "./",
        &["PACKET.sha256", "PACKET.sha256.sha256"],
    )?;
    let relative = "conformance/basic-owned-group-starting-with";
    if artifact_files(&root()?.join(relative))?
        != artifact_files(&accepted.join("source").join(relative))?
    {
        return Err(
            "immutable 7-file/5-literal corpus mount changed; no oracle refresh is allowed".into(),
        );
    }
    // The two maintained templates keep their exact accepted postimages. The
    // native/codec/facade amendment is separately bound and independently reviewed.
    for name in ["Host.rs.txt", "Host.cs.txt"] {
        let actual = root()?
            .join("crates/cli/tests/code_generation/basic_owned_sequence_item_at")
            .join(name);
        if fs::read(actual)? != fs::read(accepted.join("postimages").join(name))? {
            return Err(
                format!("shared helper is not the exact accepted postimage: {name}").into(),
            );
        }
    }
    for relative in [
        "crates/cli/tests/code_generation/host_policy.rs",
        "crates/cli/tests/code_generation/basic_owned_sequence_item_at/legacy_control.rs.txt",
        "crates/cli/tests/code_generation/basic_owned_sequence_item_at/LegacyHost.cs.txt",
    ] {
        if fs::read(root()?.join(relative))? != fs::read(accepted.join("reuse").join(relative))? {
            return Err(format!(
                "borrowed qualified implementation changed without authority: {relative}"
            )
            .into());
        }
    }
    // Explicit live-N amendment. The accepted M reuse COPY remains immutable.
    verify_live_adjacent_42686c55(&accepted)?;
    for (relative, compiled) in [
        (
            "crates/cli/tests/code_generation/basic_owned_sequence_item_at.rs",
            include_bytes!("../basic_owned_sequence_item_at.rs").as_slice(),
        ),
        (
            "crates/cli/tests/code_generation/basic_owned_sequence_item_at/native.rs",
            include_bytes!("../basic_owned_sequence_item_at/native.rs").as_slice(),
        ),
        (
            "crates/cli/tests/code_generation/basic_owned_sequence_item_at/codec.rs",
            include_bytes!("../basic_owned_sequence_item_at/codec.rs").as_slice(),
        ),
    ] {
        if fs::read(root()?.join(relative))? != compiled {
            return Err("compiled helper amendment/source binding differs".into());
        }
    }
    // The retained v2 packet includes the failed v1 custody and its explanation;
    // validating this successor never repairs or re-labels that older failure.
    Ok(())
}
fn retain_historical(
    evidence: &Evidence,
    runtime: &Path,
) -> TestResult<(
    Vec<legacy_artifact_policy::ManifestRow>,
    Vec<legacy_artifact_policy::ManifestRow>,
    Vec<u8>,
)> {
    let (_, mut rust, cs) = historical_inventory()?;
    for relative in ["historical-package", "historical-capture"] {
        for (path, bytes) in artifact_files(&fixture_root()?.join(relative))? {
            evidence.write(&format!("legacy-baseline/{relative}/{path}"), &bytes)?;
        }
    }
    evidence.write(
        "legacy-baseline/historical-capture.ORIGINAL.sha256",
        include_bytes!("historical-capture.ORIGINAL.sha256"),
    )?;
    let cargo = historical_cargo_manifest(&runtime.display().to_string())?;
    rust[0] = legacy_artifact_policy::row("Cargo.toml", &cargo);
    evidence.write("legacy-baseline/Cargo.toml.expected", &cargo)?;
    evidence.json(
        "legacy-baseline/COMPLETE-EXPECTED-ARTIFACTS.json",
        &json!({"rust":rust,"csharp":cs}),
    )?;
    Ok((rust, cs, cargo))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // Preserved original version2 schema, never N composition authority.
struct StartingSourceAcceptance {
    version: u32,
    policy: String,
    disposition: String,
    reviewer: String,
    review_reference: String,
    source_bindings: Vec<legacy_artifact_policy::ManifestRow>,
    build_time_source_bindings: Vec<legacy_artifact_policy::ManifestRow>,
    commands: Json,
    historical_manifest_sha256: String,
    historical_package_sha256: String,
    historical_runtime: Vec<legacy_artifact_policy::ManifestRow>,
    candidate_runtime: Vec<legacy_artifact_policy::ManifestRow>,
    helper_amendment_review_reference: String,
    helper_amendment_source_rows: Vec<legacy_artifact_policy::ManifestRow>,
}
const LIVE_ADJACENT_PATH: &str =
    "crates/cli/tests/code_generation/basic_owned_group_adjacent_by.rs";
fn live_adjacent_42686c55_row() -> legacy_artifact_policy::ManifestRow {
    (
        LIVE_ADJACENT_PATH.into(),
        66831,
        "85f93fae8644c8445c333eb8e6cbafdaad2e13e9c855b8696a56a24440a7f638".into(),
    )
}
fn historical_adjacent_m_row() -> legacy_artifact_policy::ManifestRow {
    (
        LIVE_ADJACENT_PATH.into(),
        63672,
        "1041e9a383f3cdcce9e7245c81db4aa0583c156297c873fd12ee4005d6640f03".into(),
    )
}
fn verify_live_adjacent_42686c55(accepted: &Path) -> TestResult<()> {
    if legacy_artifact_policy::row(
        LIVE_ADJACENT_PATH,
        &fs::read(accepted.join("reuse").join(LIVE_ADJACENT_PATH))?,
    ) != historical_adjacent_m_row()
        || legacy_artifact_policy::row(
            LIVE_ADJACENT_PATH,
            &fs::read(root()?.join(LIVE_ADJACENT_PATH))?,
        ) != live_adjacent_42686c55_row()
        || legacy_artifact_policy::row(
            LIVE_ADJACENT_PATH,
            include_bytes!("../basic_owned_group_adjacent_by.rs"),
        ) != live_adjacent_42686c55_row()
    {
        return Err(
            "immutable M reuse or separately bound compiled/live N Adjacent helper differs".into(),
        );
    }
    Ok(())
}
fn verify_merged_registry_42686c55() -> TestResult<()> {
    const REL: &str = "crates/codegen-csharp/src/runtime.rs";
    let expected: legacy_artifact_policy::ManifestRow = (
        REL.into(),
        18045,
        "b8cc5d6409888ab34b764cda976b6cc4a4582ac63016df0f8f788b3ae384530d".into(),
    );
    if legacy_artifact_policy::row(REL, &fs::read(root()?.join(REL))?) != expected
        || legacy_artifact_policy::row(
            REL,
            include_bytes!("../../../../codegen-csharp/src/runtime.rs"),
        ) != expected
    {
        return Err("N generator embedding registry current/compile-time body changed".into());
    }
    Ok(())
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveAdjacent42686c55Amendment {
    version: u32,
    policy: String,
    disposition: String,
    reviewer: String,
    review_reference: String,
    implementation_commit: String,
    implementation_tree: String,
    live_git_blob: String,
    historical_reuse: legacy_artifact_policy::ManifestRow,
    live_helper: legacy_artifact_policy::ManifestRow,
}
impl LiveAdjacent42686c55Amendment {
    fn valid(&self) -> bool {
        self.version == 1
            && self.policy == "AcceptedLiveAdjacentHelper42686c55"
            && !self.reviewer.trim().is_empty()
            && !self.review_reference.trim().is_empty()
            && self.disposition == "independently-accepted-live-helper-amendment"
            && self.implementation_commit == legacy_artifact_policy::MERGED_JSON_N_COMMIT
            && self.implementation_tree == legacy_artifact_policy::MERGED_JSON_N_TREE
            && self.live_git_blob == "b6f753c1d9cd5138cec0000dd1baf4ccb1125757"
            && self.historical_reuse == historical_adjacent_m_row()
            && self.live_helper == live_adjacent_42686c55_row()
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MergedStarting42686c55SourceAcceptance {
    version: u32,
    policy: String,
    disposition: String,
    reviewer: String,
    review_reference: String,
    source_bindings: Vec<legacy_artifact_policy::ManifestRow>,
    build_time_source_bindings: Vec<legacy_artifact_policy::ManifestRow>,
    commands: Json,
    historical_commit: String,
    historical_tree: String,
    historical_manifest_sha256: String,
    historical_package_sha256: String,
    implementation_commit: String,
    implementation_tree: String,
    fixed_merged_additions: Vec<legacy_artifact_policy::ManifestRow>,
    historical_runtime: Vec<legacy_artifact_policy::ManifestRow>,
    candidate_runtime: Vec<legacy_artifact_policy::ManifestRow>,
    helper_amendment_review_reference: String,
    helper_amendment_source_rows: Vec<legacy_artifact_policy::ManifestRow>,
    live_adjacent_amendment: LiveAdjacent42686c55Amendment,
}
impl MergedStarting42686c55SourceAcceptance {
    fn identifies_scope(&self) -> bool {
        self.version == 3
            && self.policy == "ReviewedStartingRuntimeWithMergedJson42686c55"
            && self.historical_commit == legacy_artifact_policy::HISTORICAL_M_COMMIT
            && self.historical_tree == legacy_artifact_policy::HISTORICAL_M_TREE
            && self.implementation_commit == legacy_artifact_policy::MERGED_JSON_N_COMMIT
            && self.implementation_tree == legacy_artifact_policy::MERGED_JSON_N_TREE
            && self.fixed_merged_additions == legacy_artifact_policy::merged_json42686c55_rows()
            && self.live_adjacent_amendment.valid()
    }
}
fn helper_amendment_rows() -> TestResult<Vec<legacy_artifact_policy::ManifestRow>> {
    let mut rows = Vec::new();
    for relative in [
        "crates/cli/tests/code_generation/basic_owned_sequence_item_at.rs",
        "crates/cli/tests/code_generation/basic_owned_sequence_item_at/codec.rs",
        "crates/cli/tests/code_generation/basic_owned_sequence_item_at/native.rs",
        "crates/cli/tests/code_generation/basic_owned_group_starting_with/Host.rs.txt",
        "crates/cli/tests/code_generation/basic_owned_group_starting_with/Host.cs.txt",
        "crates/cli/tests/code_generation/basic_owned_group_starting_with/HostProof.rs.txt",
        "crates/cli/tests/code_generation/basic_owned_group_starting_with/HostProof.cs.txt",
        "crates/cli/tests/code_generation/basic_owned_group_starting_with/legacy_control.rs.txt",
        "crates/cli/tests/code_generation/basic_owned_group_starting_with/LegacyHost.cs.txt",
    ] {
        rows.push(legacy_artifact_policy::row(
            relative,
            &fs::read(root()?.join(relative))?,
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(rows)
}
struct EmissionGate {
    sources: Vec<legacy_artifact_policy::ManifestRow>,
    commands: Json,
    reviewed: legacy_artifact_policy::ReviewedStartingWithMergedJson42686c55Snapshot,
    p_source_pin: Option<PSourcePin>,
}
impl EmissionGate {
    fn check(&self) -> TestResult<()> {
        if self.sources != source_bindings()?
            || self.commands != command_runtime_binding()?
            || !legacy_artifact_policy::starting_merged_json42686c55_snapshot_unchanged(
                &root()?,
                &self.reviewed,
            )?
        {
            return Err(
                "independently accepted source/build/runtime binding changed before/after emission"
                    .into(),
            );
        }
        verify_merged_registry_42686c55()?;
        verify_live_adjacent_42686c55(&fixture_root()?.join("accepted-corpus-v2"))?;
        if let Some(pin) = &self.p_source_pin {
            if selected_p_source_pin()?.as_ref() != Some(pin) {
                return Err("P target/proposed candidate identity changed after acceptance".into());
            }
        }
        Ok(())
    }
}
fn complete_source_diff(path: &str, old: &[u8], new: &[u8]) -> TestResult<String> {
    let old = std::str::from_utf8(old)?;
    let new = std::str::from_utf8(new)?;
    let mut diff = format!(
        "--- historical/{path}\n+++ candidate-source/{path}\n@@ -1,{} +1,{} @@\n",
        old.lines().count(),
        new.lines().count()
    );
    for (prefix, text) in [('-', old), ('+', new)] {
        for line in text.split_inclusive('\n') {
            diff.push(prefix);
            diff.push_str(line);
            if !line.ends_with('\n') {
                diff.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    Ok(diff)
}
fn capture_runtime_review_inputs(
    evidence: &Evidence,
) -> TestResult<legacy_artifact_policy::SourceSnapshot> {
    let p_source_pin = selected_p_source_pin()?;
    let (_, _, historical) = historical_inventory()?;
    let sources = legacy_artifact_policy::starting_snapshot(&root()?)?;
    for (path, candidate) in &sources {
        let old = fs::read(fixture_root()?.join("historical-capture/csharp").join(path))?;
        let row = legacy_artifact_policy::row(path, &old);
        if !historical.contains(&row) {
            return Err("historical source body/row differs".into());
        }
        evidence.write(&format!("runtime-delta/historical/{path}"), &old)?;
        evidence.write(&format!("runtime-delta/candidate/{path}"), candidate)?;
        evidence.write(
            &format!("runtime-delta/{path}.complete.diff"),
            complete_source_diff(path, &old, candidate)?.as_bytes(),
        )?;
        evidence.json(
            &format!("runtime-delta/{path}.sizes-hashes.json"),
            &json!({"historical":row,
            "candidate_source":legacy_artifact_policy::row(path,candidate)}),
        )?;
    }
    verify_merged_registry_42686c55()?;
    let fixed = legacy_artifact_policy::merged_json42686c55_fixed_bodies()?;
    for ((path, source, blob, _, _), (_, bytes)) in
        legacy_artifact_policy::MERGED_JSON42686C55_FILES
            .iter()
            .zip(&fixed)
    {
        let live = fs::read(root()?.join(source))?;
        if live != *bytes {
            return Err("fixed N additions are not mutable Starting source".into());
        }
        evidence.write(&format!("merged-base/fixed-N/{path}"), bytes)?;
        let mut binding = json!({"historical_M_row":null,"N_git_blob":blob,
            "implementation_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,"implementation_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,
            "fixed_N_row":legacy_artifact_policy::row(path,bytes),"current_row":legacy_artifact_policy::row(path,&live)});
        if let Some(pin) = &p_source_pin {
            binding["fixed_input_origin_commit"] =
                json!(legacy_artifact_policy::MERGED_JSON_N_COMMIT);
            binding["fixed_input_origin_tree"] = json!(legacy_artifact_policy::MERGED_JSON_N_TREE);
            binding["implementation_commit"] =
                json!(legacy_artifact_policy::IMPLEMENTATION_P_COMMIT);
            binding["implementation_tree"] = json!(legacy_artifact_policy::IMPLEMENTATION_P_TREE);
            binding["attested_candidate_head"] = json!(pin.candidate_head);
            binding["attested_candidate_tree"] = json!(pin.candidate_tree);
        }
        evidence.json(&format!("merged-base/{path}.binding.json"), &binding)?;
    }
    evidence.json("PROPOSED-LIVE-ADJACENT-42686C55-BINDINGS.json",&json!({"policy":"AcceptedLiveAdjacentHelper42686c55","version":1,
        "historical_reuse":historical_adjacent_m_row(),"live_helper":live_adjacent_42686c55_row(),"live_git_blob":"b6f753c1d9cd5138cec0000dd1baf4ccb1125757"}))?;
    let old_helper = fs::read(
        fixture_root()?
            .join("accepted-corpus-v2/reuse")
            .join(LIVE_ADJACENT_PATH),
    )?;
    let live_helper = fs::read(root()?.join(LIVE_ADJACENT_PATH))?;
    evidence.write("live-adjacent-amendment/historical-M.rs", &old_helper)?;
    evidence.write("live-adjacent-amendment/fixed-N.rs", &live_helper)?;
    evidence.write(
        "live-adjacent-amendment/M-to-N.complete.diff",
        complete_source_diff(LIVE_ADJACENT_PATH, &old_helper, &live_helper)?.as_bytes(),
    )?;
    Ok(sources)
}
fn accepted_emission_gate(evidence: &Evidence) -> TestResult<EmissionGate> {
    // Neither source capture nor a policy name grants acceptance. This function
    // never writes an approval. The independent reviewer supplies both the exact
    // receipt file and its separately approved digest through the coordinator.
    verify_frozen_corpus_and_borrowed_helpers()?;
    let path = PathBuf::from(
        std::env::var_os("FERRULE_BASIC_STARTING443_ACCEPTED_SOURCE_REVIEW")
            .ok_or("independently accepted pre-emission source review receipt required")?,
    );
    let expected_digest = std::env::var("FERRULE_BASIC_STARTING443_ACCEPTED_SOURCE_REVIEW_SHA256")?;
    if !path.is_absolute()
        || expected_digest.len() != 64
        || !expected_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("absolute independent review receipt and approved SHA256 required".into());
    }
    let bytes = fs::read(&path)?;
    evidence.write("independent-source-review/ACCEPTED-RECEIPT.json", &bytes)?;
    evidence.json(
        "independent-source-review/ORIGINAL-PATH-AND-EXPECTED-DIGEST.json",
        &json!({"path":path,"expected_sha256":expected_digest}),
    )?;
    if digest(&bytes) != expected_digest {
        return Err("independent source review receipt digest differs".into());
    }
    let version: Json = serde_json::from_slice(&bytes)?;
    if p_receipt_route(
        version["version"].as_u64(),
        selected_p_source_pin()?.is_some(),
    )? {
        return accepted_p_emission_gate(evidence, &bytes);
    }
    if serde_json::from_slice::<StartingSourceAcceptance>(&bytes).is_ok() {
        return Err("version2 strict78 receipt cannot authorize the N composition".into());
    }
    let accepted: MergedStarting42686c55SourceAcceptance = serde_json::from_slice(&bytes)?;
    let sources = source_bindings()?;
    let commands = command_runtime_binding()?;
    let (_, _, historical) = historical_inventory()?;
    let historical_runtime: Vec<_> = legacy_artifact_policy::STARTING_RUNTIME_FILES
        .iter()
        .map(|(path, _)| {
            historical
                .iter()
                .find(|row| row.0 == *path)
                .cloned()
                .ok_or("historical runtime row")
        })
        .collect::<Result<_, _>>()?;
    if !accepted.identifies_scope()
        || accepted.disposition != "independently-accepted-for-candidate-emission"
        || accepted.reviewer.trim().is_empty()
        || accepted.review_reference.trim().is_empty()
        || accepted.historical_manifest_sha256 != HISTORICAL_MANIFEST_SHA256
        || accepted.historical_package_sha256 != HISTORICAL_PACKAGE_SHA256
        || accepted.historical_runtime != historical_runtime
        || accepted.source_bindings != sources
        || accepted.build_time_source_bindings != sources
        || accepted.commands != commands
    {
        return Err(
            "receipt is not independent acceptance of this exact source/build/runtime delta".into(),
        );
    }
    if accepted.helper_amendment_review_reference.trim().is_empty()
        || accepted.helper_amendment_source_rows != helper_amendment_rows()?
    {
        return Err("separate independent acceptance of the exact opt-in helper/host amendments required before emission".into());
    }
    let snapshot = capture_runtime_review_inputs(evidence)?;
    let reviewed = legacy_artifact_policy::bind_starting_with_merged_json42686c55(
        snapshot,
        &accepted.candidate_runtime,
        &accepted.fixed_merged_additions,
        &accepted.implementation_commit,
        &accepted.implementation_tree,
    )?;
    let gate = EmissionGate {
        sources,
        commands,
        reviewed,
        p_source_pin: None,
    };
    gate.check()?;
    Ok(gate)
}

#[test]
#[ignore = "source capture only; independent review still required; coordinator supplies fresh absent root"]
fn basic_starting443_capture_pre_emission_source_review_inputs() -> TestResult<()> {
    let evidence = Evidence::new()?;
    verify_frozen_corpus_and_borrowed_helpers()?;
    let sources = source_bindings()?;
    let commands = command_runtime_binding()?;
    let p_source_pin = selected_p_source_pin()?;
    let snapshot = capture_runtime_review_inputs(&evidence)?;
    evidence.json("COMPLETE-CANDIDATE-SOURCE-BINDINGS.json", &sources)?;
    evidence.json("COMPLETE-COMMAND-AND-NATIVE-BINARY-BINDING.json", &commands)?;
    evidence.json(
        "COMPLETE-HELPER-AMENDMENT-SOURCE-ROWS.json",
        &helper_amendment_rows()?,
    )?;
    evidence.json(
        "FIXED-MERGED-JSON-42686C55-ROWS.json",
        &legacy_artifact_policy::merged_json42686c55_rows(),
    )?;
    if let Some(pin) = &p_source_pin {
        evidence.json(
            "HISTORICAL-M-ORIGIN-N-AND-IMPLEMENTATION-P.json",
            &p_source_identity_record(pin),
        )?;
        evidence.json(
            "PROPOSED-N-TO-P-BASE-SOURCE-AMENDMENT.json",
            &p_base_source_binding_record(),
        )?;
        evidence.json(
            "P-WORKSPACE-LOCK-AND-PATCH-SOURCE-ROWS.json",
            &p_dependency_source_rows(&sources),
        )?;
        evidence.json("PROPOSED-N-HELPER-TO-P-BRIDGE.json",&json!({"origin_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,
            "origin_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,"implementation_commit":legacy_artifact_policy::IMPLEMENTATION_P_COMMIT,
            "implementation_tree":legacy_artifact_policy::IMPLEMENTATION_P_TREE,"historical_reuse":historical_adjacent_m_row(),
            "current_helper":live_adjacent_42686c55_row(),"compiled_helper":live_adjacent_42686c55_row(),"independent_P_bridge_review_required":true}))?;
    } else {
        evidence.json("HISTORICAL-M-AND-IMPLEMENTATION-N.json",&json!({"historical_commit":legacy_artifact_policy::HISTORICAL_M_COMMIT,
        "historical_tree":legacy_artifact_policy::HISTORICAL_M_TREE,"implementation_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,"implementation_tree":legacy_artifact_policy::MERGED_JSON_N_TREE}))?;
    }
    evidence.json(
        "CANDIDATE-RUNTIME-SOURCE-ROWS.json",
        &snapshot
            .iter()
            .map(|(path, bytes)| legacy_artifact_policy::row(path, bytes))
            .collect::<Vec<_>>(),
    )?;
    evidence.write("NOT-AN-ACCEPTANCE.txt",b"Independent exact-source and build-time source review is required before any candidate emission. No acceptance receipt is generated here.\n")?;
    if sources != source_bindings()?
        || commands != command_runtime_binding()?
        || snapshot != legacy_artifact_policy::starting_snapshot(&root()?)?
        || p_source_pin != selected_p_source_pin()?
    {
        return Err("source changed during source-only review capture".into());
    }
    Ok(())
}

fn prepare_legacy(
    evidence: &Evidence,
    runtime: &Path,
    gate: &EmissionGate,
    baseline: &(
        Vec<legacy_artifact_policy::ManifestRow>,
        Vec<legacy_artifact_policy::ManifestRow>,
        Vec<u8>,
    ),
) -> TestResult<bool> {
    let original = include_str!(
        "../../../../../conformance/basic-owned-group-starting-with/legacy-project.json"
    );
    evidence.write("legacy-original-project.json", original.as_bytes())?;
    let project = mapping::project_file::decode_str(original)?;
    let wire = mapping::project_file::encode_pretty(&project)?;
    evidence.write("legacy-roundtrip-project.json", wire.as_bytes())?;
    let roundtrip = mapping::project_file::decode_str(&wire)?;
    let wire_same = wire == original
        && serde_json::to_value(&project)? == serde_json::to_value(&roundtrip)?
        && !wire.contains("owned_source");
    let before = mapping::project_file::encode_pretty(&project)?;
    let (program, lower_ok) = recorded(
        evidence,
        "legacy-lowering",
        || codegen::lower(&project),
        debug_capture,
        Result::is_ok,
    )?;
    evidence.write(
        "legacy-lowering-COMPLETE-INPUT-AFTER.json",
        mapping::project_file::encode_pretty(&project)?.as_bytes(),
    )?;
    let Some(program) = program else {
        return Ok(false);
    };
    let options = codegen_rust::Options {
        package_name: "ferrule-generated-mapping".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    evidence.debug("legacy-COMPLETE-RUST-OPTIONS.txt", &options)?;
    let original_program = program.clone();
    gate.check()?;
    let (rust, rust_ok) = recorded(
        evidence,
        "rust-legacy-result",
        || codegen_rust::emit(&program, &options),
        artifact_capture,
        Result::is_ok,
    )?;
    let rust_input = input_after(evidence, "rust-legacy", &program, &original_program)?;
    gate.check()?;
    let (cs, cs_ok) = recorded(
        evidence,
        "csharp-legacy-result",
        || codegen_csharp::emit(&program),
        artifact_capture,
        Result::is_ok,
    )?;
    let cs_input = input_after(evidence, "csharp-legacy", &program, &original_program)?;
    gate.check()?;
    let (new_rust, new_rust_ok) = recorded(
        evidence,
        "rust-legacy-starting-result",
        || codegen_rust::emit_with_basic_owned_group_starting_with(&program, &options),
        artifact_capture,
        Result::is_ok,
    )?;
    let new_rust_input = input_after(
        evidence,
        "rust-legacy-starting",
        &program,
        &original_program,
    )?;
    gate.check()?;
    let (new_cs, new_cs_ok) = recorded(
        evidence,
        "csharp-legacy-starting-result",
        || codegen_csharp::emit_with_basic_owned_group_starting_with(&program),
        artifact_capture,
        Result::is_ok,
    )?;
    let new_cs_input = input_after(
        evidence,
        "csharp-legacy-starting",
        &program,
        &original_program,
    )?;
    gate.check()?;
    for (name, artifacts) in [
        ("rust-legacy", &rust),
        ("csharp-legacy", &cs),
        ("rust-legacy-starting-profile", &new_rust),
        ("csharp-legacy-starting-profile", &new_cs),
    ] {
        if let Some(artifacts) = artifacts {
            evidence.artifacts(name, artifacts)?;
        }
    }
    let rust_rows = rust.as_ref().map(artifact_manifest);
    let cs_rows = cs.as_ref().map(artifact_manifest);
    let comparison = cs_rows.as_ref().map(|rows| {
        legacy_artifact_policy::compare_starting_merged_json42686c55(
            &baseline.1,
            rows,
            &gate.reviewed,
        )
    });
    let rust_exact =
        rust_rows.as_ref() == Some(&baseline.0)
            && rust.as_ref().is_some_and(|artifacts| {
                artifacts.files().iter().any(|file| {
                    file.path.as_str() == "src/lib.rs" && file.contents == HISTORICAL_LIB
                }) && artifacts
                    .files()
                    .iter()
                    .any(|file| file.path.as_str() == "Cargo.toml" && file.contents == baseline.2)
            });
    let cs_accepted = comparison.as_ref().is_some_and(|c| {
        c.accepts(legacy_artifact_policy::Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
    });
    evidence.json("legacy-COMPLETE-ARTIFACT-COMPARISONS.json",&json!({"policy":"ReviewedStartingRuntimeWithMergedJson42686c55",
        "wire_roundtrip":wire_same,"rust_exact":rust_exact,"rust_actual":rust_rows,"csharp_actual":cs_rows,
        "csharp_accepted":cs_accepted,"candidate_sources_unchanged":true,
        "paths_exact":comparison.as_ref().is_some_and(|c| c.paths_exact),
        "stable_files_exact":comparison.as_ref().is_some_and(|c| c.stable_files_exact),
        "runtime_sources_exact":comparison.as_ref().is_some_and(|c| c.runtime_sources_exact),
        "original_78_conserved":comparison.as_ref().and_then(|c|c.original_78_conserved),
        "fixed_merged_additions_exact":comparison.as_ref().is_some_and(|c|c.fixed_merged_additions_exact),
        "ordinary_mapping_bytes_exact":comparison.as_ref().and_then(|c|c.ordinary_mapping_bytes_exact),
        "historical_exact":comparison.as_ref().is_some_and(|c| c.accepts(legacy_artifact_policy::Policy::ExactHistorical))}))?;
    for (name, bytes) in [
        (
            "legacy-input.json",
            include_bytes!(
                "../../../../../conformance/basic-owned-group-starting-with/legacy-input.json"
            )
            .as_slice(),
        ),
        (
            "legacy-expected.json",
            include_bytes!(
                "../../../../../conformance/basic-owned-group-starting-with/legacy-expected.json"
            )
            .as_slice(),
        ),
    ] {
        evidence.write(name, bytes)?;
        evidence.write(&format!("rust-host/src/{name}"), bytes)?;
    }
    evidence.write(
        "rust-host/src/legacy_control.rs",
        include_bytes!("legacy_control.rs.txt"),
    )?;
    evidence.write(
        "rust-host/src/legacy-input.tree.json",
        include_bytes!(
            "../../../../../conformance/basic-owned-group-starting-with/legacy-input.tree.json"
        ),
    )?;
    evidence.write(
        "rust-host/src/legacy-expected.tree.json",
        include_bytes!(
            "../../../../../conformance/basic-owned-group-starting-with/legacy-expected.tree.json"
        ),
    )?;
    evidence.write(
        "csharp-legacy-host/Host.csproj",
        host_project("legacy").as_bytes(),
    )?;
    evidence.write(
        "csharp-legacy-host/Program.cs",
        format!(
            "{}\n{}",
            include_str!("LegacyHost.cs.txt"),
            include_str!("HostProof.cs.txt")
        )
        .as_bytes(),
    )?;
    let new_cs_bound = new_cs
        .as_ref()
        .is_some_and(|a| emitted_runtime_bound(a, gate, &baseline.1));
    // Historical ordinary versus candidate ordinary is the matching-profile
    // byte assertion above. Basic/Starting-profile output is not its oracle.
    Ok(wire_same
        && lower_ok
        && before == mapping::project_file::encode_pretty(&project)?
        && rust_ok
        && cs_ok
        && new_rust_ok
        && new_cs_ok
        && new_cs_bound
        && rust_exact
        && cs_accepted
        && rust_input
        && cs_input
        && new_rust_input
        && new_cs_input)
}

#[test]
fn basic_starting443_historical_package_template_and_original_fences() -> TestResult<()> {
    let (manifest, _, _) = historical_inventory()?;
    assert_eq!(
        historical_cargo_manifest(
            manifest["provenance"]["historical_runtime_path"]
                .as_str()
                .unwrap()
        )?,
        HISTORICAL_CARGO
    );
    let mut changed = HISTORICAL_MANIFEST.to_vec();
    changed.push(b'\n');
    assert_ne!(digest(&changed), HISTORICAL_MANIFEST_SHA256);
    let temp = TempDir::new("starting443-original-fence")?;
    let evidence = Evidence(temp.0.clone());
    evidence.write("original", b"immutable")?;
    let rows = serde_json::to_value(hashes(artifact_files(&temp.0)?))?;
    evidence.write("new-result", b"allowed")?;
    verify_original_evidence(&temp.0, &rows)?;
    assert!(evidence.write("original", b"overwrite").is_err());
    fs::write(temp.0.join("original"), b"mutation")?;
    assert!(verify_original_evidence(&temp.0, &rows).is_err());
    Ok(())
}

#[test]
fn basic_starting443_merged42686c55_receipt_headers_and_live_helper_cannot_cross_select()
-> TestResult<()> {
    // Header-only in-memory transport control. Empty closure/runtime rows would
    // fail the real emission gate; no acceptance document is written or supplied.
    let fixture = || MergedStarting42686c55SourceAcceptance {
        version: 3,
        policy: "ReviewedStartingRuntimeWithMergedJson42686c55".into(),
        disposition: String::new(),
        reviewer: String::new(),
        review_reference: String::new(),
        source_bindings: vec![],
        build_time_source_bindings: vec![],
        commands: json!(null),
        historical_commit: legacy_artifact_policy::HISTORICAL_M_COMMIT.into(),
        historical_tree: legacy_artifact_policy::HISTORICAL_M_TREE.into(),
        historical_manifest_sha256: String::new(),
        historical_package_sha256: String::new(),
        implementation_commit: legacy_artifact_policy::MERGED_JSON_N_COMMIT.into(),
        implementation_tree: legacy_artifact_policy::MERGED_JSON_N_TREE.into(),
        fixed_merged_additions: legacy_artifact_policy::merged_json42686c55_rows(),
        historical_runtime: vec![],
        candidate_runtime: vec![],
        helper_amendment_review_reference: String::new(),
        helper_amendment_source_rows: vec![],
        live_adjacent_amendment: LiveAdjacent42686c55Amendment {
            version: 1,
            policy: "AcceptedLiveAdjacentHelper42686c55".into(),
            disposition: "independently-accepted-live-helper-amendment".into(),
            reviewer: "control-only".into(),
            review_reference: "control-only".into(),
            implementation_commit: legacy_artifact_policy::MERGED_JSON_N_COMMIT.into(),
            implementation_tree: legacy_artifact_policy::MERGED_JSON_N_TREE.into(),
            live_git_blob: "b6f753c1d9cd5138cec0000dd1baf4ccb1125757".into(),
            historical_reuse: historical_adjacent_m_row(),
            live_helper: live_adjacent_42686c55_row(),
        },
    };
    assert!(fixture().identifies_scope());
    for defect in [
        "version2",
        "strict-policy",
        "adjacent-policy",
        "wrong-N",
        "wrong-M",
        "fixed-row",
        "old-helper",
        "missing-helper-review",
    ] {
        let mut value = fixture();
        match defect {
            "version2" => value.version = 2,
            "strict-policy" => value.policy = "ReviewedStartingRuntime".into(),
            "adjacent-policy" => value.policy = "ReviewedAdjacentRuntime".into(),
            "wrong-N" => {
                value.implementation_commit = legacy_artifact_policy::HISTORICAL_M_COMMIT.into()
            }
            "wrong-M" => value.historical_tree = legacy_artifact_policy::MERGED_JSON_N_TREE.into(),
            "fixed-row" => {
                value.fixed_merged_additions.pop();
            }
            "old-helper" => value.live_adjacent_amendment.live_helper = historical_adjacent_m_row(),
            _ => value.live_adjacent_amendment.review_reference.clear(),
        }
        assert!(!value.identifies_scope(), "{defect}");
    }
    assert!(
        serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(
            json!({"version":2,"policy":"ReviewedStartingRuntime"})
        )
        .is_err()
    );
    Ok(())
}

#[cfg(test)]
fn basic_starting443_complete_receipt_parser_fixtures() -> (Json, Json, Json) {
    // Complete parser shapes only: empty evidence rows and a non-acceptance
    // disposition cannot authorize emission. No document or file is produced.
    let strict = json!({
        "version":2,"policy":"ReviewedStartingRuntime","disposition":"control-only-not-an-acceptance",
        "reviewer":"control-only","review_reference":"control-only","source_bindings":[],"build_time_source_bindings":[],"commands":null,
        "historical_manifest_sha256":"","historical_package_sha256":"","historical_runtime":[],"candidate_runtime":[],
        "helper_amendment_review_reference":"","helper_amendment_source_rows":[]
    });
    let live = json!({
        "version":1,"policy":"AcceptedLiveAdjacentHelper42686c55","disposition":"control-only-not-an-acceptance",
        "reviewer":"control-only","review_reference":"control-only",
        "implementation_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,"implementation_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,
        "live_git_blob":"b6f753c1d9cd5138cec0000dd1baf4ccb1125757","historical_reuse":historical_adjacent_m_row(),"live_helper":live_adjacent_42686c55_row()
    });
    let mut merged = strict.clone();
    merged["version"] = json!(3);
    merged["policy"] = json!("ReviewedStartingRuntimeWithMergedJson42686c55");
    let composition_fields = json!({
        "historical_commit":legacy_artifact_policy::HISTORICAL_M_COMMIT,"historical_tree":legacy_artifact_policy::HISTORICAL_M_TREE,
        "implementation_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,"implementation_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,
        "fixed_merged_additions":legacy_artifact_policy::merged_json42686c55_rows(),"live_adjacent_amendment":live.clone()
    });
    for (key, value) in composition_fields.as_object().unwrap() {
        assert!(
            merged
                .as_object_mut()
                .unwrap()
                .insert(key.clone(), value.clone())
                .is_none()
        );
    }
    (strict, merged, live)
}

#[test]
fn basic_starting443_strict_receipt_parser_rejects_only_added_unknown_field() {
    let (mut baseline, _, _) = basic_starting443_complete_receipt_parser_fixtures();
    assert!(
        serde_json::from_value::<StartingSourceAcceptance>(baseline.clone()).is_ok(),
        "complete strict receipt baseline must deserialize"
    );
    assert!(
        baseline
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(
        serde_json::from_value::<StartingSourceAcceptance>(baseline).is_err(),
        "strict schema must reject the only added field"
    );
}

#[test]
fn basic_starting443_merged_outer_receipt_parser_rejects_only_added_unknown_field() {
    let (_, mut baseline, _) = basic_starting443_complete_receipt_parser_fixtures();
    assert!(
        serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(baseline.clone()).is_ok(),
        "complete merged receipt baseline must deserialize"
    );
    assert!(
        baseline
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(
        serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(baseline).is_err(),
        "outer schema must reject its only added field with a valid nested baseline"
    );
}

#[test]
fn basic_starting443_live_amendment_parser_rejects_only_added_unknown_field_standalone_and_nested()
{
    let (_, mut outer, mut nested) = basic_starting443_complete_receipt_parser_fixtures();
    assert!(
        serde_json::from_value::<LiveAdjacent42686c55Amendment>(nested.clone()).is_ok(),
        "complete standalone live amendment must deserialize"
    );
    assert!(
        serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(outer.clone()).is_ok(),
        "complete containing receipt must deserialize before nested mutation"
    );
    assert!(
        nested
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(
        serde_json::from_value::<LiveAdjacent42686c55Amendment>(nested).is_err(),
        "live amendment schema must reject its only added field"
    );
    assert!(
        outer["live_adjacent_amendment"]
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(
        serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(outer).is_err(),
        "nested schema must reject its only added field without an outer unknown field"
    );
}

const N_TO_P_DELTA_SHA256: &str =
    "4be8a8fb650fa1ce8c2c300612c16f8635f1b08cd093f7a1e3c0f8396f101db3";
const P_GIT_TREE_INDEX_SHA256: &str =
    "3e20c8fc4062afdd412449ab1d322086ff147b82870304429e893febac083f27";
const SOURCE_TARGET_ENV: &str = "FERRULE_BASIC_STARTING443_IMPLEMENTATION_TARGET";
const CANDIDATE_HEAD_ENV: &str = "FERRULE_BASIC_STARTING443_CANDIDATE_HEAD";
const CANDIDATE_TREE_ENV: &str = "FERRULE_BASIC_STARTING443_CANDIDATE_TREE";
#[derive(Clone, PartialEq, Eq)]
struct PSourcePin {
    candidate_head: String,
    candidate_tree: String,
}
fn candidate_object_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && value.bytes().any(|b| b != b'0')
}
fn selected_p_source_pin() -> TestResult<Option<PSourcePin>> {
    match std::env::var_os(SOURCE_TARGET_ENV) {
        None => Ok(None),
        Some(value) if value == OsStr::new(legacy_artifact_policy::MERGED_JSON_N_COMMIT) => {
            Ok(None)
        }
        Some(value) if value == OsStr::new(legacy_artifact_policy::IMPLEMENTATION_P_COMMIT) => {
            let pin = PSourcePin {
                candidate_head: std::env::var(CANDIDATE_HEAD_ENV)?,
                candidate_tree: std::env::var(CANDIDATE_TREE_ENV)?,
            };
            if !candidate_object_id(&pin.candidate_head)
                || !candidate_object_id(&pin.candidate_tree)
            {
                return Err(
                    "exact independently attested proposed candidate head/tree required for P"
                        .into(),
                );
            }
            Ok(Some(pin))
        }
        Some(_) => Err(
            "implementation target must be the explicit N or P base, not an arbitrary later main"
                .into(),
        ),
    }
}
fn p_receipt_route(version: Option<u64>, p_selected: bool) -> TestResult<bool> {
    match (version, p_selected) {
        (Some(4), true) => Ok(true),
        (Some(4), false) => Err("version4 requires explicit P target selection".into()),
        (_, true) => Err("version2/3 or unknown receipt cannot authorize a P target".into()),
        (_, false) => Ok(false),
    }
}
fn fixed_n_registry_row() -> legacy_artifact_policy::ManifestRow {
    (
        "crates/codegen-csharp/src/runtime.rs".into(),
        18045,
        "b8cc5d6409888ab34b764cda976b6cc4a4582ac63016df0f8f788b3ae384530d".into(),
    )
}
fn fixed_n_json_blobs() -> Vec<String> {
    legacy_artifact_policy::MERGED_JSON42686C55_FILES
        .iter()
        .map(|row| row.2.into())
        .collect()
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NToPBaseSourceAmendment {
    version: u32,
    policy: String,
    disposition: String,
    reviewer: String,
    review_reference: String,
    from_commit: String,
    from_tree: String,
    to_commit: String,
    to_tree: String,
    base_delta_sha256: String,
    target_tree_index_sha256: String,
    changed_paths: usize,
}
impl NToPBaseSourceAmendment {
    fn valid(&self) -> bool {
        self.version == 1
            && self.policy == "AcceptedBaseN42686c55ToPc9f9e083"
            && self.disposition == "independently-accepted-base-source-amendment"
            && !self.reviewer.trim().is_empty()
            && !self.review_reference.trim().is_empty()
            && self.from_commit == legacy_artifact_policy::MERGED_JSON_N_COMMIT
            && self.from_tree == legacy_artifact_policy::MERGED_JSON_N_TREE
            && self.to_commit == legacy_artifact_policy::IMPLEMENTATION_P_COMMIT
            && self.to_tree == legacy_artifact_policy::IMPLEMENTATION_P_TREE
            && self.base_delta_sha256 == N_TO_P_DELTA_SHA256
            && self.target_tree_index_sha256 == P_GIT_TREE_INDEX_SHA256
            && self.changed_paths == 148
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PLiveAdjacentBridge {
    version: u32,
    policy: String,
    disposition: String,
    reviewer: String,
    review_reference: String,
    origin_commit: String,
    origin_tree: String,
    implementation_commit: String,
    implementation_tree: String,
    origin_amendment: LiveAdjacent42686c55Amendment,
    current_helper: legacy_artifact_policy::ManifestRow,
    compiled_helper: legacy_artifact_policy::ManifestRow,
}
impl PLiveAdjacentBridge {
    fn valid(&self) -> bool {
        self.version == 1
            && self.policy == "AcceptedLiveAdjacent42686c55ForPc9f9e083"
            && self.disposition == "independently-accepted-live-helper-bridge"
            && !self.reviewer.trim().is_empty()
            && !self.review_reference.trim().is_empty()
            && self.origin_commit == legacy_artifact_policy::MERGED_JSON_N_COMMIT
            && self.origin_tree == legacy_artifact_policy::MERGED_JSON_N_TREE
            && self.implementation_commit == legacy_artifact_policy::IMPLEMENTATION_P_COMMIT
            && self.implementation_tree == legacy_artifact_policy::IMPLEMENTATION_P_TREE
            && self.origin_amendment.valid()
            && self.current_helper == live_adjacent_42686c55_row()
            && self.compiled_helper == live_adjacent_42686c55_row()
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PStartingSourceAcceptance {
    version: u32,
    policy: String,
    disposition: String,
    reviewer: String,
    review_reference: String,
    source_bindings: Vec<legacy_artifact_policy::ManifestRow>,
    build_time_source_bindings: Vec<legacy_artifact_policy::ManifestRow>,
    commands: Json,
    historical_commit: String,
    historical_tree: String,
    historical_manifest_sha256: String,
    historical_package_sha256: String,
    fixed_input_origin_commit: String,
    fixed_input_origin_tree: String,
    fixed_merged_additions: Vec<legacy_artifact_policy::ManifestRow>,
    fixed_json_git_blobs: Vec<String>,
    fixed_registry_source: legacy_artifact_policy::ManifestRow,
    implementation_commit: String,
    implementation_tree: String,
    candidate_head: String,
    candidate_tree: String,
    dependency_selection_review_reference: String,
    dependency_lock_bindings: Vec<legacy_artifact_policy::ManifestRow>,
    base_source_amendment: NToPBaseSourceAmendment,
    live_adjacent_bridge: PLiveAdjacentBridge,
    historical_runtime: Vec<legacy_artifact_policy::ManifestRow>,
    candidate_runtime: Vec<legacy_artifact_policy::ManifestRow>,
    helper_amendment_review_reference: String,
    helper_amendment_source_rows: Vec<legacy_artifact_policy::ManifestRow>,
}
impl PStartingSourceAcceptance {
    fn identifies_scope(&self) -> bool {
        self.version == 4
            && self.policy == "ReviewedStartingRuntimeWithMergedJson42686c55"
            && self.historical_commit == legacy_artifact_policy::HISTORICAL_M_COMMIT
            && self.historical_tree == legacy_artifact_policy::HISTORICAL_M_TREE
            && self.fixed_input_origin_commit == legacy_artifact_policy::MERGED_JSON_N_COMMIT
            && self.fixed_input_origin_tree == legacy_artifact_policy::MERGED_JSON_N_TREE
            && self.implementation_commit == legacy_artifact_policy::IMPLEMENTATION_P_COMMIT
            && self.implementation_tree == legacy_artifact_policy::IMPLEMENTATION_P_TREE
            && self.fixed_merged_additions == legacy_artifact_policy::merged_json42686c55_rows()
            && self.fixed_json_git_blobs == fixed_n_json_blobs()
            && self.fixed_registry_source == fixed_n_registry_row()
            && candidate_object_id(&self.candidate_head)
            && candidate_object_id(&self.candidate_tree)
            && self.base_source_amendment.valid()
            && self.live_adjacent_bridge.valid()
    }
    fn matches_pin(&self, pin: &PSourcePin) -> bool {
        self.candidate_head == pin.candidate_head && self.candidate_tree == pin.candidate_tree
    }
    fn matches_closure(
        &self,
        sources: &[legacy_artifact_policy::ManifestRow],
        commands: &Json,
    ) -> bool {
        self.source_bindings == sources
            && self.build_time_source_bindings == sources
            && self.commands == *commands
            && !self.dependency_selection_review_reference.trim().is_empty()
            && self.dependency_lock_bindings == p_dependency_source_rows(sources)
    }
}
fn p_dependency_source_rows(
    sources: &[legacy_artifact_policy::ManifestRow],
) -> Vec<legacy_artifact_policy::ManifestRow> {
    // This binds workspace locks/manifests/local patches from the FULL source
    // closure, not a claim that lock selection proves cached dependency bytes.
    // The independent P selection/cache review reference is separately required.
    sources
        .iter()
        .filter(|row| {
            row.0 == "Cargo.toml"
                || row.0 == "Cargo.lock"
                || row.0.ends_with("/Cargo.toml")
                || row.0.starts_with("vendor/")
                || row.0 == ".cargo/config"
                || row.0 == ".cargo/config.toml"
        })
        .cloned()
        .collect()
}
fn p_source_identity_record(pin: &PSourcePin) -> Json {
    json!({"historical_commit":legacy_artifact_policy::HISTORICAL_M_COMMIT,"historical_tree":legacy_artifact_policy::HISTORICAL_M_TREE,
        "fixed_input_origin_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,"fixed_input_origin_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,
        "implementation_commit":legacy_artifact_policy::IMPLEMENTATION_P_COMMIT,"implementation_tree":legacy_artifact_policy::IMPLEMENTATION_P_TREE,
        "attested_proposed_candidate_head":pin.candidate_head,"attested_proposed_candidate_tree":pin.candidate_tree,
        "fixed_json_git_blobs":fixed_n_json_blobs(),"fixed_registry_source":fixed_n_registry_row(),"not_an_acceptance":true})
}
fn p_base_source_binding_record() -> Json {
    json!({"from_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,"from_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,
        "to_commit":legacy_artifact_policy::IMPLEMENTATION_P_COMMIT,"to_tree":legacy_artifact_policy::IMPLEMENTATION_P_TREE,
        "base_delta_sha256":N_TO_P_DELTA_SHA256,"target_tree_index_sha256":P_GIT_TREE_INDEX_SHA256,"changed_paths":148,"independent_base_source_review_required":true})
}
fn accepted_p_emission_gate(evidence: &Evidence, bytes: &[u8]) -> TestResult<EmissionGate> {
    let pin = selected_p_source_pin()?.ok_or("explicit P source target required")?;
    let accepted: PStartingSourceAcceptance = serde_json::from_slice(bytes)?;
    let sources = source_bindings()?;
    let commands = command_runtime_binding()?;
    let (_, _, historical) = historical_inventory()?;
    let historical_runtime = legacy_artifact_policy::STARTING_RUNTIME_FILES
        .iter()
        .map(|(path, _)| {
            historical
                .iter()
                .find(|row| row.0 == *path)
                .cloned()
                .ok_or("historical runtime row")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !accepted.identifies_scope()
        || !accepted.matches_pin(&pin)
        || accepted.disposition != "independently-accepted-for-candidate-emission"
        || accepted.reviewer.trim().is_empty()
        || accepted.review_reference.trim().is_empty()
        || accepted.historical_manifest_sha256 != HISTORICAL_MANIFEST_SHA256
        || accepted.historical_package_sha256 != HISTORICAL_PACKAGE_SHA256
        || accepted.historical_runtime != historical_runtime
        || !accepted.matches_closure(&sources, &commands)
    {
        return Err("version4 must independently accept actual P-based source/build/tool closure, M history and fixed N origin".into());
    }
    if accepted.helper_amendment_review_reference.trim().is_empty()
        || accepted.helper_amendment_source_rows != helper_amendment_rows()?
    {
        return Err(
            "P acceptance must preserve exact nine helper rows and their independent reference"
                .into(),
        );
    }
    let snapshot = capture_runtime_review_inputs(evidence)?;
    let reviewed = legacy_artifact_policy::bind_starting_with_merged_json42686c55_for_p(
        snapshot,
        &accepted.candidate_runtime,
        &accepted.fixed_merged_additions,
        &accepted.fixed_input_origin_commit,
        &accepted.fixed_input_origin_tree,
        &accepted.implementation_commit,
        &accepted.implementation_tree,
    )?;
    evidence.json(
        "P-TARGET-BOUND-IDENTITY.json",
        &p_source_identity_record(&pin),
    )?;
    let gate = EmissionGate {
        sources,
        commands,
        reviewed,
        p_source_pin: Some(pin),
    };
    gate.check()?;
    Ok(gate)
}

#[cfg(test)]
fn basic_starting443_complete_p_receipt_parser_fixture() -> Json {
    // Complete parser transport data, never a real acceptance: the outer
    // disposition/evidence/runtime rows are deliberately non-authorizing.
    let (_, mut outer, origin) = basic_starting443_complete_receipt_parser_fixtures();
    assert!(
        outer
            .as_object_mut()
            .unwrap()
            .remove("live_adjacent_amendment")
            .is_some()
    );
    outer["version"] = json!(4);
    outer["implementation_commit"] = json!(legacy_artifact_policy::IMPLEMENTATION_P_COMMIT);
    outer["implementation_tree"] = json!(legacy_artifact_policy::IMPLEMENTATION_P_TREE);
    let added = json!({
        "fixed_input_origin_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,"fixed_input_origin_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,
        "fixed_json_git_blobs":fixed_n_json_blobs(),"fixed_registry_source":fixed_n_registry_row(),
        "candidate_head":"1".repeat(40),"candidate_tree":"2".repeat(40),
        "dependency_selection_review_reference":"control-only","dependency_lock_bindings":[],
        "base_source_amendment":{"version":1,"policy":"AcceptedBaseN42686c55ToPc9f9e083","disposition":"control-only-not-an-acceptance",
            "reviewer":"control-only","review_reference":"control-only","from_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,
            "from_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,"to_commit":legacy_artifact_policy::IMPLEMENTATION_P_COMMIT,
            "to_tree":legacy_artifact_policy::IMPLEMENTATION_P_TREE,"base_delta_sha256":N_TO_P_DELTA_SHA256,
            "target_tree_index_sha256":P_GIT_TREE_INDEX_SHA256,"changed_paths":148},
        "live_adjacent_bridge":{"version":1,"policy":"AcceptedLiveAdjacent42686c55ForPc9f9e083","disposition":"control-only-not-an-acceptance",
            "reviewer":"control-only","review_reference":"control-only","origin_commit":legacy_artifact_policy::MERGED_JSON_N_COMMIT,
            "origin_tree":legacy_artifact_policy::MERGED_JSON_N_TREE,"implementation_commit":legacy_artifact_policy::IMPLEMENTATION_P_COMMIT,
            "implementation_tree":legacy_artifact_policy::IMPLEMENTATION_P_TREE,"origin_amendment":origin,
            "current_helper":live_adjacent_42686c55_row(),"compiled_helper":live_adjacent_42686c55_row()}
    });
    for (key, value) in added.as_object().unwrap() {
        assert!(
            outer
                .as_object_mut()
                .unwrap()
                .insert(key.clone(), value.clone())
                .is_none()
        );
    }
    outer
}

#[test]
fn basic_starting443_p_outer_parser_rejects_only_added_unknown_field() {
    let mut baseline = basic_starting443_complete_p_receipt_parser_fixture();
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(baseline.clone()).is_ok());
    assert!(
        baseline
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(baseline).is_err());
}
#[test]
fn basic_starting443_p_base_amendment_parser_rejects_only_added_unknown_field() {
    let mut outer = basic_starting443_complete_p_receipt_parser_fixture();
    let mut nested = outer["base_source_amendment"].clone();
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(outer.clone()).is_ok());
    assert!(serde_json::from_value::<NToPBaseSourceAmendment>(nested.clone()).is_ok());
    assert!(
        nested
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<NToPBaseSourceAmendment>(nested).is_err());
    assert!(
        outer["base_source_amendment"]
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(outer).is_err());
}
#[test]
fn basic_starting443_p_helper_bridge_parser_rejects_only_added_unknown_field() {
    let mut outer = basic_starting443_complete_p_receipt_parser_fixture();
    let mut nested = outer["live_adjacent_bridge"].clone();
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(outer.clone()).is_ok());
    assert!(serde_json::from_value::<PLiveAdjacentBridge>(nested.clone()).is_ok());
    assert!(
        nested
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<PLiveAdjacentBridge>(nested).is_err());
    assert!(
        outer["live_adjacent_bridge"]
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(outer).is_err());
}
#[test]
fn basic_starting443_p_nested_n_origin_parser_rejects_only_added_unknown_field() {
    let mut outer = basic_starting443_complete_p_receipt_parser_fixture();
    let mut bridge = outer["live_adjacent_bridge"].clone();
    let mut origin = bridge["origin_amendment"].clone();
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(outer.clone()).is_ok());
    assert!(serde_json::from_value::<PLiveAdjacentBridge>(bridge.clone()).is_ok());
    assert!(serde_json::from_value::<LiveAdjacent42686c55Amendment>(origin.clone()).is_ok());
    assert!(
        origin
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<LiveAdjacent42686c55Amendment>(origin).is_err());
    assert!(
        bridge["origin_amendment"]
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<PLiveAdjacentBridge>(bridge).is_err());
    assert!(
        outer["live_adjacent_bridge"]["origin_amendment"]
            .as_object_mut()
            .unwrap()
            .insert("unreviewed_extra".into(), json!(true))
            .is_none()
    );
    assert!(serde_json::from_value::<PStartingSourceAcceptance>(outer).is_err());
}

#[cfg(test)]
fn basic_starting443_p_header_only_fixture() -> PStartingSourceAcceptance {
    let mut value = basic_starting443_complete_p_receipt_parser_fixture();
    value["base_source_amendment"]["disposition"] =
        json!("independently-accepted-base-source-amendment");
    value["live_adjacent_bridge"]["disposition"] =
        json!("independently-accepted-live-helper-bridge");
    value["live_adjacent_bridge"]["origin_amendment"]["disposition"] =
        json!("independently-accepted-live-helper-amendment");
    serde_json::from_value(value).unwrap()
}
#[test]
fn basic_starting443_p_route_header_and_wrong_base_controls() -> TestResult<()> {
    assert!(p_receipt_route(Some(4), true)?);
    assert!(!p_receipt_route(Some(3), false)?);
    assert!(!p_receipt_route(Some(2), false)?);
    assert!(p_receipt_route(Some(4), false).is_err());
    for version in [None, Some(2), Some(3), Some(5)] {
        assert!(p_receipt_route(version, true).is_err());
    }
    let baseline = basic_starting443_p_header_only_fixture();
    assert!(baseline.identifies_scope());
    let pin = PSourcePin {
        candidate_head: "1".repeat(40),
        candidate_tree: "2".repeat(40),
    };
    assert!(baseline.matches_pin(&pin));
    for policy in [
        "ExactHistorical",
        "ReviewedStartingRuntime",
        "ReviewedAdjacentRuntime",
    ] {
        let mut value = basic_starting443_p_header_only_fixture();
        value.policy = policy.into();
        assert!(!value.identifies_scope());
    }
    for defect in [
        "version3",
        "wrong-M",
        "wrong-M-tree",
        "origin-P",
        "origin-tree-P",
        "target-N",
        "target-tree-N",
        "fixed-JSON",
        "fixed-blob",
        "registry",
        "delta",
        "amendment-from",
        "amendment-count",
        "amendment-reference",
        "bridge-reference",
        "bridge-origin-P",
        "bridge-target-N",
        "retag-N-authority-as-P",
        "wrong-helper",
        "bad-head",
    ] {
        let mut value = basic_starting443_p_header_only_fixture();
        match defect {
            "version3" => value.version = 3,
            "wrong-M" => {
                value.historical_commit = legacy_artifact_policy::MERGED_JSON_N_COMMIT.into()
            }
            "wrong-M-tree" => {
                value.historical_tree = legacy_artifact_policy::MERGED_JSON_N_TREE.into()
            }
            "origin-P" => {
                value.fixed_input_origin_commit =
                    legacy_artifact_policy::IMPLEMENTATION_P_COMMIT.into()
            }
            "origin-tree-P" => {
                value.fixed_input_origin_tree = legacy_artifact_policy::IMPLEMENTATION_P_TREE.into()
            }
            "target-N" => {
                value.implementation_commit = legacy_artifact_policy::MERGED_JSON_N_COMMIT.into()
            }
            "target-tree-N" => {
                value.implementation_tree = legacy_artifact_policy::MERGED_JSON_N_TREE.into()
            }
            "fixed-JSON" => {
                value.fixed_merged_additions.pop();
            }
            "fixed-blob" => value.fixed_json_git_blobs.swap(0, 1),
            "registry" => value.fixed_registry_source.2 = "0".repeat(64),
            "delta" => value.base_source_amendment.base_delta_sha256 = "0".repeat(64),
            "amendment-from" => {
                value.base_source_amendment.from_commit =
                    legacy_artifact_policy::IMPLEMENTATION_P_COMMIT.into()
            }
            "amendment-count" => value.base_source_amendment.changed_paths = 147,
            "amendment-reference" => value.base_source_amendment.review_reference.clear(),
            "bridge-reference" => value.live_adjacent_bridge.review_reference.clear(),
            "bridge-origin-P" => {
                value.live_adjacent_bridge.origin_commit =
                    legacy_artifact_policy::IMPLEMENTATION_P_COMMIT.into()
            }
            "bridge-target-N" => {
                value.live_adjacent_bridge.implementation_commit =
                    legacy_artifact_policy::MERGED_JSON_N_COMMIT.into()
            }
            "retag-N-authority-as-P" => {
                value
                    .live_adjacent_bridge
                    .origin_amendment
                    .implementation_commit = legacy_artifact_policy::IMPLEMENTATION_P_COMMIT.into()
            }
            "wrong-helper" => {
                value.live_adjacent_bridge.compiled_helper = historical_adjacent_m_row()
            }
            _ => value.candidate_head = "0".repeat(40),
        }
        assert!(!value.identifies_scope(), "{defect}");
    }
    let mut changed = basic_starting443_p_header_only_fixture();
    changed.candidate_head = "3".repeat(40);
    assert!(!changed.matches_pin(&pin));
    let mut changed = basic_starting443_p_header_only_fixture();
    changed.candidate_tree = "4".repeat(40);
    assert!(!changed.matches_pin(&pin));
    let (_, mut n, _) = basic_starting443_complete_receipt_parser_fixtures();
    n["live_adjacent_amendment"]["disposition"] =
        json!("independently-accepted-live-helper-amendment");
    assert!(
        serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(n.clone())
            .unwrap()
            .identifies_scope(),
        "complete N header must be valid before changing only its target"
    );
    n["implementation_commit"] = json!(legacy_artifact_policy::IMPLEMENTATION_P_COMMIT);
    n["implementation_tree"] = json!(legacy_artifact_policy::IMPLEMENTATION_P_TREE);
    assert!(
        !serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(n)
            .unwrap()
            .identifies_scope(),
        "unchanged version3 route still rejects P identity"
    );
    assert!(
        serde_json::from_value::<MergedStarting42686c55SourceAcceptance>(
            basic_starting443_complete_p_receipt_parser_fixture()
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<PStartingSourceAcceptance>(
            basic_starting443_complete_receipt_parser_fixtures().1
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn basic_starting443_p_source_build_command_and_dependency_closure_mismatches_are_refused() {
    let sources = vec![
        legacy_artifact_policy::row("Cargo.lock", b"literal P workspace lock"),
        legacy_artifact_policy::row(
            "vendor/egui-snarl/Cargo.toml",
            b"literal P local-patch manifest",
        ),
    ];
    let commands = json!({"control":"independently fixed P command/native/config binding"});
    let mut baseline = basic_starting443_p_header_only_fixture();
    baseline.source_bindings = sources.clone();
    baseline.build_time_source_bindings = sources.clone();
    baseline.commands = commands.clone();
    baseline.dependency_lock_bindings = p_dependency_source_rows(&sources);
    assert!(baseline.matches_closure(&sources, &commands));
    baseline.source_bindings[0].2 = "0".repeat(64);
    assert!(!baseline.matches_closure(&sources, &commands));
    baseline.source_bindings = sources.clone();
    baseline.build_time_source_bindings[0].2 = "0".repeat(64);
    assert!(!baseline.matches_closure(&sources, &commands));
    baseline.build_time_source_bindings = sources.clone();
    baseline.commands = json!({"control":"stale N command"});
    assert!(!baseline.matches_closure(&sources, &commands));
    baseline.commands = commands.clone();
    baseline.dependency_lock_bindings.pop();
    assert!(!baseline.matches_closure(&sources, &commands));
    baseline.dependency_lock_bindings = p_dependency_source_rows(&sources);
    baseline.dependency_selection_review_reference.clear();
    assert!(!baseline.matches_closure(&sources, &commands));
}
