use std::path::{Path, PathBuf};

use mfd::MfdPackage;
use mfd_package_fuzz::program::{
    MAX_PROGRAM_BYTES, MAX_PROGRAM_ENTRIES, MAX_PROGRAM_ENTRY_BYTES, Name, Op, encode,
};
use mfd_package_fuzz::{
    KnownLimit, MAX_EXEMPT_ENTRY_DEPTH, MAX_EXEMPT_XSD_ELEMENTS, MAX_INPUT_BYTES, check, decode,
    documented_path, known_limit_exemption, sandbox, seeds,
};

/// The outcome label (see `Outcome::label`) each seed was written to reach.
const EXPECTED: &[(&str, &str)] = &[
    ("db_source", "package FilesystemResource"),
    (
        "design_component_elements_below_limit",
        "imported with warnings",
    ),
    ("design_deep_component_elements", "imported with warnings"),
    ("design_deep_elements", "error NotMfd"),
    ("design_deep_resources", "imported"),
    ("design_deep_source_entries", "exempt deep entry tree"),
    ("design_deep_target_entries", "exempt deep entry tree"),
    ("design_entity_expansion", "error Xml"),
    ("design_invalid_utf8", "package InvalidText"),
    ("design_not_mapping", "error NotMfd"),
    ("design_path_case_alias", "refused MissingEntry"),
    ("design_path_traversal", "refused InvalidPath"),
    ("design_ref_absolute", "package InvalidPath"),
    ("design_ref_backslash", "package InvalidPath"),
    ("design_ref_case_alias", "package MissingEntry"),
    ("design_ref_dot", "package InvalidPath"),
    ("design_ref_drive", "package InvalidPath"),
    ("design_ref_empty", "package InvalidPath"),
    ("design_ref_normalized_parent", "package InvalidPath"),
    ("design_ref_parent", "package InvalidPath"),
    ("design_ref_url", "package InvalidPath"),
    ("design_ref_virtual_root", "package InvalidPath"),
    ("design_truncated", "error Xml"),
    ("dtd_source", "imported"),
    ("dtd_truncated", "imported with warnings"),
    ("duplicate_exact", "refused DuplicatePath"),
    ("duplicate_first_letter_case", "refused DuplicatePath"),
    ("duplicate_upper_case", "refused DuplicatePath"),
    ("edi_idoc_source", "package FilesystemResource"),
    ("edi_swift_source", "package FilesystemResource"),
    ("empty", "refused MissingEntry"),
    ("entry_substitution_expansion", "refused LimitExceeded"),
    ("entry_at_byte_limit", "imported"),
    ("entry_count_at_limit", "imported"),
    ("entry_count_past_limit", "refused LimitExceeded"),
    ("entry_past_byte_limit", "refused LimitExceeded"),
    ("entry_path_absolute", "refused InvalidPath"),
    ("entry_path_at_limit", "imported"),
    ("entry_path_backslash", "refused InvalidPath"),
    ("entry_path_control", "refused InvalidPath"),
    ("entry_path_deep_directories", "refused InvalidPath"),
    ("entry_path_dot_segment", "refused InvalidPath"),
    ("entry_path_drive", "refused InvalidPath"),
    ("entry_path_empty_segment", "refused InvalidPath"),
    ("entry_path_invalid_utf8", "imported"),
    ("entry_path_nul", "refused InvalidPath"),
    ("entry_path_past_limit", "refused InvalidPath"),
    ("entry_path_trailing_dot", "refused InvalidPath"),
    ("entry_path_trailing_space", "refused InvalidPath"),
    ("entry_path_unicode", "imported"),
    ("entry_path_url", "refused InvalidPath"),
    ("instance_deep", "imported"),
    ("include_ref_absolute", "package InvalidPath"),
    ("include_ref_parent", "package InvalidPath"),
    ("include_ref_url", "package InvalidPath"),
    ("json_chain", "imported"),
    // Silent substitution, #1169.
    ("json_chain_cycle", "imported"),
    ("json_chain_dangling", "package MissingEntry"),
    ("json_chain_long", "imported"),
    ("json_ref", "imported"),
    ("json_ref_absolute", "package InvalidPath"),
    ("json_ref_parent", "package InvalidPath"),
    ("json_schema_deep", "imported with warnings"),
    ("json_schema_invalid_utf8", "imported with warnings"),
    // Silent substitution, #1169.
    ("json_schema_not_object", "imported"),
    ("json_schema_truncated", "imported with warnings"),
    ("json_to_xml", "imported"),
    ("missing_dtd", "package MissingEntry"),
    ("missing_included_xsd", "package MissingEntry"),
    ("missing_json_pointer", "package InvalidResource"),
    ("missing_local_json_pointer", "package InvalidResource"),
    ("missing_nested_schema", "package MissingEntry"),
    ("missing_referenced_json", "package MissingEntry"),
    ("missing_source_schema", "package MissingEntry"),
    ("missing_target_schema", "package MissingEntry"),
    ("nested_design", "imported"),
    ("nested_design_root_reference", "package MissingEntry"),
    ("nested_xsd_chain", "imported"),
    ("raw_garbage_design", "error Xml"),
    ("raw_minimal_design", "error NotMfd"),
    ("total_at_byte_limit", "error Xml"),
    ("total_past_byte_limit", "refused LimitExceeded"),
    ("unreferenced_entries", "imported"),
    ("xml_to_json", "imported"),
    ("xsd_chain", "imported"),
    ("xsd_chain_cycle", "imported with warnings"),
    ("xsd_chain_dangling", "package MissingEntry"),
    ("xsd_chain_long", "imported with warnings"),
    ("xsd_deep_nesting", "exempt large XSD"),
    ("xsd_deep_appinfo", "imported"),
    ("xsd_deep_types", "imported"),
    ("xsd_include", "imported"),
    ("xsd_invalid_utf8", "imported with warnings"),
    ("xsd_named_type_chain", "imported"),
    ("xsd_not_schema", "imported with warnings"),
    ("xsd_truncated", "imported with warnings"),
];

