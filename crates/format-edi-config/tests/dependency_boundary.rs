use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

#[test]
fn runtime_consumers_do_not_reach_the_config_compiler() -> Result<(), Box<dyn std::error::Error>> {
    // Inspect normal/build edges, not compiler-only test dependencies. No
    // compilation or network resolution is needed to inspect workspace edges.
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version=1",
            "--no-deps",
            "--offline",
            "--locked",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut graph = BTreeMap::new();
    for package in metadata["packages"].as_array().ok_or("missing packages")? {
        let name = package["name"].as_str().ok_or("missing package name")?;
        let dependencies = package["dependencies"]
            .as_array()
            .ok_or("missing dependencies")?
            .iter()
            .filter(|dependency| dependency["kind"] != "dev")
            .map(|dependency| dependency["name"].as_str().ok_or("missing dependency name"))
            .collect::<Result<BTreeSet<_>, _>>()?;
        graph.insert(name, dependencies);
    }
    let reachable = |root| {
        let mut visited = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(package) = pending.pop() {
            if visited.insert(package)
                && let Some(dependencies) = graph.get(package)
            {
                pending.extend(dependencies.iter().copied());
            }
        }
        visited
    };
    for consumer in ["format-edi", "engine", "codegen", "codegen-runtime"] {
        assert!(graph.contains_key(consumer), "missing {consumer}");
        assert!(
            !reachable(consumer).contains("format-edi-config"),
            "{consumer} must not depend on the configuration compiler"
        );
    }
    assert!(reachable("mfd").contains("format-edi-config"));
    assert!(!reachable("format-edi-config").contains("format-edi"));
    assert!(!graph["format-edi"].contains("roxmltree"));
    Ok(())
}
