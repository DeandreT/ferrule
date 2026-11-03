//! Proposed shared TEST-ONLY extraction from root-scalars 181571be.
//! Intended path: crates/mfd/tests/support/generated_sdk_policy.rs.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

pub(crate) fn copy_sdk_policy(cwd: &Path, source: &Path) -> Result<String, String> {
    let bytes = std::fs::read(source).map_err(|error| format!("read SDK global.json: {error}"))?;
    let policy: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| "SDK global.json must be valid JSON".to_owned())?;
    let version = policy["sdk"]["version"]
        .as_str()
        .ok_or("SDK global.json must declare sdk.version")?;
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || !parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("SDK global.json must pin a stable three-part sdk.version".into());
    }
    if policy["sdk"]["rollForward"] != "latestPatch" || policy["sdk"]["allowPrerelease"] != false {
        return Err(
            "SDK global.json must set sdk.rollForward=latestPatch and sdk.allowPrerelease=false"
                .into(),
        );
    }
    // Selection follows this actual cwd, not a --project argument.
    std::fs::write(cwd.join("global.json"), &bytes)
        .map_err(|error| format!("copy SDK global.json: {error}"))?;
    Ok(version.to_owned())
}

pub(crate) fn check_sdk_version(stdout: &[u8], expected: &str) -> Result<(), String> {
    let actual = std::str::from_utf8(stdout)
        .map_err(|_| "dotnet --version returned non-UTF-8 output".to_owned())?;
    if actual.trim() != expected {
        return Err(format!(
            "dotnet --version must select the exact global.json sdk.version ({expected})"
        ));
    }
    Ok(())
}

/// One environment factory for SDK probing, building and DLL-only runs.
pub(crate) fn dotnet_command(cwd: &Path, executable: &OsStr) -> Command {
    let mut command = Command::new(executable);
    command
        .current_dir(cwd)
        .env("DOTNET_CLI_HOME", cwd.join("dotnet-home"))
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1");
    command
}
