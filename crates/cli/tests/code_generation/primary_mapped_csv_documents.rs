//! Generic host acquisition + constructed strict CSV output, not MFD/reference parity.
use super::*;
use codegen::{CompatibilityManifest, GenerationProfile, PrimaryMappedCsvDocumentPolicy};
use std::cell::RefCell;

#[path = "primary_mapped_csv_documents/fixture.rs"]
mod fixture;

#[derive(Debug, Clone, Copy)]
struct HostFailure;
impl std::fmt::Display for HostFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("authored host failure")
    }
}
impl std::error::Error for HostFailure {}

struct RecordingHost {
    events: RefCell<Vec<String>>,
    bytes: Vec<u8>,
    requests: Vec<String>,
    resolved: Option<String>,
    selection_failure: bool,
    load_failure: bool,
}
impl RecordingHost {
    fn new(bytes: &[u8]) -> Self {
        Self {
            events: RefCell::new(vec![]),
            bytes: bytes.to_vec(),
            requests: vec!["catalog.csv".into()],
            resolved: Some("/resolved/catalog.csv".into()),
            selection_failure: false,
            load_failure: false,
        }
    }
}
impl document_source::PrimarySourceHost for RecordingHost {
    type Error = HostFailure;
    fn select(
        &self,
        filename: &str,
        selection: mapping::PrimaryFileSelection,
    ) -> Result<Vec<String>, Self::Error> {
        assert!(selection.is_direct());
        self.events
            .borrow_mut()
            .push(format!("select:{filename}:direct"));
        if self.selection_failure {
            Err(HostFailure)
        } else {
            Ok(self.requests.clone())
        }
    }
    fn load(&self, request: &str) -> Result<document_source::SourceData, Self::Error> {
        self.events.borrow_mut().push(format!("load:{request}"));
        if self.load_failure {
            Err(HostFailure)
        } else {
            Ok(document_source::SourceData {
                bytes: self.bytes.clone(),
                resolved_path: self.resolved.clone(),
            })
        }
    }
}

fn endpoint_policy(
    document: &mapping::DocumentProject,
) -> TestResult<PrimaryMappedCsvDocumentPolicy> {
    Ok(PrimaryMappedCsvDocumentPolicy::from_project(
        document.project(),
        fixture::ROWS,
    )?)
}
fn checked_manifest(
    artifacts: &codegen::ArtifactSet,
    library: codegen::LibraryIdentity,
    expected_runtime_package: &str,
    expected_binding: codegen::RuntimeBinding,
) -> TestResult<CompatibilityManifest> {
    let record = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == codegen::MANIFEST_PATH)
        .ok_or("included manifest")?;
    let manifest = CompatibilityManifest::from_json(std::str::from_utf8(&record.contents)?)?;
    assert_eq!(
        manifest.profile,
        GenerationProfile::PrimaryMappedCsvDocument
    );
    assert_eq!(manifest.library, library);
    if manifest.runtime.package != expected_runtime_package {
        return Err("manifest runtime package differs from the requested language/runtime".into());
    }
    if manifest.runtime.binding != expected_binding {
        return Err("manifest runtime binding differs from the requested options".into());
    }
    // Select the identity algorithm from the caller's expected language, never
    // from an unchecked field of the manifest being tested.
    match library {
        codegen::LibraryIdentity::Rust { .. } => {
            if expected_runtime_package != "codegen-runtime"
                || !matches!(expected_binding, codegen::RuntimeBinding::Path { .. })
            {
                return Err("authored Rust consumers require the configured runtime path".into());
            }
            assert_eq!(
                manifest.runtime.identity.as_str(),
                codegen_runtime::RUNTIME_IDENTITY
            );
        }
        codegen::LibraryIdentity::CSharp { .. } => {
            if expected_runtime_package != "Ferrule.Runtime"
                || expected_binding != codegen::RuntimeBinding::Vendored
            {
                return Err("authored C# consumers require the vendored runtime".into());
            }
            let identity = codegen::RuntimeIdentity::of_sources(
                artifacts
                    .files()
                    .iter()
                    .filter(|file| {
                        file.path.as_str().starts_with("Runtime/")
                            && file.path.as_str() != "Runtime/FerruleRuntimeIdentity.cs"
                    })
                    .map(|file| (file.path.as_str(), file.contents.as_slice())),
            );
            assert_eq!(manifest.runtime.identity, identity);
        }
    }
    Ok(manifest)
}
fn requested_path_binding(options: &codegen_rust::Options) -> codegen::RuntimeBinding {
    let codegen_rust::RuntimeDependency::Path(path) = &options.runtime_dependency else {
        panic!("authored Rust host options must select an explicit local runtime path")
    };
    codegen::RuntimeBinding::Path { path: path.clone() }
}
fn artifact_bytes(artifacts: &codegen::ArtifactSet) -> BTreeMap<&str, &[u8]> {
    artifacts
        .files()
        .iter()
        .map(|file| (file.path.as_str(), file.contents.as_slice()))
        .collect()
}
fn write_files(root: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    for file in artifacts.files() {
        let path = root.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().ok_or("artifact parent")?)?;
        std::fs::write(path, &file.contents)?;
    }
    Ok(())
}

