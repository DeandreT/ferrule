//! The toolchain baselines in docs/toolchains.md are pinned in several files;
//! a bump must change all of them together.
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(repository().join(path))?)
}

fn rust_channel() -> anyhow::Result<String> {
    let file = read("rust-toolchain.toml")?;
    let channel = file
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .ok_or_else(|| anyhow::anyhow!("rust-toolchain.toml has no channel"))?;
    Ok(channel.trim_matches('"').to_owned())
}

const WORKFLOWS: [&str; 3] = [
    ".github/workflows/ci.yml",
    ".github/workflows/codegen.yml",
    ".github/workflows/site.yml",
];

#[test]
fn rust_channel_is_an_exact_stable_release() -> anyhow::Result<()> {
    let channel = rust_channel()?;
    let parts: Vec<_> = channel.split('.').collect();
    assert_eq!(parts.len(), 3, "{channel}");
    assert!(
        parts.iter().all(|part| part.parse::<u32>().is_ok()),
        "{channel}"
    );
    Ok(())
}

#[test]
fn workflows_install_the_pinned_rust_toolchain() -> anyhow::Result<()> {
    let channel = rust_channel()?;
    for path in WORKFLOWS {
        let workflow = read(path)?;
        let lines: Vec<_> = workflow.lines().map(str::trim).collect();
        let steps: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.contains("dtolnay/rust-toolchain@"))
            .collect();
        assert!(!steps.is_empty(), "{path} installs no Rust toolchain");
        for (index, line) in steps {
            assert_eq!(*line, "- uses: dtolnay/rust-toolchain@master", "{path}");
            let toolchain = lines[index + 1..]
                .iter()
                .take_while(|line| !line.starts_with("- "))
                .find_map(|line| line.strip_prefix("toolchain: "));
            assert_eq!(toolchain, Some(channel.as_str()), "{path}");
        }
    }
    Ok(())
}

#[test]
fn dotnet_sdk_comes_from_global_json() -> anyhow::Result<()> {
    let global: serde_json::Value = serde_json::from_str(&read("global.json")?)?;
    let version = global["sdk"]["version"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("global.json has no sdk.version"))?;
    assert!(version.starts_with("10.0.1"), "{version}");
    assert_eq!(global["sdk"]["rollForward"], "latestPatch");
    for path in WORKFLOWS {
        let workflow = read(path)?;
        assert!(!workflow.contains("dotnet-version:"), "{path}");
        if workflow.contains("actions/setup-dotnet@") {
            assert!(workflow.contains("global-json-file: global.json"), "{path}");
        }
    }
    Ok(())
}

#[test]
fn site_build_uses_a_pinned_trunk_and_the_committed_lockfile() -> anyhow::Result<()> {
    let site = read(".github/workflows/site.yml")?;
    assert!(site.contains("version: v0.16.0"));
    assert!(!site.contains("version: latest"));
    assert!(site.contains("git diff --exit-code -- Cargo.lock"));
    Ok(())
}
