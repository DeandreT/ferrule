// Distinct transport/fence controls. These literal sources never construct a
// mapping or supply a campaign oracle, and do not count toward the 1,149 calls.
fn prepared_input_control_fixture(evidence: &Evidence) -> TestResult<(Json, Vec<u8>)> {
    let literal = [
        ("csharp-host0/Host.csproj", b"<Project />".as_slice()),
        (
            "csharp-host0/Program.cs",
            b"// immutable host zero".as_slice(),
        ),
        ("csharp-host1/Host.csproj", b"<Project />".as_slice()),
        (
            "csharp-host1/Program.cs",
            b"// immutable host one".as_slice(),
        ),
        (
            "csharp-model1/Ferrule.Generated.csproj",
            b"<Project />".as_slice(),
        ),
        (
            "csharp-model1/GeneratedMapping.cs",
            b"// immutable mapping source".as_slice(),
        ),
        ("rust-host/Cargo.toml", b"# immutable manifest".as_slice()),
        ("rust-host/src/main.rs", b"// immutable main".as_slice()),
    ];
    for (path, bytes) in literal {
        evidence.write(path, bytes)?;
    }
    let rows = literal
        .iter()
        .map(|(path, bytes)| legacy_artifact_policy::row(path, bytes))
        .collect::<Vec<_>>();
    let prepared = json!({"evidence":rows});
    let success = serde_json::to_vec_pretty(&prepared)?;
    evidence.write(SUCCESS, &success)?;
    Ok((prepared, success))
}

#[test]
fn basic_starting443_complete_prepared_pathset_rejects_independent_source_and_config_additions()
-> TestResult<()> {
    // Explicit expected failures independent of observed input lists, compiler
    // outputs or candidate implementation. Includes a DIFFERENT next host.
    for addition in [
        "csharp-host1/Extra.cs",
        "csharp-host1/Directory.Build.targets",
        "csharp-model1/Directory.Build.props",
        "Directory.Build.targets",
        "rust-host/.cargo/config.toml",
        "csharp-host1/obj/Extra.cs",
        "csharp-host1/bin/Extra.cs",
        "unprepared-host/Host.csproj",
    ] {
        for boundary in ["before-build", "after-build"] {
            let temp = TempDir::new("starting443-addition-fence")?;
            let evidence = Evidence(temp.0.canonicalize()?);
            let (prepared, success) = prepared_input_control_fixture(&evidence)?;
            let mut fence = PreparedInputFence::new(&prepared, &success)?;
            assert_eq!(fence.inspect(&evidence.0, false)?["matched"], true);
            evidence.write(addition, b"unreviewed addition sentinel")?;
            assert!(
                fence.check(&evidence, boundary, false).is_err(),
                "{boundary}: {addition}"
            );
            let observed: Json = serde_json::from_slice(&fs::read(
                evidence
                    .0
                    .join(format!("prepared-input-fences/{boundary}.json")),
            )?)?;
            assert_eq!(observed["matched"], false);
            assert!(
                observed["unexpected_entries"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(addition))
            );
        }
    }
    Ok(())
}

#[test]
fn basic_starting443_only_explicit_genuine_outputs_survive_the_complete_input_fence()
-> TestResult<()> {
    let temp = TempDir::new("starting443-output-allowances")?;
    let evidence = Evidence(temp.0.canonicalize()?);
    let (prepared, success) = prepared_input_control_fixture(&evidence)?;
    let mut fence = PreparedInputFence::new(&prepared, &success)?;
    for output in [
        "dotnet-artifacts-model1/obj/Host/release/generated.cs",
        "rust-build-target/debug/host.o",
        "host-originals/starting443-csharp-model1-build/status.original.json",
        "native/00-All-COMPLETE-INPUT-AFTER.json",
        "run-contracts/rust.json",
        "starting443-rust-build-COMPLETE-EXECUTION-ROUTE.json",
    ] {
        evidence.write(output, b"genuine output transport sentinel")?;
    }
    assert_eq!(fence.inspect(&evidence.0, false)?["matched"], true);
    evidence.write(
        "rust-host/Cargo.lock",
        b"generated dependency lock sentinel",
    )?;
    assert_eq!(
        fence.inspect(&evidence.0, false)?["matched"],
        false,
        "pre-build lock addition refused"
    );
    assert_eq!(
        fence.inspect(&evidence.0, true)?["matched"],
        true,
        "only first Rust post-build may bind generated lock"
    );
    assert_eq!(fence.inspect(&evidence.0, false)?["matched"], true);
    fs::write(
        evidence.0.join("rust-host/Cargo.lock"),
        b"changed dependency lock",
    )?;
    assert_eq!(
        fence.inspect(&evidence.0, true)?["matched"],
        false,
        "postfence cannot rebaseline an already bound lock"
    );
    Ok(())
}