#[test]
fn primary_mapped_csv_documents_native_contract() -> TestResult<()> {
    let records = fixture::records();
    for literal in [false, true] {
        let kind = if literal { "literal" } else { "parameter" };
        let document = fixture::document(literal);
        let policy = endpoint_policy(&document)?;
        let lowered = codegen::lower_document(&document)?;
        codegen::validate_primary_mapped_csv_documents(&lowered, &policy)?;
        let issues = engine::validate_primary_document(&document);
        assert!(issues.is_empty(), "{issues:?}");
        let schema = policy.output.row_schema(&document.project().target)?;
        let options = format_csv::CsvWriteOptions::from(&document.project().target_options);
        for empty in [false, true] {
            let input = &records[&format!("{kind}-{}", if empty { "empty" } else { "input" })];
            for text in [false, true] {
                let host = RecordingHost::new(input.as_bytes());
                let mut parameters = engine::RuntimeParameters::new();
                parameters.insert(
                    "InputName",
                    Value::String(records["parameter-name"].clone()),
                )?;
                parameters.insert("Ending", Value::String("!".into()))?;
                let context = engine::ExecutionContext::new(Path::new("authored-map.ferrule"))
                    .with_parameters(&parameters)
                    .with_current_datetime(fixture::CLOCK);
                let output = engine::run_primary_csv_document(
                    &document,
                    &context,
                    &host,
                    format_csv::CsvByteDecodingPolicy::StrictUtf8,
                    1,
                )?;
                let actual: Vec<(String, Vec<u8>)> = if text {
                    format_csv::to_mapped_document_text(&schema, &output, fixture::ROWS, &options)?
                        .into_iter()
                        .map(|member| (member.path, member.text.into_bytes()))
                        .collect()
                } else {
                    format_csv::to_mapped_document_bytes(&schema, &output, fixture::ROWS, &options)?
                        .into_iter()
                        .map(|member| (member.path, member.bytes))
                        .collect()
                };
                let expected = if empty {
                    vec![]
                } else {
                    vec![
                        (
                            records["east-path"].clone(),
                            records[&format!("{kind}-east")].as_bytes().to_vec(),
                        ),
                        (
                            records["west-path"].clone(),
                            records[&format!("{kind}-west")].as_bytes().to_vec(),
                        ),
                    ]
                };
                assert_eq!(actual, expected, "{kind}/{text}/{empty}");
                let filename = &records[&format!("{kind}-name")];
                assert_eq!(
                    *host.events.borrow(),
                    [
                        format!("select:{filename}:direct"),
                        "load:catalog.csv".into()
                    ]
                );
                assert_eq!(host.bytes.as_slice(), input.as_bytes());
            }
        }
        // Separate parsing creates fresh origin identities even for equal paths/bytes.
        let mut parser = engine::primary_csv_parser(
            document.project(),
            format_csv::CsvByteDecodingPolicy::StrictUtf8,
        );
        let data = || document_source::SourceData {
            bytes: records[&format!("{kind}-input")].as_bytes().to_vec(),
            resolved_path: Some(records["resolved"].clone()),
        };
        let first = parser("catalog.csv", data())?;
        let second = parser("catalog.csv", data())?;
        assert_eq!(first, second); // Logical equality is independent of fresh hidden origins.
        let Instance::Repeated(first_rows) = &first else {
            return Err("parsed rows".into());
        };
        let Instance::Repeated(second_rows) = &second else {
            return Err("parsed rows".into());
        };
        let Instance::Group(first_row) = &first_rows[0] else {
            return Err("parsed row group".into());
        };
        let Instance::Group(second_row) = &second_rows[0] else {
            return Err("parsed row group".into());
        };
        let first_origin = first_row.source_origin().ok_or("source row origin")?;
        let second_origin = second_row.source_origin().ok_or("source row origin")?;
        assert_eq!(first_origin.position(), 1);
        assert_eq!(first_origin.document().path(), records["request"]);
        assert_eq!(first_origin.document().source_path(), records["resolved"]);
        assert!(
            !first_origin
                .document()
                .same_document(second_origin.document())
        );
        assert!(!first_origin.same_occurrence(second_origin));
    }
    // Neutral direct-cardinality and acquisition failures are exercised without mapping.
    let document = fixture::document(false);
    for count in [0, 2] {
        let mut host = RecordingHost::new(records["parameter-input"].as_bytes());
        host.requests = vec!["catalog.csv".into(); count];
        let result = document_source::acquire_primary_documents(
            || Ok::<_, HostFailure>("bound.csv".into()),
            mapping::PrimaryFileSelection::Direct,
            &host,
            1,
            engine::primary_csv_parser(
                document.project(),
                format_csv::CsvByteDecodingPolicy::StrictUtf8,
            ),
        );
        assert!(
            matches!(result, Err(document_source::PrimaryAcquisitionError::DirectSelection { found }) if found == count)
        );
        assert_eq!(*host.events.borrow(), ["select:bound.csv:direct"]);
    }
    Ok(())
}

