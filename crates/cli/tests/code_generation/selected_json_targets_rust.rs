//! Compiler-free preparation and the frozen 76 native selected-JSON bridge calls.
//! Generated Rust compilation/execution is a separate, serial root-owned gate.
use super::*;
use serde_json::Value as Json;
use sha2::Digest as _;
use std::{fs, io::Write as _};
#[path = "selected_json_targets_rust/native.rs"]
mod native;
#[path = "selected_json_targets_rust/projects.rs"]
mod projects;
const CONTROLS: &str = include_str!("selected_json_targets_rust/controls.json");
struct Evidence {
    path: PathBuf,
    complete: bool,
}
impl Evidence {
    fn new() -> TestResult<Self> {
        let path = PathBuf::from(
            std::env::var_os("FERRULE_SELECTED_JSON218_RUST_PREPARE_DIR")
                .ok_or("root must provide a fresh absolute prepare directory")?,
        );
        if !path.is_absolute()
            || fs::symlink_metadata(&path).is_ok()
            || !path.parent().is_some_and(Path::is_dir)
        {
            return Err("fresh absolute preparation path with existing parent required".into());
        }
        fs::create_dir(&path)?;
        eprintln!("SELECTED_JSON218_RUST_PREPARATION={}", path.display());
        Ok(Self {
            path,
            complete: false,
        })
    }
    fn write(&self, name: &str, bytes: &[u8]) -> TestResult<()> {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
        Ok(())
    }
    fn record(&self, name: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        self.write(name, format!("{value:#?}\n").as_bytes())
    }
    fn error(&self, name: &str, error: &(dyn std::error::Error + 'static)) -> TestResult<()> {
        let mut original = format!("DEBUG={error:#?}\nDISPLAY={error}\n");
        let mut cause = error.source();
        while let Some(error) = cause {
            original.push_str(&format!("CAUSE_DEBUG={error:#?}\nCAUSE_DISPLAY={error}\n"));
            cause = error.source();
        }
        original.push_str("CAUSE_CHAIN_TERMINATED=None\n");
        self.write(name, original.as_bytes())
    }
    fn artifacts(&self, prefix: &str, actual: &codegen::ArtifactSet) -> TestResult<()> {
        let mut rows = Vec::new();
        for file in actual.files() {
            self.write(&format!("{prefix}/{}", file.path.as_str()), &file.contents)?;
            rows.push(serde_json::json!({"path":file.path.as_str(),"bytes":file.contents.len(),"sha256":format!("{:x}",sha2::Sha256::digest(&file.contents))}));
        }
        self.write(
            &format!("{prefix}-COMPLETE-ARTIFACT-INDEX.json"),
            &serde_json::to_vec_pretty(&rows)?,
        )
    }
}
impl Drop for Evidence {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_KEEP_GENERATED_TEST_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
#[test]
#[ignore = "root-only one-job offline preparation/native cohort; generated host execution is separate"]
fn prepare_selected_json218_seven_projects_seventy_six_native_and_rust_controls() -> TestResult<()>
{
    let mut evidence = Evidence::new()?;
    evidence.write("COMPLETE-FROZEN40-RECIPES.json", CONTROLS.as_bytes())?;
    let corpus: Json = serde_json::from_str(CONTROLS)?;
    let projects = projects::projects();
    evidence.record("ALL7-COMPLETE-TYPED-PROJECTS.txt", &projects)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("workspace crates")?
        .join("codegen-runtime");
    let mut preparation = Vec::new();
    let mut modules = String::new();
    let mut dispatch =
        String::from("fn invoke(call:&Call<'_>)->Actual { let mut actual=match call.profile {\n");
    for (index, project) in projects.iter().enumerate() {
        let encoded = mapping::project_file::encode_pretty(project);
        evidence.record(
            &format!("profile{index}-COMPLETE-PUBLIC-CODEC-OUTCOME.txt"),
            &encoded,
        )?;
        let encoded = match encoded {
            Ok(encoded) => encoded,
            Err(error) => {
                evidence.error(
                    &format!("profile{index}-COMPLETE-PUBLIC-CODEC-ERROR.txt"),
                    &error,
                )?;
                return Err(error.into());
            }
        };
        evidence.write(
            &format!("profile{index}-PUBLIC-PROJECT.json"),
            encoded.as_bytes(),
        )?;
        let serde = serde_json::to_vec_pretty(project);
        evidence.record(
            &format!("profile{index}-COMPLETE-SERDE-PROJECT-OUTCOME.txt"),
            &serde,
        )?;
        let serde = match serde {
            Ok(serde) => serde,
            Err(error) => {
                evidence.error(
                    &format!("profile{index}-COMPLETE-SERDE-PROJECT-ERROR.txt"),
                    &error,
                )?;
                return Err(error.into());
            }
        };
        evidence.write(
            &format!("profile{index}-COMPLETE-SERDE-PROJECT.json"),
            &serde,
        )?;
        let validation = engine::validate(project);
        evidence.record(
            &format!("profile{index}-COMPLETE-VALIDATION.txt"),
            &validation,
        )?;
        let lowered = codegen::lower(project);
        evidence.record(
            &format!("profile{index}-COMPLETE-LOWERED-PROGRAM.txt"),
            &lowered,
        )?;
        let program = match lowered {
            Ok(program) => program,
            Err(error) => {
                evidence.error(
                    &format!("profile{index}-COMPLETE-LOWERING-ERROR.txt"),
                    &error,
                )?;
                preparation.push((index, false, error.to_string()));
                continue;
            }
        };
        let options = codegen_rust::Options {
            package_name: format!("selected-json218-profile{index}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        evidence.record(
            &format!("profile{index}-COMPLETE-EMISSION-OPTIONS.txt"),
            &options,
        )?;
        let emitted = codegen_rust::emit(&program, &options);
        match emitted {
            Ok(artifacts) => {
                // Complete bytes and their ordered index replace ArtifactSet byte-array Debug repetition.
                evidence.artifacts(&format!("rust-profile{index}"), &artifacts)?;
                evidence.record(&format!("profile{index}-EMISSION-STATUS.txt"), &"Ok")?;
                let source = artifacts
                    .files()
                    .iter()
                    .find(|file| file.path.as_str() == "src/lib.rs")
                    .ok_or("emitter omitted lib.rs")?;
                evidence.write(&format!("rust-host/src/case_{index}.rs"), &source.contents)?;
                modules.push_str(&format!("pub mod case_{index};\n"));
                dispatch.push_str(&format!("{index}=>route!(case_{index},call),\n"));
                preparation.push((index, validation.is_empty(), String::new()));
            }
            Err(error) => {
                evidence.record(
                    &format!("profile{index}-COMPLETE-EMISSION-ERROR.txt"),
                    &error,
                )?;
                evidence.error(
                    &format!("profile{index}-COMPLETE-EMISSION-ERROR-CHAIN.txt"),
                    &error,
                )?;
                preparation.push((index, false, error.to_string()));
            }
        }
    }
    dispatch.push_str("_=>panic!(\"unknown frozen profile\"), }; public_original(call,&actual); actual.wire=actual.outcome.as_ref().ok().map(|output|codegen_runtime::parse_json_bytes(call.wire_schema,&output.document)); actual }\n");
    let schemas = projects
        .iter()
        .map(|project| {
            (
                serde_json::to_string(&project.target).unwrap(),
                project
                    .extra_targets
                    .iter()
                    .map(|target| {
                        (
                            target.name.clone(),
                            serde_json::to_string(&target.schema).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let schemas = serde_json::to_string(&schemas)?;
    evidence.write(
        "COMPLETE-FROZEN-SELECTED-WIRE-SCHEMAS.json",
        schemas.as_bytes(),
    )?;
    let schema_source = format!(
        "const FROZEN_SCHEMAS:&str={};\n",
        serde_json::to_string(&schemas)?
    );
    let host=include_str!("selected_json_targets_rust/Host.rs.txt")
        .replace("// CAPTURE_SUPPORT is replaced by the complete immutable recorder source.",include_str!("selected_json_targets_rust/capture.rs.txt"))
        .replace("// FROZEN_SCHEMAS is replaced by independently constructed selected-schema descriptors.",&schema_source)
        .replace("// CASE_MODULES is replaced by seven public ordinary emitted modules.",&modules)
        .replace("// CASE_DISPATCH is replaced by the fixed seven-arm invocation.",&dispatch);
    evidence.write("rust-host/src/main.rs", host.as_bytes())?;
    evidence.write("rust-host/Cargo.toml",format!("[package]\nname = \"selected-json218-public-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\ncodegen-runtime = {{ path = {} }}\nserde_json = \"=1.0.150\"\n\n[workspace]\n",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;
    evidence.record("ALL7-COMPLETE-PREPARATION-OUTCOMES.txt", &preparation)?;
    fs::create_dir(evidence.path.join("native76"))?;
    let native = native::run(&projects, &corpus, &evidence.path.join("native76"));
    evidence.record(
        "COMPLETE-PREPARATION-AND-NATIVE-COMPARISONS.txt",
        &(&preparation, &native),
    )?;
    if preparation.len() != 7
        || preparation.iter().any(|(_, okay, _)| !*okay)
        || native.len() != 76
        || native.iter().any(|(_, _, matched)| !*matched)
    {
        return Err(
            "complete originals retained; preparation or native selected JSON differs".into(),
        );
    }
    evidence.complete = true;
    Ok(())
}