/// Debug builds use far more stack per frame than the release fuzz build, so
/// checks run on a thread with a stack large enough for the deepest input
/// the known-limit exemptions still import.
fn with_large_stack(test: impl FnOnce() + Send + 'static) {
    sandbox::warn_if_unsupported();
    let worker = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(test)
        .unwrap_or_else(|error| panic!("could not spawn the check thread: {error}"));
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_seed_reaches_its_intended_outcome() {
    with_large_stack(|| {
        let seeds = seeds::all();
        for (name, _) in EXPECTED {
            assert!(
                seeds.iter().any(|(seed, _)| seed == name),
                "expectation for unknown seed {name}"
            );
        }
        for (name, bytes) in &seeds {
            assert!(
                bytes.len() <= MAX_INPUT_BYTES,
                "{name} exceeds the input cap"
            );
            let expected = EXPECTED
                .iter()
                .find(|(seed, _)| seed == name)
                .map(|(_, label)| *label)
                .unwrap_or_else(|| panic!("seed {name} has no expected outcome"));
            let outcome = check(bytes);
            assert_eq!(outcome.label(), expected, "{name}: {outcome:?}");
        }
    });
}

/// Every prefix of every short seed exercises truncation at each byte
/// boundary; longer seeds are cut at 64 evenly spaced points.
#[test]
fn seed_prefixes_satisfy_the_invariants() {
    with_large_stack(|| {
        for (_, bytes) in seeds::all() {
            let step = (bytes.len() / 64).max(1);
            for end in (0..bytes.len()).step_by(step) {
                check(&bytes[..end]);
            }
        }
    });
}

#[test]
fn checked_in_seeds_match_the_generator() {
    let dir = manifest_dir().join("seeds");
    let mut expected: Vec<(String, Vec<u8>)> = seeds::all()
        .into_iter()
        .map(|(name, bytes)| (format!("{name}.bin"), bytes))
        .collect();
    expected.sort();
    let mut actual: Vec<(String, Vec<u8>)> = read_dir(&dir)
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .unwrap_or_default();
            let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
            (name, bytes)
        })
        .collect();
    actual.sort();
    assert!(
        actual == expected,
        "seeds/ is stale; run `cargo run --example write_seeds`"
    );
}

