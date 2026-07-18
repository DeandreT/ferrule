//! Local-only MapForce reference-output generation.
//!
//! This survey is ignored by default and never runs in CI. It launches the
//! locally installed reference application under a dedicated Xvfb display,
//! rejects mappings that could publish outside the redirected directory, and
//! writes one manifest below a brand-new caller-supplied output root.
//!
//! ```text
//! FERRULE_REFERENCE_SAMPLES_DIR=/tmp/ferrule-mapforce-references \
//! FERRULE_REFERENCE_SAMPLES_FILTER=ReferenceSamples_Hierarchical_JSON,SuppressNAFields \
//! cargo test -p mfd --test samples_reference_survey -- --ignored --nocapture
//! ```

use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const OUTPUT_DIR_ENV: &str = "FERRULE_REFERENCE_SAMPLES_DIR";
const FILTER_ENV: &str = "FERRULE_REFERENCE_SAMPLES_FILTER";
const LIMIT_ENV: &str = "FERRULE_REFERENCE_SAMPLES_LIMIT";
const DEFAULT_LIMIT: usize = 3;
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
struct PreflightSkip(String);

struct XvfbGuard {
    child: Child,
    display: String,
}

impl Drop for XvfbGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "requires the local MapForce reference app, Wine, xdotool, and Xvfb"]
fn generate_reference_samples_outputs() -> Result<(), Box<dyn Error>> {
    let workspace = workspace_root();
    let samples_root = workspace.join("samples");
    let samples = samples_root.join("ReferenceSamples");
    let reference_app = workspace.join("ref-app/MapForce.exe");
    if !samples.is_dir() || !reference_app.is_file() {
        println!("local samples/reference application unavailable; skipped");
        return Ok(());
    }
    require_commands(["Xvfb", "wine", "xdotool", "setsid", "kill"])?;
    require_registered_server()?;

    let output_root = std::env::var_os(OUTPUT_DIR_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{OUTPUT_DIR_ENV} must name a brand-new output directory"))?;
    create_safe_output_root(&workspace, &samples_root, &output_root)?;
    let script_path = output_root.join(".mapforce-reference.vbs");
    fs::write(&script_path, automation_script())?;

    let filter = requested_filter();
    let limit = requested_limit()?;
    let mut designs = Vec::new();
    collect_designs(&samples, &mut designs)?;
    designs.sort();
    let designs = designs
        .into_iter()
        .filter(|path| filter_matches(path, &filter))
        .take(limit)
        .collect::<Vec<_>>();
    if designs.is_empty() {
        return Err("the reference filter selected no .mfd files".into());
    }

    let xvfb = start_xvfb()?;
    let mut records = Vec::with_capacity(designs.len());
    for (index, design) in designs.iter().enumerate() {
        let relative = design.strip_prefix(&samples)?.to_string_lossy().to_string();
        let output_dir = output_root.join(format!(
            "{index:03}-{}",
            safe_stem(
                design
                    .file_stem()
                    .and_then(OsStr::to_str)
                    .unwrap_or("mapping")
            )
        ));
        let directory = output_dir
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("mapping-output")
            .to_string();
        let record = match safe_mapping(design) {
            Ok(()) => {
                fs::create_dir(&output_dir)?;
                match run_mapping(&xvfb.display, &script_path, design, &output_dir) {
                    Ok(output) => {
                        let outputs = collect_relative_outputs(&output_dir)?;
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let succeeded = stdout.contains("succeeded=True")
                            && stdout.contains("result_code=0")
                            && !outputs.is_empty();
                        serde_json::json!({
                            "file": relative,
                            "directory": directory,
                            "status": if succeeded { "passed" } else { "failed" },
                            "outputs": outputs,
                            "host_exit_code": output.status.code(),
                            "stdout": stdout.trim(),
                            "stderr": String::from_utf8_lossy(&output.stderr).trim(),
                        })
                    }
                    Err(error) => serde_json::json!({
                        "file": relative,
                        "directory": directory,
                        "status": "failed",
                        "outputs": [],
                        "reason": error.to_string(),
                    }),
                }
            }
            Err(PreflightSkip(reason)) => serde_json::json!({
                "file": relative,
                "directory": directory,
                "status": "skipped",
                "outputs": [],
                "reason": reason,
            }),
        };
        println!(
            "{}: {}",
            record["status"].as_str().unwrap_or("failed"),
            design
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("mapping.mfd")
        );
        records.push(record);
    }
    let display = xvfb.display.clone();
    drop(xvfb);
    fs::remove_file(&script_path)?;

    let passed = records
        .iter()
        .filter(|record| record["status"] == "passed")
        .count();
    let failed = records
        .iter()
        .filter(|record| record["status"] == "failed")
        .count();
    let skipped = records.len() - passed - failed;
    let manifest = serde_json::json!({
        "schema_version": 1,
        "kind": "ferrule.reference_samples_outputs",
        "safety": {
            "display": display,
            "wayland_unset": true,
            "sequential_processes": true,
            "output_root_was_new": true,
            "unsafe_and_dynamic_outputs_rejected": true,
        },
        "summary": {
            "selected": records.len(),
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
        },
        "samples": records,
    });
    let manifest_path = output_root.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    println!(
        "reference generation: {passed} passed, {failed} failed, {skipped} skipped; {}",
        manifest_path.display()
    );
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn create_safe_output_root(
    workspace: &Path,
    samples: &Path,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if output.exists() {
        return Err(format!("reference output root already exists: {}", output.display()).into());
    }
    fs::create_dir(output)?;
    let output = output.canonicalize()?;
    let samples = samples.canonicalize()?;
    let reference_app = workspace.join("ref-app").canonicalize()?;
    if output.starts_with(samples) || output.starts_with(reference_app) {
        fs::remove_dir(&output)?;
        return Err("reference outputs cannot be placed below samples/ or ref-app/".into());
    }
    Ok(())
}

fn require_commands<const N: usize>(commands: [&str; N]) -> Result<(), Box<dyn Error>> {
    for command in commands {
        let status = Command::new("sh")
            .args(["-c", &format!("command -v '{command}' >/dev/null 2>&1")])
            .status()?;
        if !status.success() {
            return Err(format!("required local command `{command}` is unavailable").into());
        }
    }
    Ok(())
}

fn require_registered_server() -> Result<(), Box<dyn Error>> {
    let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
    let prefix = std::env::var_os("WINEPREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".wine"));
    let server = prefix.join("drive_c/Program Files/ReferenceSamples/ReferenceSamples/MapForce.exe");
    if !server.is_file() {
        return Err(format!(
            "the registered Wine COM server is unavailable at {}; link the local ref-app directory there without copying it",
            server.display()
        )
        .into());
    }
    Ok(())
}

fn requested_filter() -> Vec<String> {
    std::env::var(FILTER_ENV)
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn requested_limit() -> Result<usize, Box<dyn Error>> {
    let limit = std::env::var(LIMIT_ENV)
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > 120 {
        return Err(format!("{LIMIT_ENV} must be between 1 and 120").into());
    }
    Ok(limit)
}

fn filter_matches(path: &Path, filter: &[String]) -> bool {
    filter.is_empty()
        || path
            .file_stem()
            .and_then(OsStr::to_str)
            .is_some_and(|stem| filter.iter().any(|candidate| candidate == stem))
}

fn safe_stem(stem: &str) -> String {
    stem.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn collect_designs(directory: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_designs(&path, output)?;
        } else if path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
        {
            output.push(path);
        }
    }
    Ok(())
}

fn safe_mapping(path: &Path) -> Result<(), PreflightSkip> {
    let text = fs::read_to_string(path)
        .map_err(|error| PreflightSkip(format!("cannot read mapping: {error}")))?;
    let document = roxmltree::Document::parse(&text)
        .map_err(|error| PreflightSkip(format!("cannot parse mapping: {error}")))?;
    for node in document.descendants().filter(|node| node.is_element()) {
        let output_instance = node.attribute("outputinstance").or_else(|| {
            (node.has_tag_name("file") && node.attribute("role") == Some("outputinstance"))
                .then(|| node.attribute("name"))
                .flatten()
        });
        if output_instance.is_some_and(|value| !is_safe_relative_output(value)) {
            return Err(PreflightSkip(
                "an output path could bypass the redirected base directory".into(),
            ));
        }
        if node.attribute("updateexistingfile") == Some("1") {
            return Err(PreflightSkip(
                "update-in-place workbook output is not reference-safe".into(),
            ));
        }
        if node
            .attribute("inputinstance")
            .is_some_and(|value| value.contains("://"))
        {
            return Err(PreflightSkip("network input is disabled".into()));
        }
        if node.has_tag_name("component")
            && matches!(node.attribute("library"), Some("db" | "webservice"))
        {
            return Err(PreflightSkip(
                "database and web-service components are excluded".into(),
            ));
        }
        if node.has_tag_name("entry")
            && node.attribute("name") == Some("FileInstance")
            && node.attribute("inpkey").is_some()
        {
            return Err(PreflightSkip(
                "connected target FileInstance paths are dynamic".into(),
            ));
        }
    }
    Ok(())
}

fn is_safe_relative_output(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with(['/', '\\'])
        || value
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
        || value.split_once("://").is_some()
    {
        return false;
    }
    !value
        .replace('\\', "/")
        .split('/')
        .any(|segment| segment == "..")
}

fn start_xvfb() -> Result<XvfbGuard, Box<dyn Error>> {
    for number in 120..200 {
        let display = format!(":{number}");
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        let lock = PathBuf::from(format!("/tmp/.X{number}-lock"));
        if socket.exists() || lock.exists() {
            continue;
        }
        let mut child = Command::new("Xvfb")
            .args([&display, "-screen", "0", "1600x900x24", "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if socket.exists() {
                return Ok(XvfbGuard { child, display });
            }
            if child.try_wait()?.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    Err("could not allocate an isolated Xvfb display".into())
}

fn run_mapping(
    display: &str,
    script: &Path,
    design: &Path,
    output_dir: &Path,
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new("setsid");
    command
        .arg("wine")
        .arg("cscript.exe")
        .arg("//Nologo")
        .arg(wine_path(script)?)
        .arg(wine_path(design)?)
        .arg(wine_path(output_dir)?)
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", display)
        .env("WINEDEBUG", "-all")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let process_group = child.id();
    let deadline = Instant::now() + SAMPLE_TIMEOUT;
    loop {
        dismiss_completion_modal(display);
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map_err(Into::into);
        }
        if Instant::now() >= deadline {
            terminate_process_group(process_group);
            let _ = child.wait();
            return Err(format!("MapForce generation exceeded {SAMPLE_TIMEOUT:?}").into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn dismiss_completion_modal(display: &str) {
    let search = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", "^MapForce$"])
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", display)
        .output();
    let Ok(search) = search else {
        return;
    };
    for id in String::from_utf8_lossy(&search.stdout).split_whitespace() {
        let focused = Command::new("xdotool")
            .args(["windowfocus", id])
            .env_remove("WAYLAND_DISPLAY")
            .env("DISPLAY", display)
            .status()
            .is_ok_and(|status| status.success());
        if focused {
            let _ = Command::new("xdotool")
                .args(["key", "Return"])
                .env_remove("WAYLAND_DISPLAY")
                .env("DISPLAY", display)
                .status();
        }
    }
}

fn terminate_process_group(process_group: u32) {
    let _ = Command::new("kill")
        .args(["--", &format!("-{process_group}")])
        .status();
}

fn wine_path(path: &Path) -> Result<String, Box<dyn Error>> {
    let path = path.canonicalize()?;
    let path = path
        .to_str()
        .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))?;
    Ok(format!("Z:{}", path.replace('/', "\\")))
}

fn collect_relative_outputs(directory: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let root = directory.canonicalize()?;
    let mut paths = Vec::new();
    collect_output_files(directory, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let path = path.canonicalize()?;
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| "generated output escaped its assigned directory")?;
            Ok(relative.to_string_lossy().to_string())
        })
        .collect()
}

fn collect_output_files(directory: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_output_files(&path, output)?;
        } else {
            output.push(path);
        }
    }
    Ok(())
}

fn automation_script() -> &'static str {
    r#"Option Explicit
If WScript.Arguments.Count <> 2 Then WScript.Quit 2
Dim application, document, result, succeeded
On Error Resume Next
Set application = CreateObject("MapForce_x64.Application")
If Err.Number <> 0 Then
  WScript.Echo "create application failed: " & Err.Description
  WScript.Quit 3
End If
application.Visible = True
Set document = application.OpenMapping(WScript.Arguments(0))
If Err.Number <> 0 Then
  WScript.Echo "open mapping failed: " & Err.Description
  application.Quit
  WScript.Quit 4
End If
Set result = document.GenerateOutputEx2(WScript.Arguments(1))
If Err.Number <> 0 Then
  WScript.Echo "generate output failed: " & Err.Description
  application.Quit
  WScript.Quit 5
End If
succeeded = result.Succeeded
WScript.Echo "succeeded=" & CStr(succeeded)
WScript.Echo "result_code=" & CStr(result.ResultCode)
WScript.Echo "messages=" & result.MessageText
application.Quit
If CBool(succeeded) = False Then WScript.Quit 6
"#
}

#[test]
fn safety_preflight_rejects_external_publication_and_network_inputs() {
    let cases = [
        r#"<mapping><document outputinstance="../outside.xml"/></mapping>"#,
        r#"<mapping><document outputinstance="C:\outside.xml"/></mapping>"#,
        r#"<mapping><document inputinstance="https://example.test/input.xml"/></mapping>"#,
        r#"<mapping><entry name="FileInstance" inpkey="7"/></mapping>"#,
        r#"<mapping><component library="db"/></mapping>"#,
        r#"<mapping><excel updateexistingfile="1"/></mapping>"#,
    ];
    let directory = TestDir::new("unsafe");
    for (index, contents) in cases.iter().enumerate() {
        let path = directory.0.join(format!("case-{index}.mfd"));
        fs::write(&path, contents).unwrap();
        assert!(safe_mapping(&path).is_err(), "{contents}");
    }
}

#[test]
fn safety_preflight_accepts_local_inputs_and_pathless_outputs() {
    let directory = TestDir::new("safe");
    let path = directory.0.join("safe.mfd");
    fs::write(
        &path,
        r#"<mapping><component library="xml"><document inputinstance="input.xml" outputinstance="unused-source.xml"/></component><component library="json"><json outputinstance="result.json"/></component></mapping>"#,
    )
    .unwrap();
    assert!(safe_mapping(&path).is_ok());
}

#[test]
fn wine_paths_use_the_local_z_drive_without_loss() {
    let directory = TestDir::new("wine_path");
    let path = directory.0.join("mapping.mfd");
    fs::write(&path, "<mapping/>").unwrap();
    let converted = wine_path(&path).unwrap();
    assert!(converted.starts_with("Z:\\"), "{converted}");
    assert!(converted.ends_with("\\mapping.mfd"), "{converted}");
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_reference_samples_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
