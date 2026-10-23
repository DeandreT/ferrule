//! Configurable generated identities and opt-in compatibility manifests.
//!
//! Default requests keep the artifacts of the free emitters, a configured C#
//! identity changes only the hosted names, a manifest adds exactly its own
//! files, and two distinct self-authored mappings share one C# host process
//! or one Cargo build, while an incompatible library is refused.
use super::*;
use codegen::{
    CompanionSelection, CompatibilityManifest, GenerationProfile, LibraryIdentity,
    ManifestSelection, RuntimeBinding, RuntimeIdentity,
};
use std::collections::BTreeSet;
use std::fs;

const CSHARP_ADDITIONS: [&str; 4] = [
    "GeneratedMapping.Compatibility.cs",
    "Runtime/FerruleCompatibility.cs",
    "Runtime/FerruleRuntimeIdentity.cs",
    "ferrule-manifest.json",
];
const RUST_ADDITIONS: [&str; 2] = ["ferrule-manifest.json", "src/ferrule_compatibility.rs"];
const ORDERS_INPUT: &str = r#"{"Name":"Ada","Score":8,"Enabled":true,"Danger":"unused"}"#;
const INVOICES_INPUT: &str = r#"{"Customer":"Grace","Amount":41}"#;

fn identity(assembly: &str, namespace: &str, class: &str) -> codegen_csharp::Identity {
    codegen_csharp::Identity::new(assembly, namespace, class).expect("valid test identity")
}

fn orders_identity() -> codegen_csharp::Identity {
    identity("Acme.Orders", "Acme.Orders", "OrderMapping")
}

fn csharp_options(
    identity: codegen_csharp::Identity,
    manifest: ManifestSelection,
) -> codegen_csharp::Options {
    codegen_csharp::Options { identity, manifest }
}

fn rust_options(package_name: &str, runtime: &Path) -> codegen_rust::Options {
    codegen_rust::Options {
        package_name: package_name.into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    }
}

fn workspace_runtime() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?)
}

/// A second self-authored mapping with its own source and target shape.
fn invoices_project() -> Project {
    Project {
        source: SchemaNode::group("Invoice", vec![string("Customer"), int("Amount")]),
        target: SchemaNode::group(
            "Statement",
            vec![string("Customer"), int("Total"), string("Kind")],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Customer".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Amount".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::Call {
                        function: "add".into(),
                        args: vec![2, 3],
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::String("invoice".into()),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Customer".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Total".into(),
                    node: 4,
                },
                Binding {
                    target_field: "Kind".into(),
                    node: 5,
                },
            ],
            ..Scope::default()
        },
    }
}

fn files(set: &codegen::ArtifactSet) -> BTreeMap<String, Vec<u8>> {
    set.files()
        .iter()
        .map(|file| (file.path.as_str().to_owned(), file.contents.clone()))
        .collect()
}

fn text(files: &BTreeMap<String, Vec<u8>>, path: &str) -> TestResult<String> {
    Ok(String::from_utf8(
        files
            .get(path)
            .ok_or_else(|| format!("missing {path}"))?
            .clone(),
    )?)
}

fn write_tree(root: &Path, set: &codegen::ArtifactSet) -> TestResult<()> {
    for file in set.files() {
        let path = root.join(file.path.as_str());
        fs::create_dir_all(path.parent().ok_or("artifact parent")?)?;
        fs::write(path, &file.contents)?;
    }
    Ok(())
}

