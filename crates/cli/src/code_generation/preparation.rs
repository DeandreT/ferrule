//! Generation preparation shared by the generating calls and their plan
//! queries. Preparation applies every refusal generation can make before it
//! writes, in generation's order: the profile's backend requirement, its
//! boundary policies, ordinary lowering, Rust runtime resolution, in-memory
//! emission and finally the destination check.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use codegen::{ArtifactSet, X12EnvelopeProfile};

use super::{GenerateSettings, GenerateTarget, csv_json, csv_x12, static_documents, x12};
use crate::route_capability::{HostRoute, PlannedPublication, RoutePlan, RouteRefusal};
use crate::{extension_for_dispatch, load_project, validate_tabular_fallback};

/// The generated companion adapters selected for one generation. File
/// suffixes and stored identities never select a profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GenerateProfile {
    /// The ordinary mapping APIs only.
    #[default]
    Ordinary,
    /// Plus a bounded flat CSV output adapter.
    CsvOutput,
    /// Complete single-document CSV input and output methods.
    CsvDocument,
    /// CSV input with unescaped CRLF text output.
    RawDelimitedDocument,
    /// Singular JSON5 text and byte companions.
    Json5,
    /// Primary raw X12 companions; an absent envelope keeps the
    /// single-transaction default.
    X12 {
        source_envelope: Option<X12EnvelopeProfile>,
        target_envelope: Option<X12EnvelopeProfile>,
    },
    /// Static JSON/X12 document adapters; C# only.
    StaticDocuments {
        source_envelope: Option<X12EnvelopeProfile>,
        target_envelope: Option<X12EnvelopeProfile>,
    },
    /// All-row CSV input and singular X12 output; C# only.
    CsvX12 {
        target_envelope: Option<X12EnvelopeProfile>,
    },
    /// Full-row CSV input and strict JSON output; C# only.
    CsvJson,
}

impl GenerateProfile {
    fn csharp_requirement(self) -> Option<&'static str> {
        match self {
            Self::StaticDocuments { .. } => {
                Some("generated static document adapters currently require the C# backend")
            }
            Self::CsvX12 { .. } => Some("generated CSV-to-X12 adapters require the C# backend"),
            Self::CsvJson => Some("generated CSV-to-JSON adapters require the C# backend"),
            _ => None,
        }
    }
}

/// The boundary policy a profile admitted before ordinary lowering.
enum ProfilePolicy {
    Ordinary,
    CsvOutput(codegen::CsvOutputPolicy),
    CsvDocument(codegen::CsvDocumentBoundaryPolicy),
    Json5,
    X12(codegen::X12BoundaryPolicy),
    StaticDocuments(codegen::StaticDocumentBoundaryPolicy),
    CsvX12(codegen::CsvX12BoundaryPolicy),
    CsvJson(codegen::CsvJsonBoundaryPolicy),
}

/// A lowered design with its admitted profile policy.
struct PreparedDesign {
    program: codegen::Program,
    policy: ProfilePolicy,
}

/// The package name of generated Rust libraries unless a request names one.
const DEFAULT_RUST_PACKAGE_NAME: &str = "ferrule-generated-mapping";

/// The backend a prepared design is emitted for.
enum Backend {
    Rust { runtime_dependency: String },
    CSharp,
}

fn require_backend(
    target: &GenerateTarget,
    profile: GenerateProfile,
    settings: &GenerateSettings,
) -> anyhow::Result<()> {
    if let Some(message) = profile.csharp_requirement()
        && *target != GenerateTarget::CSharp
    {
        bail!("{message}");
    }
    match target {
        GenerateTarget::Rust { .. } if settings.csharp_identity.is_some() => {
            bail!("a generated C# identity applies only to the C# backend")
        }
        GenerateTarget::CSharp if settings.rust_package_name.is_some() => {
            bail!("a generated Rust package name applies only to the Rust backend")
        }
        _ => Ok(()),
    }
}

fn lower(project: &mapping::Project) -> anyhow::Result<codegen::Program> {
    codegen::lower(project).map_err(|error| {
        let details = error
            .diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        anyhow::anyhow!("{error}:\n  - {details}")
    })
}

