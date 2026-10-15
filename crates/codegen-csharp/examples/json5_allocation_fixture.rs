//! Materialize the original issue145 recipes and unchanged Release host for issue160.
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

// Reuse the authored byte and Program recipes, rather than maintaining a copy.
#[path = "../../cli/tests/json5_resources/corpus.rs"]
mod corpus;

mod support {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Read;

    pub(super) struct Identity {
        path: PathBuf,
        pub(super) bytes: u64,
        sha256: String,
    }
    impl Identity {
        pub(super) fn json(&self) -> Json {
            json!({"path":self.path,"bytes":self.bytes,"sha256":self.sha256})
        }
    }
    pub(super) fn identity(path: &Path) -> TestResult<Identity> {
        let path = path.canonicalize()?;
        let mut file = std::fs::File::open(&path)?;
        let mut hash = Sha256::new();
        let mut block = [0_u8; 65536];
        let mut bytes = 0;
        loop {
            let n = file.read(&mut block)?;
            if n == 0 {
                break;
            }
            hash.update(&block[..n]);
            bytes += n as u64;
        }
        Ok(Identity {
            path,
            bytes,
            sha256: format!("{:x}", hash.finalize()),
        })
    }
}

fn main() -> TestResult<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 2 {
        return Err("one absent preparation directory required".into());
    }
    let root = PathBuf::from(&args[1]);
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    std::fs::create_dir(root.join("corpus"))?;
    let cases = corpus::prepare(&root.join("corpus"))?;
    for profile in corpus::PROFILES {
        let program = corpus::program(profile)?;
        let valid = codegen::validate_program(&program);
        std::fs::write(
            root.join(format!("{profile}-PROGRAM.debug.txt")),
            format!("{program:#?}\n{valid:#?}\n"),
        )?;
        valid?;
        let result = codegen_csharp::emit_with_json5(&program);
        std::fs::write(
            root.join(format!("{profile}-EMISSION.debug.txt")),
            format!("{result:#?}\n"),
        )?;
        let artifacts = result?;
        let dir = root.join(profile);
        std::fs::create_dir(&dir)?;
        let library = dir.join("library");
        std::fs::create_dir(&library)?;
        for artifact in artifacts.files() {
            let path = library.join(artifact.path.as_str());
            std::fs::create_dir_all(path.parent().ok_or("artifact parent required")?)?;
            std::fs::write(path, &artifact.contents)?;
        }
        let host = dir.join("host");
        std::fs::create_dir(&host)?;
        std::fs::write(
            host.join("Program.cs"),
            include_bytes!("../../cli/tests/fixtures/json5_resources_csharp.cs.txt"),
        )?;
        std::fs::write(
            host.join("Host.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><Deterministic>true</Deterministic><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup>
  <ItemGroup><ProjectReference Include="../library/Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
        )?;
    }
    std::fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )?;
    std::fs::write(root.join("CASES.json"), serde_json::to_vec_pretty(&cases)?)?;
    Ok(())
}
