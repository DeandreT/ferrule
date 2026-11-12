//! Private temporary directory for `dotnet` processes launched by tests.

use std::path::Path;
use std::process::Command;

/// Points .NET's own temporary output (`MSBuildTemp*`, `NuGetScratch*`,
/// runtime diagnostic pipes and random scratch directories) at
/// `<sandbox>/.tmp`, so it is removed or retained together with the
/// test-owned sandbox instead of accumulating in the caller's TMPDIR. The
/// leading dot keeps the directory out of SDK default item globs.
///
/// MSBuild worker nodes and the shared compiler server would otherwise
/// outlive the command with this directory as their temporary path, so these
/// launches build in-process.
pub trait DotnetTemp {
    fn dotnet_temp(&mut self, sandbox: &Path) -> &mut Self;
}

impl DotnetTemp for Command {
    fn dotnet_temp(&mut self, sandbox: &Path) -> &mut Self {
        let temp = sandbox.join(".tmp");
        // A missing directory surfaces as a .NET temporary-path failure.
        let _ = std::fs::create_dir_all(&temp);
        self.env("TMPDIR", &temp)
            .env("TEMP", &temp)
            .env("TMP", &temp)
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("UseSharedCompilation", "false")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TempDir;

    #[test]
    fn isolates_scratch_and_preserves_the_callers_command() {
        let directory = TempDir::with_retention("dotnet-command", false).unwrap();
        let mut command = Command::new("dotnet");
        command
            .args(["build", "--no-restore"])
            .current_dir(directory.path())
            .env("DOTNET_CLI_HOME", directory.join(".dotnet-home"))
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .dotnet_temp(directory.path());
        assert_eq!(command.get_program(), "dotnet");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["build", "--no-restore"]
        );
        assert_eq!(command.get_current_dir(), Some(directory.path()));
        let environment: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        let temp = directory.join(".tmp");
        assert!(temp.is_dir());
        for name in ["TMPDIR", "TEMP", "TMP"] {
            assert_eq!(
                environment[std::ffi::OsStr::new(name)],
                Some(temp.as_os_str())
            );
        }
        for (name, value) in [
            ("MSBUILDDISABLENODEREUSE", "1"),
            ("UseSharedCompilation", "false"),
            ("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false"),
        ] {
            assert_eq!(
                environment[std::ffi::OsStr::new(name)],
                Some(value.as_ref())
            );
        }
        assert_eq!(
            environment[std::ffi::OsStr::new("DOTNET_CLI_HOME")],
            Some(directory.join(".dotnet-home").as_os_str())
        );
        let path = directory.path().to_owned();
        drop(directory);
        assert!(!path.exists());
    }

    #[test]
    fn an_invalid_sandbox_still_defers_failure_to_dotnet() {
        let directory = TempDir::with_retention("dotnet-invalid", false).unwrap();
        let file = directory.write("file", "not a directory").unwrap();
        let mut command = Command::new("dotnet");
        command.dotnet_temp(&file);
        assert!(!file.join(".tmp").exists());
        assert!(command.get_envs().any(|(name, value)| {
            name == "TMPDIR" && value == Some(file.join(".tmp").as_os_str())
        }));
    }
}