fn admit_profile(
    project: &mapping::Project,
    profile: GenerateProfile,
) -> anyhow::Result<ProfilePolicy> {
    Ok(match profile {
        GenerateProfile::Ordinary => ProfilePolicy::Ordinary,
        GenerateProfile::CsvDocument | GenerateProfile::RawDelimitedDocument => {
            for (label, path, options) in [
                (
                    "source",
                    project.source_path.as_deref(),
                    &project.source_options,
                ),
                (
                    "target",
                    project.target_path.as_deref(),
                    &project.target_options,
                ),
            ] {
                let kind = options.tabular_kind;
                let extension = path
                    .and_then(|p| Path::new(p).extension())
                    .and_then(|s| s.to_str());
                if kind != Some(mapping::TabularBoundaryKind::Csv)
                    && (kind.is_some() || !extension.is_some_and(|s| s.eq_ignore_ascii_case("csv")))
                {
                    bail!(
                        "generated CSV document {label} requires an explicit CSV format identity"
                    );
                }
            }
            let mut policy = codegen::CsvDocumentBoundaryPolicy::from_project(project)?;
            policy.target.raw_delimited_crlf = profile == GenerateProfile::RawDelimitedDocument;
            ProfilePolicy::CsvDocument(policy)
        }
        GenerateProfile::CsvJson => {
            let policy = csv_json::policy(project)?;
            codegen::prepare_csv_json_project_boundary(project, &policy)?;
            ProfilePolicy::CsvJson(policy)
        }
        GenerateProfile::CsvX12 { target_envelope } => {
            let policy = csv_x12::policy_with_envelope_profile(project, target_envelope)?;
            codegen::prepare_csv_x12_project_boundary(project, &policy)?;
            ProfilePolicy::CsvX12(policy)
        }
        GenerateProfile::StaticDocuments {
            source_envelope,
            target_envelope,
        } => {
            let policy = static_documents::policy_with_envelope_profiles(
                project,
                source_envelope,
                target_envelope,
            )?;
            codegen::prepare_static_document_project_boundaries(project, &policy)?;
            ProfilePolicy::StaticDocuments(policy)
        }
        GenerateProfile::X12 {
            source_envelope,
            target_envelope,
        } => ProfilePolicy::X12(x12::policy_with_envelope_profiles(
            project,
            source_envelope,
            target_envelope,
        )?),
        GenerateProfile::Json5 => {
            // Admission borrows the loaded schemas before ordinary lowering clones them.
            // The shared policy owns every option/schema decision and original cause.
            for (options, side) in [
                (&project.source_options, codegen::Json5BoundarySide::Source),
                (&project.target_options, codegen::Json5BoundarySide::Target),
            ] {
                codegen::validate_json5_format_options(options, side)?;
            }
            for (schema, side) in [
                (&project.source, codegen::Json5BoundarySide::Source),
                (&project.target, codegen::Json5BoundarySide::Target),
            ] {
                codegen_schema::json5_profile::validate_schema(schema)
                    .map_err(|error| codegen::Json5BoundaryPolicyError::Schema { side, error })?;
            }
            ProfilePolicy::Json5
        }
        GenerateProfile::CsvOutput => {
            let policy = codegen::CsvOutputPolicy::from_format_options(&project.target_options)?;
            if let Some(path) = &project.target_path {
                let path = Path::new(path);
                validate_tabular_fallback(path, &project.target_options, "target")?;
                let extension = extension_for_dispatch(path, &project.target_options)?;
                if !matches!(extension.as_str(), "csv" | "txt") {
                    bail!(
                        "generated CSV output requires a CSV target; stored target selects {extension:?}"
                    );
                }
            }
            ProfilePolicy::CsvOutput(policy)
        }
    })
}

fn prepare_design(
    project: &mapping::Project,
    profile: GenerateProfile,
) -> anyhow::Result<PreparedDesign> {
    let policy = admit_profile(project, profile)?;
    let program = lower(project)?;
    Ok(PreparedDesign { program, policy })
}

fn resolve_runtime_path(runtime_path: &Path) -> anyhow::Result<String> {
    let runtime_path = fs::canonicalize(runtime_path).with_context(|| {
        format!(
            "resolving Rust codegen runtime path {}",
            runtime_path.display()
        )
    })?;
    Ok(runtime_path
        .to_str()
        .context("Rust codegen runtime path must be valid UTF-8")?
        .to_owned())
}

/// Resolves the backend. A Rust runtime bound by a compatibility manifest
/// must have the runtime identity this generator emits code for.
fn resolve_backend(
    target: &GenerateTarget,
    settings: &GenerateSettings,
) -> anyhow::Result<Backend> {
    Ok(match target {
        GenerateTarget::Rust { runtime_path } => {
            let runtime_dependency = resolve_runtime_path(runtime_path)?;
            if settings.manifest == codegen::ManifestSelection::Included {
                codegen_rust::verify_runtime_path(Path::new(&runtime_dependency))?;
            }
            Backend::Rust { runtime_dependency }
        }
        GenerateTarget::CSharp => Backend::CSharp,
    })
}