#[test]
fn primary_mapped_csv_documents_admission_and_artifacts() -> TestResult<()> {
    for literal in [false, true] {
        let document = fixture::document(literal);
        let policy = endpoint_policy(&document)?;
        let lowered = codegen::lower_document(&document)?;
        let options = codegen_rust::Options {
            package_name: "authored-primary-packets".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .to_string_lossy()
                    .into(),
            ),
        };
        let free =
            codegen_rust::emit_with_primary_mapped_csv_documents(&lowered, &options, &policy)?;
        let configured = codegen_rust::Generator::new(&options)
            .emit_with_primary_mapped_csv_documents(&lowered, &policy)?;
        assert_eq!(artifact_bytes(&free), artifact_bytes(&configured));
        let free = codegen_csharp::emit_with_primary_mapped_csv_documents(&lowered, &policy)?;
        let default_options = codegen_csharp::Options::default();
        let configured = codegen_csharp::Generator::new(&default_options)
            .emit_with_primary_mapped_csv_documents(&lowered, &policy)?;
        assert_eq!(artifact_bytes(&free), artifact_bytes(&configured));
        let rust = codegen_rust::Generator::new(&options)
            .with_manifest(codegen::ManifestSelection::Included)
            .emit_with_primary_mapped_csv_documents(&lowered, &policy)?;
        checked_manifest(
            &rust,
            codegen::LibraryIdentity::Rust {
                package_name: options.package_name.clone(),
                crate_name: "authored_primary_packets".into(),
            },
            "codegen-runtime",
            requested_path_binding(&options),
        )?;
        let rust_library = codegen::LibraryIdentity::Rust {
            package_name: options.package_name.clone(),
            crate_name: "authored_primary_packets".into(),
        };
        for wrong_binding in [
            codegen::RuntimeBinding::Vendored,
            codegen::RuntimeBinding::Path {
                path: "different-runtime-path".into(),
            },
            codegen::RuntimeBinding::Version {
                version: "0.0.0".into(),
            },
        ] {
            assert!(
                checked_manifest(
                    &rust,
                    rust_library.clone(),
                    "codegen-runtime",
                    wrong_binding
                )
                .is_err()
            );
        }
        assert!(
            checked_manifest(
                &rust,
                rust_library,
                "Ferrule.Runtime",
                requested_path_binding(&options),
            )
            .is_err()
        );
        let csharp_options = codegen_csharp::Options {
            identity: codegen_csharp::Identity::new(
                "Authored.Primary.Packets",
                "Authored.Primary.Packets",
                "PacketMapping",
            )?,
            manifest: codegen::ManifestSelection::Included,
        };
        let csharp = codegen_csharp::Generator::new(&csharp_options)
            .emit_with_primary_mapped_csv_documents(&lowered, &policy)?;
        checked_manifest(
            &csharp,
            codegen::LibraryIdentity::CSharp {
                assembly_name: "Authored.Primary.Packets".into(),
                root_namespace: "Authored.Primary.Packets".into(),
                mapping_class: "PacketMapping".into(),
            },
            "Ferrule.Runtime",
            codegen::RuntimeBinding::Vendored,
        )?;
        let csharp_library = codegen::LibraryIdentity::CSharp {
            assembly_name: "Authored.Primary.Packets".into(),
            root_namespace: "Authored.Primary.Packets".into(),
            mapping_class: "PacketMapping".into(),
        };
        assert!(
            checked_manifest(
                &csharp,
                csharp_library.clone(),
                "codegen-runtime",
                codegen::RuntimeBinding::Vendored,
            )
            .is_err()
        );
        assert!(
            checked_manifest(
                &csharp,
                csharp_library,
                "Ferrule.Runtime",
                codegen::RuntimeBinding::Path {
                    path: "not-vendored".into(),
                },
            )
            .is_err()
        );
        let rust_source = rust
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .ok_or("Rust mapping")?;
        let rust_source = std::str::from_utf8(&rust_source.contents)?;
        for name in [
            "execute_primary_mapped_csv_documents",
            "execute_primary_mapped_csv_documents_bytes",
        ] {
            let declaration = format!("pub fn {name}<");
            let body = rust_source
                .split(declaration.as_str())
                .nth(1)
                .ok_or("complete primary Rust API")?;
            let body = body.split("\npub ").next().ok_or("Rust API body")?;
            assert!(body.contains("acquire_primary_documents"));
            assert_eq!(
                body.matches("codegen_runtime::acquire_primary_documents(")
                    .count(),
                1
            );
            assert!(body.contains("parsed.rows().map(|row| row.value().clone())"));
            let call = if name.ends_with("_bytes") {
                "execute_mapped_csv_documents_bytes_with_context("
            } else {
                "execute_mapped_csv_documents_with_context("
            };
            assert_eq!(
                body.matches(call).count(),
                1,
                "one mapper adapter call, not an instrumented invocation counter"
            );
        }
        assert!(!rust_source.contains("pub fn execute_primary_mapped_csv_documents_with_context"));
        let companion = csharp
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.PrimaryMappedCsvDocuments.cs")
            .ok_or("C# companion")?;
        let companion = std::str::from_utf8(&companion.contents)?;
        assert!(companion.contains("namespace Authored.Primary.Packets;"));
        assert!(companion.contains("partial class PacketMapping"));
        assert!(companion.contains("AcquireDocuments"));
        for suffix in ["", "Bytes"] {
            let name = format!(" ExecutePrimaryMappedCsvDocuments{suffix}(");
            let body = companion
                .split(name.as_str())
                .nth(1)
                .ok_or("complete C# primary API")?;
            let body = body.split("\n    public ").next().ok_or("C# API body")?;
            assert_eq!(
                body.matches("FerrulePrimaryDocuments.AcquireDocuments(")
                    .count(),
                1
            );
            let call = format!("ExecuteMappedCsvDocuments{suffix}(input, execution)");
            assert_eq!(body.matches(call.as_str()).count(), 1);
        }
        // Composite validation refuses a bad input before the independently bad output.
        let mut invalid = policy.clone();
        invalid.input.delimiter = Some('\n');
        invalid.output.rows_field.clear();
        assert!(matches!(
            codegen::validate_primary_mapped_csv_documents(&lowered, &invalid),
            Err(codegen::PrimaryMappedCsvDocumentBoundaryError::Input(_))
        ));
    }
    Ok(())
}

