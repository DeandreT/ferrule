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
pub(crate) trait DotnetTemp {
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