fn emit(
    design: &PreparedDesign,
    backend: &Backend,
    settings: &GenerateSettings,
) -> anyhow::Result<ArtifactSet> {
    let program = &design.program;
    Ok(match backend {
        Backend::Rust { runtime_dependency } => {
            let options = codegen_rust::Options {
                package_name: settings
                    .rust_package_name
                    .as_ref()
                    .map_or(DEFAULT_RUST_PACKAGE_NAME, codegen_rust::PackageName::as_str)
                    .to_string(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    runtime_dependency.clone(),
                ),
            };
            let generator = codegen_rust::Generator::new(&options).with_manifest(settings.manifest);
            match &design.policy {
                ProfilePolicy::CsvDocument(policy) => {
                    generator.emit_with_csv_document_adapters(program, policy)?
                }
                ProfilePolicy::X12(policy) => generator.emit_with_x12(program, policy)?,
                ProfilePolicy::Json5 => generator.emit_with_json5(program)?,
                ProfilePolicy::CsvOutput(policy) => {
                    generator.emit_with_csv_output(program, policy)?
                }
                ProfilePolicy::Ordinary => generator.emit(program)?,
                ProfilePolicy::StaticDocuments(_)
                | ProfilePolicy::CsvX12(_)
                | ProfilePolicy::CsvJson(_) => {
                    // `require_backend` refuses these profiles before preparation.
                    bail!("this generated adapter profile requires the C# backend")
                }
            }
        }
        Backend::CSharp => {
            let options = codegen_csharp::Options {
                identity: settings.csharp_identity.clone().unwrap_or_default(),
                manifest: settings.manifest,
            };
            let generator = codegen_csharp::Generator::new(&options);
            match &design.policy {
                ProfilePolicy::CsvDocument(policy) => {
                    generator.emit_with_csv_document_adapters(program, policy)?
                }
                ProfilePolicy::CsvJson(policy) => {
                    generator.emit_with_csv_json_adapters(program, policy)?
                }
                ProfilePolicy::CsvX12(policy) => {
                    generator.emit_with_csv_x12_adapters(program, policy)?
                }
                ProfilePolicy::StaticDocuments(policy) => {
                    generator.emit_with_static_document_adapters(program, policy)?
                }
                ProfilePolicy::X12(policy) => generator.emit_with_x12(program, policy)?,
                ProfilePolicy::Json5 => generator.emit_with_json5(program)?,
                ProfilePolicy::CsvOutput(policy) => {
                    generator.emit_with_csv_output(program, policy)?
                }
                ProfilePolicy::Ordinary => generator.emit(program)?,
            }
        }
    })
}

/// Loads, admits, lowers and emits one generation in memory; the caller
/// publishes the artifacts.
pub(super) fn generate_artifacts(
    project_path: &Path,
    target: &GenerateTarget,
    profile: GenerateProfile,
    settings: &GenerateSettings,
) -> anyhow::Result<ArtifactSet> {
    require_backend(target, profile, settings)?;
    let project = load_project(project_path)?;
    let design = prepare_design(&project, profile)?;
    emit(&design, &resolve_backend(target, settings)?, settings)
}

/// Admits one generation of an in-memory design without reading or writing
/// anything: the profile's backend requirement and boundary policies, ordinary
/// lowering and in-memory emission. A Rust runtime path is used as given; only
/// [`plan_generate_project`] resolves it. Use this when the design has no
/// saved location yet to resolve relative destinations against.
pub fn check_generate_project(
    project: &mapping::Project,
    target: &GenerateTarget,
    profile: GenerateProfile,
) -> anyhow::Result<()> {
    let settings = GenerateSettings::default();
    require_backend(target, profile, &settings)?;
    let design = prepare_design(project, profile)?;
    let backend = match target {
        GenerateTarget::Rust { runtime_path } => Backend::Rust {
            runtime_dependency: runtime_path
                .to_str()
                .context("Rust codegen runtime path must be valid UTF-8")?
                .to_owned(),
        },
        GenerateTarget::CSharp => Backend::CSharp,
    };
    emit(&design, &backend, &settings).map(drop)
}

/// Reports what generating an in-memory design at `project_path` reads and
/// publishes. It applies every refusal the generating call makes before it
/// writes, in the same order, and ends with the destination check; nothing is
/// written and the emitted source is discarded.
pub fn plan_generate_project(
    project: &mapping::Project,
    project_path: &Path,
    output_directory: &Path,
    target: &GenerateTarget,
    profile: GenerateProfile,
) -> anyhow::Result<RoutePlan> {
    let settings = GenerateSettings::default();
    require_backend(target, profile, &settings)?;
    let design = prepare_design(project, profile)?;
    emit(&design, &resolve_backend(target, &settings)?, &settings)?;
    reject_existing_output(output_directory)?;
    let mut plan = RoutePlan::new(HostRoute::Generate, project_path);
    plan.publish(PlannedPublication::GeneratedTree(
        output_directory.to_path_buf(),
    ));
    Ok(plan)
}

/// [`plan_generate_project`] for the saved project at `project_path`.
pub fn plan_generate(
    project_path: &Path,
    output_directory: &Path,
    target: &GenerateTarget,
    profile: GenerateProfile,
) -> anyhow::Result<RoutePlan> {
    require_backend(target, profile, &GenerateSettings::default())?;
    let project = load_project(project_path)?;
    plan_generate_project(&project, project_path, output_directory, target, profile)
}

fn reject_existing_output(output_directory: &Path) -> Result<(), RouteRefusal> {
    if output_directory.exists() {
        return Err(RouteRefusal::GeneratedOutputExists {
            path: output_directory.to_path_buf(),
        });
    }
    Ok(())
}