/// Minimized reproductions saved under `regressions/` must keep passing.
#[test]
fn regressions_satisfy_the_invariants() {
    let inputs: Vec<_> = read_dir(&manifest_dir().join("regressions"))
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
            (path, bytes)
        })
        .collect();
    with_large_stack(move || {
        for (path, bytes) in inputs {
            let outcome = check(&bytes);
            // 20,000 unclosed elements, which overflowed roxmltree before
            // 0.21.2 parsed element content iteratively.
            if path.ends_with("deep_design_xml.bin") {
                assert_eq!(outcome.label(), "error Xml", "{path:?}");
            }
        }
    });
}

#[test]
fn programs_stay_within_their_limits() {
    let mut program = vec![0];
    // Twice the entry cap of filler at the entry byte limit, then wrap an
    // entry far past the per-entry limit.
    program.extend([10, 255, 3, 10, 255, 4]);
    program.extend([11, 0, 0xff, 0xff, 1, 0, b'<', 0, 0]);
    let candidate = decode(&program);
    assert!(candidate.entries.len() <= MAX_PROGRAM_ENTRIES);
    assert!(candidate.total_bytes() <= MAX_PROGRAM_BYTES);
    assert!(
        candidate
            .entries
            .iter()
            .all(|(_, contents)| contents.len() <= MAX_PROGRAM_ENTRY_BYTES)
    );
    let decoded_again = decode(&program);
    assert_eq!(candidate, decoded_again, "decoding is not deterministic");
}

/// Replacing every byte of a 4 MiB filler entry with 256 bytes would build
/// 1 GiB; decoding stops each entry at its cap instead, so the result never
/// holds more than about twice the entry limit.
#[test]
fn expanding_substitutions_stop_at_the_entry_limit() {
    let replacement = [b'x'; 256];
    let program = encode(
        "empty",
        &[
            Op::Filler(5, 3),
            Op::Substitute(0, Name::Raw(b"#"), Name::Raw(&replacement)),
            Op::Substitute(1, Name::Raw(b"#"), Name::Raw(&replacement)),
            Op::Substitute(0, Name::Raw(b"x"), Name::Raw(&replacement)),
            Op::Alias(0, mfd_package_fuzz::program::Alias::Upper),
            Op::Wrap(4, u16::MAX, &replacement, &replacement),
        ],
    );
    let candidate = decode(&program);
    assert!(candidate.total_bytes() <= MAX_PROGRAM_BYTES);
    for (path, contents) in &candidate.entries {
        assert!(contents.len() <= MAX_PROGRAM_ENTRY_BYTES, "{path}");
        assert!(
            contents.capacity() <= 2 * MAX_PROGRAM_ENTRY_BYTES + replacement.len(),
            "{path} allocated {} bytes",
            contents.capacity()
        );
    }
    assert_eq!(candidate.entries[0].1.len(), MAX_PROGRAM_ENTRY_BYTES);
    assert!(candidate.entries[0].1.iter().all(|byte| *byte == b'x'));
}

/// A long search needle would make `starts_with` compare up to its whole
/// length at every entry position, so the scan budget charges entry length
/// times needle length. A 4 MiB filler with a ~30 KB needle exceeds it and
/// the substitution does nothing, leaving the entry untouched.
#[test]
fn long_search_needles_do_not_make_substitution_scan_the_whole_timeout() {
    let mut needle = vec![b'#'; 30_000];
    needle.push(b'x');
    let program = encode(
        "empty",
        &[
            Op::Filler(1, 3),
            Op::Substitute(0, Name::Raw(&needle), Name::Raw(b"x")),
        ],
    );
    let candidate = decode(&program);
    let filler = &candidate.entries[0].1;
    assert_eq!(filler.len(), MAX_PROGRAM_ENTRY_BYTES - 1);
    assert!(
        filler.iter().all(|byte| *byte == b'#'),
        "needle budget skipped"
    );
}

