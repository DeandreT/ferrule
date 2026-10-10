//! Compiler-free preparation of seven public C# selected JSON host profiles.
//! The root builds the retained host and separately admits its small/large cohorts.
use super::*;
use serde_json::Value as Json;
use std::fs;
use std::io::Write;

#[path = "selected_json_targets_csharp/projects.rs"]
mod projects;
const CONTROLS: &str = include_str!("selected_json_targets_csharp/controls.json");

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let path = PathBuf::from(
            std::env::var_os("FERRULE_SELECTED_JSON218_CSHARP_PREPARE_DIR")
                .ok_or("root must supply a fresh absolute preparation directory")?,
        );
        if !path.is_absolute() || fs::symlink_metadata(&path).is_ok() {
            return Err("preparation directory must be fresh and absolute".into());
        }
        if !path.parent().is_some_and(Path::is_dir) {
            return Err("existing preparation parent required".into());
        }
        fs::create_dir(&path)?;
        eprintln!("SELECTED_JSON218_CSHARP_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn write(&self, name: &str, bytes: &[u8]) -> TestResult<()> {
        let leaf = Path::new(name);
        if leaf
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("evidence paths must contain only ordinary relative components".into());
        }
        let path = self.0.join(leaf);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }
    fn record(&self, name: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        self.write(name, format!("{value:#?}\n").as_bytes())
    }
    fn error(&self, name: &str, error: &(dyn std::error::Error + 'static)) -> TestResult<()> {
        let mut text = format!("DEBUG={error:#?}\nDISPLAY={error}\n");
        let mut cause = error.source();
        while let Some(error) = cause {
            text.push_str(&format!("CAUSE_DEBUG={error:#?}\nCAUSE_DISPLAY={error}\n"));
            cause = error.source();
        }
        self.write(name, text.as_bytes())
    }
}