/// Identifiers ending in `Mapping` outside C# comments, string and character
/// literals and member accesses: generated declarations and references to
/// them.
fn mapping_identifiers(source: &str) -> BTreeSet<String> {
    let bytes = source.as_bytes();
    let mut names = BTreeSet::new();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &bytes[index..];
        if rest.starts_with(b"//") {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if rest.starts_with(b"/*") {
            index += 2;
            while index < bytes.len() && !bytes[index..].starts_with(b"*/") {
                index += 1;
            }
            index += 2;
        } else if matches!(bytes[index], b'"' | b'\'') {
            let quote = bytes[index];
            let verbatim = index > 0 && bytes[index - 1] == b'@';
            index += 1;
            while index < bytes.len() {
                if !verbatim && bytes[index] == b'\\' {
                    index += 2;
                } else if bytes[index] == quote {
                    if verbatim && bytes.get(index + 1) == Some(&quote) {
                        index += 2;
                    } else {
                        index += 1;
                        break;
                    }
                } else {
                    index += 1;
                }
            }
        } else if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let name = &source[start..index];
            // A member of another type, such as a runtime enum value.
            let member = start > 0 && bytes[start - 1] == b'.';
            if !member && name.ends_with("Mapping") {
                names.insert(name.to_owned());
            }
        } else {
            index += 1;
        }
    }
    names
}