#[test]
fn documented_paths_follow_the_package_rules() {
    for valid in ["a", "a/b.xsd", "λ.json", "a b/c", &"p".repeat(256)] {
        assert!(documented_path(valid), "{valid:?}");
    }
    for invalid in [
        "",
        "/a",
        "a/",
        "a//b",
        ".",
        "./a",
        "a/..",
        "a\\b",
        "C:a",
        "a.",
        "a ",
        "a\u{7f}",
        "a\u{85}",
        &"p".repeat(257),
    ] {
        assert!(!documented_path(invalid), "{invalid:?}");
    }
}

/// The exemptions start one past their documented limits. They measure
/// `<entry>` nesting inside the design's components only, and schema
/// components by local name in any namespace outside annotations.
#[test]
fn known_limit_exemptions_start_past_their_limits() {
    let package = |entries: Vec<(&str, String)>| {
        MfdPackage::new(
            "design.mfd",
            entries.iter().map(|(path, text)| (*path, text.as_bytes())),
        )
        .unwrap_or_else(|error| panic!("test package must be admitted: {error:?}"))
    };
    let nest = |depth: usize| {
        format!(
            "{}<n/>{}",
            "<entry>".repeat(depth),
            "</entry>".repeat(depth)
        )
    };
    let mapping = |depth: usize| {
        format!(
            "<mapping><resources>{}</resources><component><data><root>{}</root></data></component></mapping>",
            nest(4096),
            nest(depth)
        )
    };
    // Counted: unprefixed and foreign-prefix particles. Not counted: the
    // root, annotation subtrees and facets.
    let schema = |components: usize| {
        format!(
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:f=\"urn:foreign\">\
             <xs:annotation><xs:appinfo>{}</xs:appinfo></xs:annotation>\
             <xs:simpleType name=\"s\"><xs:restriction base=\"xs:string\">{}</xs:restriction></xs:simpleType>\
             {}{}</xs:schema>",
            "<xs:element/>".repeat(4096),
            "<xs:enumeration value=\"v\"/>".repeat(64),
            "<element/>".repeat((components - 2) / 2),
            "<f:element/>".repeat(components - 2 - (components - 2) / 2)
        )
    };
    let half = MAX_EXEMPT_XSD_ELEMENTS / 2;
    let at_limits = package(vec![
        ("design.mfd", mapping(MAX_EXEMPT_ENTRY_DEPTH)),
        ("a.xsd", schema(half)),
        ("b.xsd", schema(MAX_EXEMPT_XSD_ELEMENTS - half)),
        ("other.mfd", mapping(4096)),
    ]);
    assert_eq!(known_limit_exemption(&at_limits), None);

    let deep = package(vec![("design.mfd", mapping(MAX_EXEMPT_ENTRY_DEPTH + 1))]);
    assert_eq!(
        known_limit_exemption(&deep),
        Some(KnownLimit::EntryTree {
            depth: MAX_EXEMPT_ENTRY_DEPTH + 1
        })
    );
    let large = package(vec![
        ("design.mfd", mapping(1)),
        ("a.xsd", schema(half)),
        ("b.xsd", schema(MAX_EXEMPT_XSD_ELEMENTS - half + 1)),
    ]);
    assert_eq!(
        known_limit_exemption(&large),
        Some(KnownLimit::Schemas {
            elements: MAX_EXEMPT_XSD_ELEMENTS + 1
        })
    );
    // A design the importer cannot read as UTF-8 is not measured.
    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend(
        mapping(MAX_EXEMPT_ENTRY_DEPTH + 1)
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    let unreadable = MfdPackage::new("design.mfd", [("design.mfd", utf16.as_slice())])
        .unwrap_or_else(|error| panic!("test package must be admitted: {error:?}"));
    assert_eq!(known_limit_exemption(&unreadable), None);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn sandbox_detects_host_filesystem_and_network_calls() {
    with_large_stack(|| {
        let ((), quiet) = sandbox::monitor(|| {
            let _ = std::hint::black_box(vec![0u8; 1 << 20]);
        });
        assert!(quiet.none(), "{quiet:?}");

        let (metadata, stat) = sandbox::monitor(|| std::fs::metadata("/"));
        assert!(metadata.is_ok(), "trapped calls still run: {metadata:?}");
        assert!(!stat.none(), "stat of / was not detected");

        let (read, open) = sandbox::monitor(|| std::fs::read("/__ferrule_package__/design.mfd"));
        assert!(read.is_err());
        assert!(!open.none(), "open was not detected");

        let (connect, network) = sandbox::monitor(|| std::net::TcpStream::connect("127.0.0.1:9"));
        assert!(connect.is_err());
        assert!(!network.none(), "connect was not detected");
        assert_eq!(network.first, Some(number("socket")));

        let (spawned, child) = sandbox::monitor(|| {
            std::thread::spawn(|| std::fs::metadata("/").is_ok())
                .join()
                .unwrap_or(false)
        });
        assert!(spawned);
        assert!(!child.none(), "a spawned thread's stat was not detected");

        // glibc's allocator probe is not an import's access; a neighbor is.
        let probe =
            std::str::from_utf8(&sandbox::ALLOCATOR_PROBE[..sandbox::ALLOCATOR_PROBE.len() - 1])
                .unwrap_or_default();
        let (_, allocator) = sandbox::monitor(|| std::fs::File::open(probe));
        assert!(allocator.none(), "{allocator:?}");
        let (_, ratio) = sandbox::monitor(|| std::fs::File::open("/proc/sys/vm/overcommit_ratio"));
        assert!(!ratio.none(), "a neighboring /proc read was not detected");

        // Read-only path, directory-watch and handle calls run and count.
        let (cwd, getcwd) = sandbox::monitor(std::env::current_dir);
        assert!(cwd.is_ok(), "getcwd still runs: {cwd:?}");
        assert_eq!(getcwd.first, Some(number("getcwd")));
        let (_, xattr) = sandbox::monitor(|| {
            let mut value = [0u8; 16];
            // SAFETY: valid NUL-terminated strings and a writable buffer.
            unsafe {
                libc::getxattr(
                    c"/".as_ptr(),
                    c"user.ferrule".as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            }
        });
        assert_eq!(xattr.first, Some(number("getxattr")));
        // SAFETY: plain descriptor calls on a descriptor this test owns.
        let inotify = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        assert!(inotify >= 0, "inotify_init1 failed");
        let (_, watch) = sandbox::monitor(|| {
            // SAFETY: a valid descriptor and NUL-terminated path.
            unsafe { libc::inotify_add_watch(inotify, c"/".as_ptr(), libc::IN_ACCESS) }
        });
        // SAFETY: closes the descriptor opened above.
        unsafe { libc::close(inotify) };
        assert_eq!(watch.first, Some(number("inotify_add_watch")));

        // Changes and io_uring fail with EPERM and count.
        let scratch =
            std::env::temp_dir().join(format!("mfd-package-sandbox-{}", std::process::id()));
        let kept = scratch.join("kept");
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(std::fs::create_dir_all(&scratch).is_ok());
        assert!(std::fs::write(&kept, b"kept").is_ok());
        let (made, mkdir) = sandbox::monitor(|| std::fs::create_dir(scratch.join("made")));
        assert_eq!(
            made.map_err(|error| error.kind()),
            Err(std::io::ErrorKind::PermissionDenied)
        );
        assert!(!mkdir.none() && !scratch.join("made").exists(), "{mkdir:?}");
        let (removed, unlink) = sandbox::monitor(|| std::fs::remove_file(&kept));
        assert!(
            removed.is_err() && !unlink.none() && kept.exists(),
            "{unlink:?}"
        );
        let (renamed, rename) = sandbox::monitor(|| std::fs::rename(&kept, scratch.join("moved")));
        assert!(
            renamed.is_err() && !rename.none() && kept.exists(),
            "{rename:?}"
        );
        let (permissions, chmod) = sandbox::monitor(|| {
            std::fs::set_permissions(&kept, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        });
        assert!(permissions.is_err() && !chmod.none(), "{chmod:?}");
        let (truncated, truncate) = sandbox::monitor(|| {
            let path =
                std::ffi::CString::new(kept.as_os_str().as_encoded_bytes()).unwrap_or_default();
            // SAFETY: a NUL-terminated path.
            unsafe { libc::truncate(path.as_ptr(), 0) }
        });
        assert!(truncated != 0 && !truncate.none(), "{truncate:?}");
        assert_eq!(
            std::fs::read(&kept).ok().as_deref(),
            Some(b"kept".as_slice())
        );
        // Spawning a process is refused with EPERM before any child exists,
        // through both the posix_spawn path and, with a pre_exec hook, the
        // fork+exec path. The refusal is a seccomp errno, not a trap, so it
        // is not counted and leaves no child and no core dump (this test
        // aborting would be that core dump). It holds even though posix_spawn
        // blocks SIGSYS around its clone.
        let (spawned, spawn) =
            sandbox::monitor(|| std::process::Command::new("/bin/true").status());
        assert!(
            matches!(
                spawned.map_err(|error| error.kind()),
                Err(std::io::ErrorKind::PermissionDenied)
            ),
            "posix_spawn was not refused"
        );
        assert!(
            spawn.none(),
            "a refused spawn should not be counted: {spawn:?}"
        );
        let (forked, fork) = sandbox::monitor(|| {
            use std::os::unix::process::CommandExt;
            let mut command = std::process::Command::new("/bin/true");
            // SAFETY: the hook only returns Ok; it forces the fork+exec path.
            unsafe { command.pre_exec(|| Ok(())) };
            command.status()
        });
        assert!(
            matches!(
                forked.map_err(|error| error.kind()),
                Err(std::io::ErrorKind::PermissionDenied)
            ),
            "fork+exec was not refused"
        );
        assert!(
            fork.none(),
            "a refused fork should not be counted: {fork:?}"
        );
        // Thread creation (clone with CLONE_THREAD) still works under the
        // filter, so the importer and libFuzzer keep creating threads.
        let (joined, thread) = sandbox::monitor(|| std::thread::spawn(|| 7u8).join().ok());
        assert_eq!(joined, Some(7), "thread creation was refused");
        assert!(
            thread.none(),
            "thread creation counted as host access: {thread:?}"
        );
        let (uring, setup) = sandbox::monitor(|| {
            let mut params = [0u8; 120];
            // SAFETY: denied before it reaches the kernel; a writable buffer.
            unsafe { libc::syscall(libc::SYS_io_uring_setup, 1, params.as_mut_ptr()) }
        });
        assert!(uring < 0, "io_uring_setup ran");
        assert_eq!(setup.first, Some(number("io_uring_setup")));
        assert!(std::fs::remove_dir_all(&scratch).is_ok());

        // Every x32-ABI number is trapped, counted and refused.
        let (x32, x32_access) = sandbox::monitor(|| {
            // SAFETY: getpid takes no arguments; the call is refused.
            unsafe { libc::syscall(sandbox::X32_SYSCALL_BIT | libc::SYS_getpid) }
        });
        assert!(x32 < 0, "an x32 call ran");
        assert_eq!(
            x32_access.first.map(sandbox::syscall_name),
            Some("x32 system call")
        );

        // Outside `monitor`, a thread with the filter does ordinary I/O.
        let manifest = std::fs::read(manifest_dir().join("Cargo.toml"));
        assert!(manifest.is_ok());
        let ((), after) = sandbox::monitor(|| {});
        assert!(after.none(), "{after:?}");
    });
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn number(name: &str) -> i64 {
    sandbox::TRAPPED
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map_or(-1, |(number, _)| *number)
}

/// A spawned thread's call during one monitor is charged to that monitor
/// alone, even while other tests import in parallel.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn foreign_calls_belong_to_their_monitor() {
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            std::thread::spawn(move || {
                for _ in 0..50 {
                    if worker == 0 {
                        let ((), access) = sandbox::monitor(|| {
                            let _ = std::thread::spawn(|| std::fs::metadata("/")).join();
                        });
                        assert!(!access.none(), "the spawned thread's stat was not charged");
                    } else {
                        let ((), quiet) = sandbox::monitor(|| {
                            std::hint::black_box(vec![0u8; 4096]);
                        });
                        assert!(quiet.none(), "charged a foreign call: {quiet:?}");
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        if let Err(panic) = worker.join() {
            std::panic::resume_unwind(panic);
        }
    }
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths
}