#[test]
#[ignore = "root-owned preparation; builds and 77 calls are separate serial gates"]
fn prepare_seven_frozen_selected_json_csharp_profiles_and_full_artifacts() -> TestResult<()> {
    let evidence = Evidence::new()?;
    evidence.write("controls.json", CONTROLS.as_bytes())?;
    let corpus: Json = serde_json::from_str(CONTROLS)?;
    if corpus["profile_count"] != 7
        || corpus["case_count"] != 40
        || corpus["planned_csharp_calls"] != 77
    {
        return Err("wrong fixed selected JSON catalog".into());
    }
    let profiles = projects::all();
    let mut shared = BTreeMap::<String, Vec<u8>>::new();
    let mut schemas = serde_json::Map::new();
    let mut statuses = Vec::new();
    for (index, (id, project)) in profiles.iter().enumerate() {
        let prefix = format!("profile{index}");
        evidence.record(&format!("{prefix}-COMPLETE-PROJECT-ORIGINAL.txt"), project)?;
        let wire = mapping::project_file::encode_pretty(project);
        evidence.record(
            &format!("{prefix}-COMPLETE-CODEC-OUTCOME-ORIGINAL.txt"),
            &wire,
        )?;
        let wire = match wire {
            Ok(wire) => wire,
            Err(error) => {
                evidence.error(&format!("{prefix}-CODEC-ERROR-ORIGINAL.txt"), &error)?;
                return Err(error.into());
            }
        };
        evidence.write(&format!("{prefix}-project.json"), wire.as_bytes())?;
        let decoded = mapping::project_file::decode_str(&wire);
        evidence.record(
            &format!("{prefix}-COMPLETE-CODEC-RELOAD-ORIGINAL.txt"),
            &decoded,
        )?;
        let decoded = match decoded {
            Ok(decoded) => decoded,
            Err(error) => {
                evidence.error(&format!("{prefix}-CODEC-RELOAD-ERROR-ORIGINAL.txt"), &error)?;
                return Err(error.into());
            }
        };
        if serde_json::to_value(&decoded)? != serde_json::to_value(project)? {
            return Err(
                "complete project changed on public codec reload; both originals retained".into(),
            );
        }
        let validation = engine::validate(project);
        evidence.record(
            &format!("{prefix}-COMPLETE-VALIDATION-ORIGINAL.txt"),
            &validation,
        )?;
        let lowered = codegen::lower(project);
        evidence.record(
            &format!("{prefix}-COMPLETE-PROGRAM-OUTCOME-ORIGINAL.txt"),
            &lowered,
        )?;
        let program = match lowered {
            Ok(program) => program,
            Err(error) => {
                evidence.error(&format!("{prefix}-LOWERING-ERROR-ORIGINAL.txt"), &error)?;
                statuses.push((index, *id, false));
                continue;
            }
        };
        let emitted = codegen_csharp::emit(&program);
        // Complete outcomes precede status checks and artifact partitioning.
        evidence.record(
            &format!("{prefix}-COMPLETE-ARTIFACT-OUTCOME-ORIGINAL.txt"),
            &emitted,
        )?;
        let artifacts = match emitted {
            Ok(artifacts) => artifacts,
            Err(error) => {
                evidence.error(&format!("{prefix}-EMITTER-ERROR-ORIGINAL.txt"), &error)?;
                statuses.push((index, *id, false));
                continue;
            }
        };
        for file in artifacts.files() {
            evidence.write(
                &format!("raw-profile{index}/{}", file.path.as_str()),
                &file.contents,
            )?;
        }
        for file in artifacts.files() {
            let path = file.path.as_str();
            if matches!(path, "GeneratedMapping.cs" | "GeneratedTargetBuilder.cs") {
                let original = std::str::from_utf8(&file.contents)?;
                let before = "namespace Ferrule.Generated;";
                if original.matches(before).count() != 1 {
                    return Err("exact sole namespace anchor required".into());
                }
                let after = format!("namespace Ferrule.Generated.Case{index};");
                let composed = original.replacen(before, &after, 1);
                evidence.write(
                    &format!("aggregate/Case{index}/{path}"),
                    composed.as_bytes(),
                )?;
                evidence.record(
                    &format!("{prefix}-{path}-NAMESPACE-ONLY-EQUATION.txt"),
                    &(before, &after, original, &composed),
                )?;
            } else if path.starts_with("Runtime/") {
                if let Some(before) = shared.get(path) {
                    evidence.record(
                        &format!(
                            "{prefix}-{}-SHARED-BODY-COMPARISON.txt",
                            path.replace('/', "_")
                        ),
                        &(before, &file.contents),
                    )?;
                    if before != &file.contents {
                        return Err("shared runtime bodies differ; originals retained".into());
                    }
                } else {
                    shared.insert(path.to_owned(), file.contents.clone());
                }
            } else if path != "Ferrule.Generated.csproj" {
                return Err("unexpected artifact retained before refusal".into());
            }
        }
        let primary = codegen::serialize_embedded_schema(&project.target, 1_048_576);
        let named: Vec<_> = project
            .extra_targets
            .iter()
            .map(|target| {
                (
                    target.name.clone(),
                    codegen::serialize_embedded_schema(&target.schema, 1_048_576),
                )
            })
            .collect();
        evidence.record(
            &format!("{prefix}-COMPLETE-SELECTED-SCHEMA-OUTCOMES-ORIGINAL.txt"),
            &(&primary, &named),
        )?;
        let primary = match primary {
            Ok(primary) => primary,
            Err(error) => {
                evidence.error(
                    &format!("{prefix}-PRIMARY-SCHEMA-ERROR-ORIGINAL.txt"),
                    &error,
                )?;
                return Err(error.into());
            }
        };
        let mut named_schemas = serde_json::Map::new();
        for (ordinal, (name, schema)) in named.into_iter().enumerate() {
            let schema = match schema {
                Ok(schema) => schema,
                Err(error) => {
                    evidence.error(
                        &format!("{prefix}-NAMED-SCHEMA{ordinal}-ERROR-ORIGINAL.txt"),
                        &error,
                    )?;
                    return Err(error.into());
                }
            };
            named_schemas.insert(name, Json::String(schema));
        }
        schemas.insert(
            (*id).to_owned(),
            serde_json::json!({"primary": primary, "named": named_schemas}),
        );
        statuses.push((index, *id, validation.is_empty()));
    }
    for (path, body) in shared {
        evidence.write(&format!("aggregate/{path}"), &body)?;
    }
    evidence.write("aggregate/Host.csproj", HOST_PROJECT.as_bytes())?;
    evidence.write(
        "aggregate/Host.cs",
        include_str!("selected_json_targets_csharp/Host.cs.txt").as_bytes(),
    )?;
    evidence.write(
        "aggregate/Capture.cs",
        include_str!("selected_json_targets_csharp/Capture.cs.txt").as_bytes(),
    )?;
    evidence.write(
        "aggregate/NuGet.Config",
        b"<configuration><packageSources><clear /></packageSources></configuration>\n",
    )?;
    evidence.write(
        "selected-schemas.json",
        &serde_json::to_vec_pretty(&schemas)?,
    )?;
    evidence.record("ALL7-COMPLETE-PREPARATION-OUTCOMES-ORIGINAL.txt", &statuses)?;
    if statuses.len() != 7 || statuses.iter().any(|(_, _, matched)| !matched) {
        return Err("complete originals retained; seven-profile preparation refused".into());
    }
    Ok(())
}

const HOST_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <Deterministic>true</Deterministic><InvariantGlobalization>true</InvariantGlobalization>
    <NuGetAudit>false</NuGetAudit><UseSharedCompilation>false</UseSharedCompilation>
    <DisableTransitiveFrameworkReferenceDownloads>true</DisableTransitiveFrameworkReferenceDownloads>
    <AutomaticallyUseReferenceAssemblyPackages>false</AutomaticallyUseReferenceAssemblyPackages>
  </PropertyGroup>
</Project>
"#;
