//! Whole-design admission ledger for the self-authored corpus in
//! `fixtures/synthetic-corpus`. Each design's import, executable assessment and
//! Rust/C# source generation outcome must match `ledger.json`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic-corpus")
}

fn generated(project: &mapping::Project) -> (&'static str, &'static str) {
    let Ok(program) = codegen::lower(project) else {
        return ("refused", "refused");
    };
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-synthetic-corpus".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        },
    );
    let csharp = codegen_csharp::emit(&program);
    let outcome = |ok: bool| if ok { "ok" } else { "refused" };
    (outcome(rust.is_ok()), outcome(csharp.is_ok()))
}

/// The observed outcome, with each issue's full message as its fragment.
fn observe(path: &Path) -> Value {
    let imported = match mfd::import(path) {
        Ok(imported) => imported,
        Err(error) => return json!({ "import": format!("error: {error}") }),
    };
    let report = mfd::assess_import(&imported);
    let strict = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    );
    assert_eq!(strict.is_ok(), report.executable, "{}", path.display());
    let (rust, csharp) = generated(&imported.project);
    json!({
        "import": "ok",
        "executable": report.executable,
        "issues": report.issues.iter().map(|issue| json!({
            "kind": issue.kind,
            "contains": issue.message,
        })).collect::<Vec<_>>(),
        "rust": rust,
        "csharp": csharp,
    })
}

/// Every non-issue field must be equal. Issues must agree in number and kind,
/// and each expected fragment must occur in a distinct observed issue.
fn matches(expected: &Value, observed: &Value) -> bool {
    let fields = |value: &Value| {
        let mut value = value.clone();
        value.as_object_mut().map(|object| object.remove("issues"));
        value
    };
    let issues = |value: &Value| value["issues"].as_array().cloned().unwrap_or_default();
    if fields(expected) != fields(observed) {
        return false;
    }
    let (expected, mut observed) = (issues(expected), issues(observed));
    expected.len() == observed.len()
        && expected.iter().all(|want| {
            let fragment = want["contains"].as_str().unwrap_or_default();
            observed
                .iter()
                .position(|have| {
                    have["kind"] == want["kind"]
                        && have["contains"]
                            .as_str()
                            .unwrap_or_default()
                            .contains(fragment)
                })
                .map(|index| observed.remove(index))
                .is_some()
        })
}

#[test]
fn synthetic_corpus_designs_match_the_stage_outcome_ledger() {
    let ledger: BTreeMap<String, Value> =
        serde_json::from_slice(&std::fs::read(corpus().join("ledger.json")).unwrap()).unwrap();
    let mut designs: Vec<_> = std::fs::read_dir(corpus())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mfd"))
        .collect();
    designs.sort();
    assert!(!designs.is_empty());
    let observed: BTreeMap<String, Value> = designs
        .iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, observe(path))
        })
        .collect();
    let stale: Vec<_> = observed
        .iter()
        .filter(|(name, value)| !ledger.get(*name).is_some_and(|want| matches(want, value)))
        .map(|(name, _)| name.as_str())
        .chain(
            ledger
                .keys()
                .filter(|name| !observed.contains_key(*name))
                .map(String::as_str),
        )
        .collect();
    assert!(
        stale.is_empty(),
        "ledger differs for {stale:?}; observed outcomes (trim each issue to a stable fragment):\n{}",
        serde_json::to_string_pretty(&observed).unwrap()
    );
}