#[test]
fn basic_starting443_complete_input_fence_rejects_removed_changed_and_empty_added_inputs()
-> TestResult<()> {
    for defect in ["removed", "changed", "directory"] {
        let temp = TempDir::new("starting443-input-pathset")?;
        let evidence = Evidence(temp.0.canonicalize()?);
        let (prepared, success) = prepared_input_control_fixture(&evidence)?;
        let mut fence = PreparedInputFence::new(&prepared, &success)?;
        let source = evidence.0.join("csharp-host1/Program.cs");
        match defect {
            "removed" => fs::remove_file(source)?,
            "changed" => fs::write(source, b"unreviewed replacement")?,
            _ => fs::create_dir(evidence.0.join("csharp-host1/.cargo"))?,
        }
        assert_eq!(
            fence.inspect(&evidence.0, false)?["matched"],
            false,
            "{defect}"
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn basic_starting443_output_allowance_is_not_a_symlink_escape_from_the_input_fence()
-> TestResult<()> {
    let temp = TempDir::new("starting443-output-root-link")?;
    let evidence = Evidence(temp.0.canonicalize()?);
    let (prepared, success) = prepared_input_control_fixture(&evidence)?;
    let mut fence = PreparedInputFence::new(&prepared, &success)?;
    std::os::unix::fs::symlink(
        evidence.0.join("csharp-host1"),
        evidence.0.join("dotnet-artifacts-model1"),
    )?;
    assert_eq!(fence.inspect(&evidence.0, false)?["matched"], false);
    Ok(())
}

#[test]
fn basic_starting443_path_keys_preserve_components_and_refuse_all_duplicate_insertions()
-> TestResult<()> {
    assert_eq!(
        campaign_path_key(&Path::new("csharp-host1").join("Program.cs"))?,
        "csharp-host1/Program.cs"
    );
    for path in [
        "",
        "./Program.cs",
        "csharp-host1/./Program.cs",
        "csharp-host1//Program.cs",
        "csharp-host1/../Program.cs",
        "csharp-host1/Program.cs/",
    ] {
        assert!(campaign_path_key(Path::new(path)).is_err(), "{path:?}");
    }
    let mut files = BTreeMap::new();
    campaign_insert_path(
        &mut files,
        "csharp-host1/Program.cs".into(),
        b"original".to_vec(),
    )?;
    for bytes in [b"original".as_slice(), b"replacement".as_slice()] {
        assert!(
            campaign_insert_path(&mut files, "csharp-host1/Program.cs".into(), bytes.to_vec())
                .is_err()
        );
        assert_eq!(
            files["csharp-host1/Program.cs"].as_slice(),
            b"original".as_slice()
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    campaign_insert_path_set(&mut seen, "csharp-host1".into())?;
    assert!(campaign_insert_path_set(&mut seen, "csharp-host1".into()).is_err());
    assert!(campaign_path_set(["native".into(), "native".into()]).is_err());
    let temp = TempDir::new("starting443-duplicate-original")?;
    let evidence = Evidence(temp.0.canonicalize()?);
    let (mut prepared, success) = prepared_input_control_fixture(&evidence)?;
    let duplicate = prepared["evidence"][0].clone();
    prepared["evidence"].as_array_mut().unwrap().push(duplicate);
    assert!(PreparedInputFence::new(&prepared, &success).is_err());
    assert!(
        campaign_insert_path(
            &mut files,
            r"csharp-host1\Program.cs".into(),
            b"original".to_vec()
        )
        .is_err(),
        "portable manifest keys never use backslashes"
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn basic_starting443_native_windows_separators_are_serialized_between_components() -> TestResult<()>
{
    assert_eq!(
        campaign_path_key(Path::new(r"csharp-host1\Program.cs"))?,
        "csharp-host1/Program.cs"
    );
    assert!(campaign_path_key(Path::new(r"csharp-host1\\Program.cs")).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn basic_starting443_literal_backslash_additions_and_lock_substitution_are_refused()
-> TestResult<()> {
    // Independent literal names/bytes: the first alias has IDENTICAL bytes to
    // the real Program.cs, so hashing either cannot hide the extra root entry.
    for alias in [
        r"csharp-host1\Program.cs",
        r"csharp-host1\Extra.cs",
        r"csharp-host1\Directory.Build.targets",
        r"dotnet-artifacts-model1\obj\Host\release\generated.cs",
    ] {
        let temp = TempDir::new("starting443-literal-alias")?;
        let evidence = Evidence(temp.0.canonicalize()?);
        let (prepared, success) = prepared_input_control_fixture(&evidence)?;
        let mut fence = PreparedInputFence::new(&prepared, &success)?;
        assert_eq!(fence.inspect(&evidence.0, false)?["matched"], true);
        fs::write(evidence.0.join(alias), b"// immutable host one")?;
        assert!(
            fence.check(&evidence, "alias-before", false).is_err(),
            "{alias}"
        );
        assert!(
            fence.check(&evidence, "alias-after", false).is_err(),
            "{alias}"
        );
    }
    for bound in [false, true] {
        let temp = TempDir::new("starting443-lock-alias")?;
        let evidence = Evidence(temp.0.canonicalize()?);
        let (prepared, success) = prepared_input_control_fixture(&evidence)?;
        let mut fence = PreparedInputFence::new(&prepared, &success)?;
        let lock = b"literal generated lock sentinel";
        if bound {
            evidence.write("rust-host/Cargo.lock", lock)?;
            assert_eq!(fence.inspect(&evidence.0, true)?["matched"], true);
            fs::rename(
                evidence.0.join("rust-host/Cargo.lock"),
                evidence.0.join(r"rust-host\Cargo.lock"),
            )?;
        } else {
            fs::write(evidence.0.join(r"rust-host\Cargo.lock"), lock)?;
        }
        assert!(!evidence.0.join("rust-host/Cargo.lock").exists());
        assert!(
            fence.inspect(&evidence.0, false).is_err(),
            "alias cannot replace required native lock path"
        );
        assert!(
            fence.inspect(&evidence.0, true).is_err(),
            "alias cannot enter through the first-build lock allowance"
        );
        assert_eq!(
            fence.generated_lock.is_some(),
            bound,
            "failed alias inspection must not create a lock binding"
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn basic_starting443_related_artifact_and_archival_scans_refuse_aliases_and_non_utf8_names()
-> TestResult<()> {
    let temp = TempDir::new("starting443-binding-alias")?;
    let evidence = Evidence(temp.0.canonicalize()?);
    let bytes = b"// independently fixed source";
    evidence.write("csharp-host1/Program.cs", bytes)?;
    let manifest = format!("{}  ./csharp-host1/Program.cs\n", digest(bytes));
    assert_eq!(
        artifact_files(&evidence.0)?,
        vec![("csharp-host1/Program.cs".into(), bytes.to_vec())]
    );
    verify_archival_manifest(&evidence.0, &manifest, "./", &[])?;
    // Addition first, then replacement: neither order can select a winner.
    fs::write(evidence.0.join(r"csharp-host1\Program.cs"), bytes)?;
    assert!(artifact_files(&evidence.0).is_err());
    assert!(verify_archival_manifest(&evidence.0, &manifest, "./", &[]).is_err());
    fs::remove_file(evidence.0.join("csharp-host1/Program.cs"))?;
    assert!(artifact_files(&evidence.0).is_err());
    assert!(verify_archival_manifest(&evidence.0, &manifest, "./", &[]).is_err());
    fs::remove_file(evidence.0.join(r"csharp-host1\Program.cs"))?;
    use std::os::unix::ffi::OsStringExt as _;
    let invalid = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
    fs::write(evidence.0.join(invalid), bytes)?;
    assert!(
        artifact_files(&evidence.0).is_err(),
        "no replacement-character filename keys"
    );
    Ok(())
}