#[test]
fn primary_mapped_csv_documents_compiled_release_hosts() -> TestResult<()> {
    let directory = TempDir::new("primary-mapped-csv-hosts")?;
    let root = &directory.0;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let csv = runtime.parent().ok_or("runtime parent")?.join("format-csv");
    for literal in [false, true] {
        let kind = if literal { "literal" } else { "parameter" };
        let variant = root.join(kind);
        std::fs::create_dir_all(&variant)?;
        let fixture_path = variant.join("cases.tsv");
        std::fs::write(
            &fixture_path,
            include_str!("primary_mapped_csv_documents/cases.tsv"),
        )?;
        let document = fixture::document(literal);
        let policy = endpoint_policy(&document)?;
        let lowered = codegen::lower_document(&document)?;
        let library_name = format!("authored-primary-packets-{kind}");
        let host_name = format!("authored-primary-host-{kind}");
        let options = codegen_rust::Options {
            package_name: library_name.clone(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_string_lossy().into(),
            ),
        };
        let rust = codegen_rust::Generator::new(&options)
            .with_manifest(codegen::ManifestSelection::Included)
            .emit_with_primary_mapped_csv_documents(&lowered, &policy)?;
        checked_manifest(
            &rust,
            codegen::LibraryIdentity::Rust {
                package_name: library_name.clone(),
                crate_name: library_name.replace('-', "_"),
            },
            "codegen-runtime",
            requested_path_binding(&options),
        )?;
        write_files(&variant.join("rust"), &rust)?;
        let host = variant.join("rust-host");
        std::fs::create_dir_all(host.join("src"))?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname={host_name:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nprimary-packet-lib={{package={library_name:?},path=\"../rust\"}}\ncodegen-runtime={{path={}}}\nformat-csv={{path={}}}\n",
                serde_json::to_string(&runtime.to_string_lossy())?,
                serde_json::to_string(&csv.to_string_lossy())?
            ),
        )?;
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("primary_mapped_csv_documents/Host.rs.txt"),
        )?;
        let native_host =
            super::mapped_csv_documents::seed_host_lock(&host, &host_name, &library_name)?;
        let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("cargo-target"));
        let mut command = Command::new("cargo");
        command
            .args([
                "run",
                "--release",
                "--quiet",
                "--locked",
                "--offline",
                "--jobs",
                "1",
                "--target",
                &native_host,
                "--",
                kind,
            ])
            .current_dir(&host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_BUILD_BUILD_DIR", &target)
            .env("RUSTFLAGS", "-D warnings")
            .env("FERRULE_PRIMARY_MAPPED_FIXTURE", &fixture_path)
            .env(
                "FERRULE_PRIMARY_MAPPED_CALLS",
                variant.join("rust-calls.txt"),
            );
        let result = super::host_policy::recorded_output_with_sources(
            &mut command,
            &variant,
            "primary-mapped-rust-release",
            &["rust".into(), "rust-host".into(), "cases.tsv".into()],
        )?;
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout).trim(),
            "primary mapped CSV cases passed"
        );
        let assembly = format!("Authored.Primary.Packets.{kind}");
        let options = codegen_csharp::Options {
            identity: codegen_csharp::Identity::new(
                &assembly,
                "Authored.Primary.Packets",
                "PacketMapping",
            )?,
            manifest: codegen::ManifestSelection::Included,
        };
        let csharp = codegen_csharp::Generator::new(&options)
            .emit_with_primary_mapped_csv_documents(&lowered, &policy)?;
        let manifest = checked_manifest(
            &csharp,
            codegen::LibraryIdentity::CSharp {
                assembly_name: assembly.clone(),
                root_namespace: "Authored.Primary.Packets".into(),
                mapping_class: "PacketMapping".into(),
            },
            "Ferrule.Runtime",
            codegen::RuntimeBinding::Vendored,
        )?;
        write_files(&variant.join("csharp"), &csharp)?;
        let host = variant.join("csharp-host");
        std::fs::create_dir_all(&host)?;
        std::fs::write(
            host.join("Host.csproj"),
            format!(
                r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><EnableTargetingPackDownload>false</EnableTargetingPackDownload><EnableRuntimePackDownload>false</EnableRuntimePackDownload></PropertyGroup><ItemGroup><ProjectReference Include="../csharp/{assembly}.csproj" /></ItemGroup></Project>"#
            ),
        )?;
        std::fs::write(
            host.join("Program.cs"),
            include_str!("primary_mapped_csv_documents/Host.cs.txt"),
        )?;
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
            variant.join("global.json"),
        )?;
        std::fs::write(
            variant.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>",
        )?;
        let mut restore = dotnet_command(root);
        restore
            .args([
                "restore",
                "--disable-parallel",
                "csharp-host/Host.csproj",
                "--locked-mode",
                "--configfile",
                "NuGet.Config",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                "-p:BuildInParallel=false",
                "-p:UseSharedCompilation=false",
            ])
            .current_dir(&variant);
        let result = super::host_policy::recorded_output_with_sources(
            &mut restore,
            &variant,
            "primary-mapped-csharp-restore",
            &[
                "csharp".into(),
                "csharp-host".into(),
                "cases.tsv".into(),
                "global.json".into(),
                "NuGet.Config".into(),
            ],
        )?;
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let mut command = dotnet_command(root);
        command
            .args([
                "run",
                "--no-restore",
                "--project",
                "csharp-host/Host.csproj",
                "--configuration",
                "Release",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                "-p:BuildInParallel=false",
                "-p:UseSharedCompilation=false",
                "--",
                kind,
            ])
            .current_dir(&variant)
            .env("FERRULE_PRIMARY_MAPPED_FIXTURE", &fixture_path)
            .env(
                "FERRULE_PRIMARY_MAPPED_CALLS",
                variant.join("csharp-calls.txt"),
            )
            .env(
                "FERRULE_PRIMARY_MAPPED_RUNTIME_ID",
                manifest.runtime.identity.as_str(),
            );
        let result = super::host_policy::recorded_output_with_sources(
            &mut command,
            &variant,
            "primary-mapped-csharp-release",
            &[
                "csharp".into(),
                "csharp-host".into(),
                "cases.tsv".into(),
                "global.json".into(),
                "NuGet.Config".into(),
            ],
        )?;
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout).trim(),
            "primary mapped CSV cases passed"
        );
        for language in ["rust", "csharp"] {
            let calls = std::fs::read_to_string(variant.join(format!("{language}-calls.txt")))?;
            assert!(
                calls
                    .lines()
                    .any(|line| line.starts_with("success|bytes|ok|2"))
            );
            assert!(
                calls
                    .lines()
                    .any(|line| line.starts_with("success|text|ok|2"))
            );
            assert!(
                calls
                    .lines()
                    .any(|line| line.starts_with("late-map|bytes|error|2"))
            );
            assert!(calls.lines().all(|line| line.split('|').count() == 4));
            let records = fixture::records();
            let mut cases = records["common-cases"].split(',').collect::<Vec<_>>();
            if !literal {
                cases.extend(records["parameter-cases"].split(','));
            }
            let expected = cases
                .iter()
                .flat_map(|case| [format!("{case}|bytes"), format!("{case}|text")])
                .collect::<std::collections::BTreeSet<_>>();
            let actual = calls
                .lines()
                .map(|line| line.split('|').take(2).collect::<Vec<_>>().join("|"))
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                actual, expected,
                "exact {language}/{kind} public-call membership"
            );
            assert_eq!(
                calls.lines().count(),
                expected.len(),
                "no duplicate call receipts"
            );
        }
    }
    Ok(())
}
