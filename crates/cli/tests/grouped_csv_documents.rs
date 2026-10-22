use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule-grouped-cli-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_none() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn import(source: &Path, output: &Path) -> anyhow::Result<Vec<String>> {
    cli::import_mfd_grouped_csv(source, output, None, None, &[], &[])
}

#[test]
fn grouped_import_retains_document_kind_and_generates_both_boundaries() {
    let scratch = Scratch::new();
    let source = scratch.0.join("mapping.mfd");
    let document = scratch.0.join("mapping.json");
    std::fs::write(
        &source,
        include_str!("../../../conformance/grouped-csv-document-import/basic.mfd"),
    )
    .unwrap();
    assert!(import(&source, &document).unwrap().is_empty());
    let restored =
        mapping::document_project_file::decode_bytes(&std::fs::read(&document).unwrap()).unwrap();
    assert!(matches!(
        restored.global_filename().unwrap(),
        mapping::GlobalFilename::Parameter("InputFile")
    ));
    assert_eq!(
        restored.project().root.construction,
        mapping::ScopeConstruction::DocumentRows
    );
    assert!(cli::load_project(&document).is_err());
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    let rust = scratch.0.join("rust");
    let csharp = scratch.0.join("csharp");
    cli::generate_document_with_csv_document_set_adapters(
        &document,
        &rust,
        cli::GenerateTarget::Rust {
            runtime_path: runtime,
        },
        true,
    )
    .unwrap();
    cli::generate_document_with_csv_document_set_adapters(
        &document,
        &csharp,
        cli::GenerateTarget::CSharp,
        true,
    )
    .unwrap();
    let rs = std::fs::read_to_string(rust.join("src/lib.rs")).unwrap();
    assert!(rs.contains("evaluate_primary_filename"));
    assert!(rs.contains("visit_target_documents_with_filename_clock"));
    assert!(rs.contains("parse_csv_bytes_with_decoding_policy"));
    let cs = std::fs::read_to_string(csharp.join("GeneratedMapping.cs")).unwrap();
    assert!(cs.contains("EvaluatePrimaryFilename"));
    assert!(cs.contains("VisitTargetDocumentsWithFilenameClock"));
    assert!(csharp.join("Runtime/FerruleCsvInput.cs").is_file());
    let prior = std::fs::read(csharp.join("GeneratedMapping.cs")).unwrap();
    assert!(
        cli::generate_document_with_csv_document_set_adapters(
            &document,
            &csharp,
            cli::GenerateTarget::CSharp,
            true
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(csharp.join("GeneratedMapping.cs")).unwrap(),
        prior
    );
}

#[test]
fn explicit_raw_policy_is_checked_before_reading_or_publication() {
    let scratch = Scratch::new();
    let output = scratch.0.join("out");
    let error = cli::generate_document_with_csv_document_set_adapters(
        &scratch.0.join("missing.json"),
        &output,
        cli::GenerateTarget::CSharp,
        false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("explicit raw-delimited CRLF"));
    assert!(!output.exists());
}

#[test]
fn rejected_import_does_not_overwrite_a_saved_document() {
    let scratch = Scratch::new();
    let source = scratch.0.join("mapping.mfd");
    let document = scratch.0.join("mapping.json");
    let bad = include_str!("../../../conformance/grouped-csv-document-import/basic.mfd")
        .replace("name=\"auto-number\"", "name=\"unknown-counter\"");
    std::fs::write(&source, bad).unwrap();
    std::fs::write(&document, b"keep").unwrap();
    assert!(import(&source, &document).is_err());
    assert_eq!(std::fs::read(&document).unwrap(), b"keep");
}

#[test]
fn quoted_targets_do_not_enter_the_raw_document_profile() {
    let scratch = Scratch::new();
    let source = scratch.0.join("mapping.mfd");
    let document = scratch.0.join("mapping.json");
    let bad = include_str!("../../../conformance/grouped-csv-document-import/basic.mfd")
        .replace("quote=\"\"", "quote=\"&quot;\"");
    std::fs::write(&source, bad).unwrap();
    assert!(import(&source, &document).is_err());
    assert!(!document.exists());
}

#[test]
fn cli_import_and_generate_use_the_retained_document_route() {
    let scratch = Scratch::new();
    let source = scratch.0.join("mapping.mfd");
    let document = scratch.0.join("mapping.json");
    std::fs::write(
        &source,
        include_str!("../../../conformance/grouped-csv-document-import/basic.mfd"),
    )
    .unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_ferrule"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "stdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&[
        "import-mfd".as_ref(),
        "--mfd".as_ref(),
        source.as_os_str(),
        "--out".as_ref(),
        document.as_os_str(),
        "--grouped-csv-documents".as_ref(),
    ]);
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    for language in ["rust", "csharp"] {
        let out = scratch.0.join(language);
        let mut args = vec![
            "generate".as_ref(),
            "--project".as_ref(),
            document.as_os_str(),
            "--language".as_ref(),
            language.as_ref(),
            "--out".as_ref(),
            out.as_os_str(),
            "--csv-document-set-adapters".as_ref(),
            "--raw-delimited-crlf".as_ref(),
        ];
        if language == "rust" {
            args.extend(["--rust-runtime-path".as_ref(), runtime.as_os_str()]);
        }
        run(&args);
        assert!(
            out.join(if language == "rust" {
                "src/lib.rs"
            } else {
                "GeneratedMapping.cs"
            })
            .is_file()
        );
    }
}

#[test]
fn altered_document_boundaries_are_refused_without_publication() {
    let scratch = Scratch::new();
    let source = scratch.0.join("mapping.mfd");
    let document = scratch.0.join("mapping.json");
    std::fs::write(
        &source,
        include_str!("../../../conformance/grouped-csv-document-import/basic.mfd"),
    )
    .unwrap();
    import(&source, &document).unwrap();
    let original =
        mapping::document_project_file::decode_bytes(&std::fs::read(&document).unwrap()).unwrap();
    for quoted in [false, true] {
        let mut project = original.project().clone();
        if quoted {
            project.target_options.csv_quote_disabled = false;
            project.target_options.csv_quote = Some('"');
        } else {
            project.source_options.tabular_kind = None;
        }
        let altered = mapping::DocumentProject::new(project, original.primary_source().clone());
        std::fs::write(
            &document,
            mapping::document_project_file::encode_pretty(&altered).unwrap(),
        )
        .unwrap();
        let output = scratch.0.join(if quoted { "quoted" } else { "implicit" });
        assert!(
            cli::generate_document_with_csv_document_set_adapters(
                &document,
                &output,
                cli::GenerateTarget::CSharp,
                true
            )
            .is_err()
        );
        assert!(!output.exists());
    }
}