/// `configured` is `default` with only the hosted names changed: the project
/// file name and properties, every namespace declaration and every mapping
/// class declaration. No other generated identifier ends in `Mapping`.
fn assert_renamed_only(
    subject: &str,
    default: &BTreeMap<String, Vec<u8>>,
    configured: &BTreeMap<String, Vec<u8>>,
) -> TestResult<()> {
    let renamed = default
        .keys()
        .map(|path| {
            if path == "Ferrule.Generated.csproj" {
                "Acme.Orders.csproj".to_owned()
            } else {
                path.clone()
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        configured.keys().cloned().collect::<BTreeSet<_>>(),
        renamed,
        "{subject}: inventory"
    );
    for (path, bytes) in default {
        if path.starts_with("Runtime/") {
            assert_eq!(configured.get(path), Some(bytes), "{subject}: {path}");
            continue;
        }
        let configured_path = if path == "Ferrule.Generated.csproj" {
            "Acme.Orders.csproj"
        } else {
            path
        };
        let expected = String::from_utf8(bytes.clone())?
            .replace(
                "<RootNamespace>Ferrule.Generated</RootNamespace>",
                "<RootNamespace>Acme.Orders</RootNamespace>",
            )
            .replace(
                "<AssemblyName>Ferrule.Generated</AssemblyName>",
                "<AssemblyName>Acme.Orders</AssemblyName>",
            )
            .replace("namespace Ferrule.Generated;\n", "namespace Acme.Orders;\n")
            .replace("class GeneratedMapping\n{\n", "class OrderMapping\n{\n");
        let actual = text(configured, configured_path)?;
        assert_eq!(actual, expected, "{subject}: {path}");
        if path.ends_with(".cs") {
            let names = mapping_identifiers(&actual);
            assert!(
                names.iter().all(|name| name == "OrderMapping"),
                "{subject}: {path} declares or uses {names:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn generator_defaults_match_the_free_emitters() -> TestResult<()> {
    let runtime = Path::new("codegen-runtime");
    for project in [project(), invoices_project()] {
        let program = codegen::lower(&project)?;
        let options = rust_options("identity-default", runtime);
        let generator = codegen_rust::Generator::new(&options);
        assert_eq!(
            generator.emit(&program)?,
            codegen_rust::emit(&program, &options)?
        );
        assert_eq!(
            generator.emit_with_companions(&program, CompanionSelection::TypedOnly)?,
            codegen_rust::emit_with_companions(&program, &options, CompanionSelection::TypedOnly)?
        );
        assert_eq!(
            format!("{:?}", generator.emit_with_json5(&program)),
            format!("{:?}", codegen_rust::emit_with_json5(&program, &options))
        );
        let options = codegen_csharp::Options::default();
        let generator = codegen_csharp::Generator::new(&options);
        assert_eq!(generator.emit(&program)?, codegen_csharp::emit(&program)?);
        assert_eq!(
            generator.emit_with_companions(&program, CompanionSelection::TypedOnly)?,
            codegen_csharp::emit_with_companions(&program, CompanionSelection::TypedOnly)?
        );
        assert_eq!(
            format!("{:?}", generator.emit_with_json5(&program)),
            format!("{:?}", codegen_csharp::emit_with_json5(&program))
        );
    }
    Ok(())
}

/// Every C# emission route of the authored fixtures and the owned corpora,
/// under a configured identity, renames only the hosted names.
#[test]
fn configured_csharp_identity_renames_only_hosted_names() -> TestResult<()> {
    let directory = TempDir::new("generated_identities_csharp")?;
    let settings = cli::GenerateSettings {
        csharp_identity: Some(orders_identity()),
        ..Default::default()
    };
    let mut compared = 0;
    for (index, (title, project)) in super::csharp_composition::saved_subjects()?
        .into_iter()
        .enumerate()
    {
        let root = directory.0.join(index.to_string());
        fs::create_dir(&root)?;
        let path = root.join("project.json");
        fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
        for (name, profile) in super::csharp_composition::PROFILES {
            let default = root.join(format!("{name}-default"));
            let configured = root.join(format!("{name}-configured"));
            let default_result = cli::generate_project_with_profile(
                &path,
                &default,
                GenerateTarget::CSharp,
                profile,
            );
            let configured_result = cli::generate_project_with_settings(
                &path,
                &configured,
                GenerateTarget::CSharp,
                profile,
                &settings,
            );
            match (default_result, configured_result) {
                (Ok(_), Ok(_)) => {
                    let default = artifact_files(&default)?.into_iter().collect();
                    let configured = artifact_files(&configured)?.into_iter().collect();
                    assert_renamed_only(&format!("{title} {name}"), &default, &configured)?;
                    compared += 1;
                }
                (Err(default), Err(configured)) => {
                    assert_eq!(
                        format!("{default:#}").replace("-default", ""),
                        format!("{configured:#}").replace("-configured", ""),
                        "{title} {name}: refusal"
                    );
                }
                (default, configured) => {
                    return Err(format!(
                        "{title} {name}: default {:?} but configured {:?}",
                        default.map(drop),
                        configured.map(drop)
                    )
                    .into());
                }
            }
        }
        fs::remove_dir_all(&root)?;
    }
    let options = csharp_options(orders_identity(), ManifestSelection::Omitted);
    let generator = codegen_csharp::Generator::new(&options);
    for (_, corpus) in super::owned_capability_presets::corpora()? {
        for (id, subject) in corpus {
            let program = match subject {
                super::owned_capability_presets::Subject::Program(program) => *program,
                super::owned_capability_presets::Subject::Project(project) => {
                    match codegen::lower(&project) {
                        Ok(program) => program,
                        Err(_) => continue,
                    }
                }
            };
            for companions in [CompanionSelection::Complete, CompanionSelection::TypedOnly] {
                if let (Ok(default), Ok(configured)) = (
                    codegen_csharp::emit_with_companions(&program, companions),
                    generator.emit_with_companions(&program, companions),
                ) {
                    assert_renamed_only(&id, &files(&default), &files(&configured))?;
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 100, "only {compared} emissions were compared");
    Ok(())
}

#[test]
fn csharp_manifest_adds_its_files_and_records_the_request() -> TestResult<()> {
    let program = codegen::lower(&project())?;
    let options = csharp_options(orders_identity(), ManifestSelection::Included);
    let generator = codegen_csharp::Generator::new(&options);
    let plain = csharp_options(orders_identity(), ManifestSelection::Omitted);
    let plain = codegen_csharp::Generator::new(&plain);
    for (companions, profile) in [
        (CompanionSelection::Complete, GenerationProfile::Complete),
        (CompanionSelection::TypedOnly, GenerationProfile::TypedOnly),
    ] {
        let with = files(&generator.emit_with_companions(&program, companions)?);
        let without = files(&plain.emit_with_companions(&program, companions)?);
        let added = with
            .keys()
            .filter(|path| !without.contains_key(*path))
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(added, CSHARP_ADDITIONS);
        for (path, bytes) in &without {
            if path != "Acme.Orders.csproj" && path != "GeneratedMapping.cs" {
                assert_eq!(with.get(path), Some(bytes), "{path} is unchanged");
            }
        }
        let mapping = text(&with, "GeneratedMapping.cs")?;
        assert_eq!(
            mapping,
            text(&without, "GeneratedMapping.cs")?.replacen(
                "public static class OrderMapping\n",
                "public static partial class OrderMapping\n",
                1
            )
        );
        let project_file = text(&with, "Acme.Orders.csproj")?;
        assert!(project_file.contains(
            "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Compatibility.cs\" />\n"
        ));
        assert!(project_file.contains(
            "    <EmbeddedResource Include=\"ferrule-manifest.json\" LogicalName=\"ferrule-manifest.json\" />\n  </ItemGroup>"
        ));
        let manifest = CompatibilityManifest::from_json(&text(&with, "ferrule-manifest.json")?)?;
        assert_eq!(manifest.profile, profile);
        assert_eq!(manifest.runtime.binding, RuntimeBinding::Vendored);
        assert_eq!(
            manifest.library,
            LibraryIdentity::CSharp {
                assembly_name: "Acme.Orders".into(),
                root_namespace: "Acme.Orders".into(),
                mapping_class: "OrderMapping".into(),
            }
        );
        let runtime = RuntimeIdentity::of_sources(
            with.iter()
                .filter(|(path, _)| {
                    path.starts_with("Runtime/") && *path != "Runtime/FerruleRuntimeIdentity.cs"
                })
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
        assert_eq!(manifest.runtime.identity, runtime);
        assert!(text(&with, "Runtime/FerruleRuntimeIdentity.cs")?.contains(runtime.as_str()));
        let check = text(&with, "GeneratedMapping.Compatibility.cs")?;
        assert!(check.starts_with(
            "namespace Acme.Orders;\n\npublic static partial class OrderMapping\n{\n"
        ));
        assert!(check.contains("    static OrderMapping()\n"));
        manifest.require_runtime(&runtime)?;
    }
    let json5 = generator.emit_with_json5(&program);
    if let Ok(json5) = json5 {
        let manifest =
            CompatibilityManifest::from_json(&text(&files(&json5), "ferrule-manifest.json")?)?;
        assert_eq!(manifest.profile, GenerationProfile::Json5);
    }
    Ok(())
}

#[test]
fn rust_manifest_adds_its_files_and_records_the_binding() -> TestResult<()> {
    let program = codegen::lower(&invoices_project())?;
    let runtime = workspace_runtime()?;
    let options = rust_options("invoices-mapping", &runtime);
    let with = files(
        &codegen_rust::Generator::new(&options)
            .with_manifest(ManifestSelection::Included)
            .emit(&program)?,
    );
    let without = files(&codegen_rust::emit(&program, &options)?);
    let added = with
        .keys()
        .filter(|path| !without.contains_key(*path))
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(added, RUST_ADDITIONS);
    assert_eq!(with["Cargo.toml"], without["Cargo.toml"]);
    assert_eq!(
        text(&with, "src/lib.rs")?,
        text(&without, "src/lib.rs")?
            + "\nmod ferrule_compatibility;\npub use ferrule_compatibility::{COMPATIBILITY_MANIFEST, RUNTIME_IDENTITY};\n"
    );
    let manifest_text = text(&with, "ferrule-manifest.json")?;
    let manifest = CompatibilityManifest::from_json(&manifest_text)?;
    assert_eq!(manifest.profile, GenerationProfile::Complete);
    assert_eq!(
        manifest.runtime.identity.as_str(),
        codegen_rust::RUNTIME_IDENTITY
    );
    assert_eq!(
        manifest.runtime.binding,
        RuntimeBinding::Path {
            path: runtime.display().to_string()
        }
    );
    assert_eq!(
        manifest.library,
        LibraryIdentity::Rust {
            package_name: "invoices-mapping".into(),
            crate_name: "invoices_mapping".into(),
        }
    );
    let module = text(&with, "src/ferrule_compatibility.rs")?;
    assert!(module.contains(&format!(
        "pub const RUNTIME_IDENTITY: &str = \"{}\";",
        codegen_rust::RUNTIME_IDENTITY
    )));
    assert!(module.contains("codegen_runtime::runtime_identity_matches(RUNTIME_IDENTITY)"));
    Ok(())
}

#[test]
fn generation_settings_are_refused_before_anything_is_written() -> TestResult<()> {
    let directory = TempDir::new("generated_identities_refusals")?;
    let project = write_project(&directory.0)?;
    let runtime = workspace_runtime()?;
    let rust = GenerateTarget::Rust {
        runtime_path: runtime.clone(),
    };
    let output = directory.0.join("out");
    let package = cli::GenerateSettings {
        rust_package_name: Some(codegen_rust::PackageName::new("orders-mapping")?),
        ..Default::default()
    };
    let error = cli::generate_project_with_settings(
        &project,
        &output,
        GenerateTarget::CSharp,
        cli::GenerateProfile::Ordinary,
        &package,
    )
    .expect_err("a Rust package name is refused for C#");
    assert!(error.to_string().contains("only to the Rust backend"));
    let error = cli::generate_project_with_settings(
        &project,
        &output,
        rust.clone(),
        cli::GenerateProfile::Ordinary,
        &cli::GenerateSettings {
            csharp_identity: Some(orders_identity()),
            ..Default::default()
        },
    )
    .expect_err("a C# identity is refused for Rust");
    assert!(error.to_string().contains("only to the C# backend"));

    // A declared runtime whose sources differ from the generator's runtime.
    let changed = directory.0.join("changed-runtime");
    fs::create_dir_all(changed.join("src"))?;
    for name in ["Cargo.toml", "build.rs", "identity.rs"] {
        fs::copy(runtime.join(name), changed.join(name))?;
    }
    fs::write(changed.join("src/lib.rs"), "pub fn changed() {}\n")?;
    let manifest = cli::GenerateSettings {
        manifest: ManifestSelection::Included,
        ..package.clone()
    };
    let error = cli::generate_project_with_settings(
        &project,
        &output,
        GenerateTarget::Rust {
            runtime_path: changed,
        },
        cli::GenerateProfile::Ordinary,
        &manifest,
    )
    .expect_err("a mismatched runtime is refused");
    assert!(matches!(
        error.downcast_ref::<codegen_rust::RuntimeBindingError>(),
        Some(codegen_rust::RuntimeBindingError::Mismatch { required, .. })
            if required.as_str() == codegen_rust::RUNTIME_IDENTITY
    ));
    assert!(!output.exists());

    cli::generate_project_with_settings(
        &project,
        &output,
        rust,
        cli::GenerateProfile::Ordinary,
        &manifest,
    )?;
    let cargo = fs::read_to_string(output.join("Cargo.toml"))?;
    assert!(cargo.starts_with("[package]\nname = \"orders-mapping\"\n"));
    assert!(output.join("ferrule-manifest.json").is_file());
    Ok(())
}

/// Generates one C# library into `root/<directory>`.
fn csharp_library(
    root: &Path,
    directory: &str,
    project: &Project,
    identity: codegen_csharp::Identity,
) -> TestResult<PathBuf> {
    let program = codegen::lower(project)?;
    let options = csharp_options(identity, ManifestSelection::Included);
    let output = root.join(directory);
    write_tree(
        &output,
        &codegen_csharp::Generator::new(&options).emit(&program)?,
    )?;
    Ok(output)
}

#[test]
#[ignore = "builds and runs one C# host over two generated libraries; requires the .NET 10 SDK"]
fn distinct_csharp_mappings_share_one_host_process() -> TestResult<()> {
    let temp = TempDir::new("generated_identities_csharp_host")?;
    csharp_library(&temp.0, "orders", &project(), orders_identity())?;
    csharp_library(
        &temp.0,
        "invoices",
        &invoices_project(),
        identity("Acme.Invoices", "Acme.Invoices", "InvoiceMapping"),
    )?;
    // Libraries changed after generation: another vendored runtime identity,
    // and a manifest that differs from the one compiled into the library.
    let tampered_runtime = csharp_library(
        &temp.0,
        "tampered-runtime",
        &invoices_project(),
        identity(
            "Acme.TamperedRuntime",
            "Acme.TamperedRuntime",
            "TamperedRuntimeMapping",
        ),
    )?;
    let identity_file = tampered_runtime.join("Runtime/FerruleRuntimeIdentity.cs");
    let source = fs::read_to_string(&identity_file)?;
    let start = source.find("\"sha256:").ok_or("runtime identity literal")?;
    let replaced = format!(
        "{}\"sha256:{}\"{}",
        &source[..start],
        "0".repeat(64),
        &source[start + 73..]
    );
    fs::write(&identity_file, replaced)?;
    let tampered_manifest = csharp_library(
        &temp.0,
        "tampered-manifest",
        &invoices_project(),
        identity(
            "Acme.TamperedManifest",
            "Acme.TamperedManifest",
            "TamperedManifestMapping",
        ),
    )?;
    let manifest = tampered_manifest.join("ferrule-manifest.json");
    let text = fs::read_to_string(&manifest)?;
    fs::write(
        &manifest,
        text.replace("\"profile\": \"complete\"", "\"profile\": \"typed_only\""),
    )?;

    let host = temp.0.join("host");
    fs::create_dir(&host)?;
    fs::write(
        host.join("Host.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../orders/Acme.Orders.csproj" />
    <ProjectReference Include="../invoices/Acme.Invoices.csproj" Aliases="Invoices" />
    <ProjectReference Include="../tampered-runtime/Acme.TamperedRuntime.csproj" Aliases="TamperedRuntime" />
    <ProjectReference Include="../tampered-manifest/Acme.TamperedManifest.csproj" Aliases="TamperedManifest" />
  </ItemGroup>
</Project>
"#,
    )?;
    fs::write(
        host.join("Program.cs"),
        format!(
            r#"extern alias Invoices;
extern alias TamperedRuntime;
extern alias TamperedManifest;

using System.Text.Json.Nodes;
using Acme.Orders;
using Ferrule.Runtime;
using InvoiceMapping = Invoices::Acme.Invoices.InvoiceMapping;

// Both mappings run in this process under distinct assemblies, namespaces
// and classes; each vendors its own runtime types.
Same(OrderMapping.ExecuteJson({orders}), """{{"Copied":"Ada","Fixed":"fixed","Adjusted":13,"Bucket":"large","Enabled":true,"Lazy":"fixed","Details":{{"NestedCopied":"Ada"}}}}""");
Same(InvoiceMapping.ExecuteJson({invoices}), """{{"Customer":"Grace","Total":42,"Kind":"invoice"}}""");
Assert(typeof(OrderMapping).Assembly.GetName().Name == "Acme.Orders");
Assert(typeof(InvoiceMapping).Assembly.GetName().Name == "Acme.Invoices");
Assert(typeof(FerruleInstance) != typeof(Invoices::Ferrule.Runtime.FerruleInstance));

var order = (FerruleGroup)OrderMapping.Execute(new FerruleGroup(new FerruleField[]
{{
    new("Name", new FerruleScalar(FerruleValue.FromString("Ada"))),
    new("Score", new FerruleScalar(FerruleValue.FromInt64(8))),
    new("Enabled", new FerruleScalar(FerruleValue.FromBoolean(true))),
}}));
Assert(((FerruleScalar)order.Fields[2].Value).Value == FerruleValue.FromInt64(13));
var invoice = (Invoices::Ferrule.Runtime.FerruleGroup)InvoiceMapping.Execute(
    new Invoices::Ferrule.Runtime.FerruleGroup(new Invoices::Ferrule.Runtime.FerruleField[]
    {{
        new("Customer", new Invoices::Ferrule.Runtime.FerruleScalar(Invoices::Ferrule.Runtime.FerruleValue.FromString("Grace"))),
        new("Amount", new Invoices::Ferrule.Runtime.FerruleScalar(Invoices::Ferrule.Runtime.FerruleValue.FromInt64(41))),
    }}));
Assert(((Invoices::Ferrule.Runtime.FerruleScalar)invoice.Fields[1].Value).Value
    == Invoices::Ferrule.Runtime.FerruleValue.FromInt64(42));
Assert(OrderMapping.CompatibilityManifest.Contains("\"mapping_class\": \"OrderMapping\""));
Assert(InvoiceMapping.CompatibilityManifest.Contains("\"assembly_name\": \"Acme.Invoices\""));

Refused(() => TamperedRuntime::Acme.TamperedRuntime.TamperedRuntimeMapping.ExecuteJson({invoices}), "runtime identity");
Refused(() => TamperedManifest::Acme.TamperedManifest.TamperedManifestMapping.ExecuteJson({invoices}), "manifest");
Console.WriteLine("hosted");

static void Same(string actual, string expected) =>
    Assert(JsonNode.DeepEquals(JsonNode.Parse(actual), JsonNode.Parse(expected)), $"{{actual}} != {{expected}}");

static void Refused(Func<string> call, string field)
{{
    for (var attempt = 0; attempt < 2; attempt++)
    {{
        try
        {{
            call();
        }}
        catch (TypeInitializationException error)
        {{
            var inner = error.InnerException ?? throw new InvalidOperationException("no compatibility cause");
            Assert(inner.GetType().FullName == "Ferrule.Runtime.FerruleCompatibilityException", inner.ToString());
            Assert((string?)inner.GetType().GetProperty("Field")!.GetValue(inner) == field, inner.Message);
            continue;
        }}
        throw new InvalidOperationException($"{{field}} was not refused");
    }}
}}

static void Assert(bool condition, string message = "generated host assertion failed")
{{
    if (!condition)
    {{
        throw new InvalidOperationException(message);
    }}
}}
"#,
            orders = serde_json::to_string(ORDERS_INPUT)?,
            invoices = serde_json::to_string(INVOICES_INPUT)?,
        ),
    )?;
    let command = dotnet_command(&temp.0)
        .args([
            "run",
            "--project",
            "host/Host.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&temp.0)
        .isolated_output()?;
    assert!(
        command.status.success() && String::from_utf8_lossy(&command.stdout).contains("hosted"),
        "two-mapping C# host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    Ok(())
}

fn rust_library(
    root: &Path,
    package: &str,
    project: &Project,
    runtime: &Path,
) -> TestResult<PathBuf> {
    let program = codegen::lower(project)?;
    let options = rust_options(package, runtime);
    let output = root.join(package);
    write_tree(
        &output,
        &codegen_rust::Generator::new(&options)
            .with_manifest(ManifestSelection::Included)
            .emit(&program)?,
    )?;
    Ok(output)
}

fn cargo_build(directory: &Path, case: &Path, name: &str) -> TestResult<Output> {
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet"])
        .current_dir(directory)
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    Ok(host_policy::recorded_output(&mut command, case, name)?)
}

#[test]
#[ignore = "builds and runs one Rust host over two generated crates; requires cargo"]
fn distinct_rust_mappings_share_one_host_build() -> TestResult<()> {
    let temp = TempDir::new("generated_identities_rust_host")?;
    let runtime = workspace_runtime()?;
    rust_library(&temp.0, "orders-mapping", &project(), &runtime)?;
    rust_library(&temp.0, "invoices-mapping", &invoices_project(), &runtime)?;
    let host = temp.0.join("host");
    fs::create_dir_all(host.join("src"))?;
    fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname = \"identity-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\norders-mapping = {{ path = \"../orders-mapping\" }}\ninvoices-mapping = {{ path = \"../invoices-mapping\" }}\ncodegen-runtime = {{ path = {} }}\n\n[workspace]\n",
            serde_json::to_string(&runtime.display().to_string())?
        ),
    )?;
    fs::write(
        host.join("src/main.rs"),
        format!(
            r#"fn main() -> Result<(), Box<dyn std::error::Error>> {{
    // Both crates expose the same generated item names, scoped by crate.
    let compact = |json: String| json.split_whitespace().collect::<String>();
    let orders = compact(orders_mapping::execute_json({orders})?);
    let invoices = compact(invoices_mapping::execute_json({invoices})?);
    for field in ["\"Adjusted\":13", "\"Bucket\":\"large\"", "\"NestedCopied\":\"Ada\""] {{
        assert!(orders.contains(field), "{{orders}}");
    }}
    for field in ["\"Customer\":\"Grace\"", "\"Total\":42", "\"Kind\":\"invoice\""] {{
        assert!(invoices.contains(field), "{{invoices}}");
    }}
    let _: Option<(orders_mapping::NamedOutput, invoices_mapping::NamedOutput)> = None;
    assert_eq!(orders_mapping::RUNTIME_IDENTITY, codegen_runtime::RUNTIME_IDENTITY);
    assert_eq!(invoices_mapping::RUNTIME_IDENTITY, codegen_runtime::RUNTIME_IDENTITY);
    assert!(orders_mapping::COMPATIBILITY_MANIFEST.contains("\"package_name\": \"orders-mapping\""));
    assert!(invoices_mapping::COMPATIBILITY_MANIFEST.contains("\"crate_name\": \"invoices_mapping\""));
    println!("hosted");
    Ok(())
}}
"#,
            orders = serde_json::to_string(ORDERS_INPUT)?,
            invoices = serde_json::to_string(INVOICES_INPUT)?,
        ),
    )?;
    let output = cargo_build(&host, &temp.0, "host")?;
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("hosted"),
        "two-crate Rust host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // A library bound to a runtime of another identity, and a manifest that
    // differs from the embedded record, fail to compile.
    let module = host.join("../orders-mapping/src/ferrule_compatibility.rs");
    let original = fs::read_to_string(&module)?;
    let declaration = format!(
        "pub const RUNTIME_IDENTITY: &str = \"{}\";",
        codegen_rust::RUNTIME_IDENTITY
    );
    assert!(original.contains(&declaration));
    fs::write(
        &module,
        original.replace(
            &declaration,
            &format!(
                "pub const RUNTIME_IDENTITY: &str = \"sha256:{}\";",
                "0".repeat(64)
            ),
        ),
    )?;
    let output = cargo_build(&host, &temp.0, "other-runtime")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success()
            && stderr.contains("ferrule: this generated mapping requires codegen-runtime"),
        "another runtime identity was not refused:\n{stderr}"
    );
    fs::write(&module, original)?;
    let manifest = host.join("../invoices-mapping/ferrule-manifest.json");
    let text = fs::read_to_string(&manifest)?;
    fs::write(
        &manifest,
        text.replace("\"profile\": \"complete\"", "\"profile\": \"typed_only\""),
    )?;
    let output = cargo_build(&host, &temp.0, "other-manifest")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success()
            && stderr.contains("ferrule: ferrule-manifest.json differs from the manifest"),
        "a changed manifest was not refused:\n{stderr}"
    );
    Ok(())
}
