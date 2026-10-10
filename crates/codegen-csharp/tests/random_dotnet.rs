use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope};

#[test]
fn generated_random_preserves_float_range_binding_shape_and_arity_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let project = Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("First", ScalarType::Float),
                SchemaNode::scalar("Again", ScalarType::Float),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                1,
                Node::Call {
                    function: "random".into(),
                    args: vec![],
                },
            )]
            .into(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "First".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Again".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    };
    let program = codegen::lower(&project)?;
    let root = std::env::temp_dir().join(format!(
        "ferrule_random_host_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&root)?;
    eprintln!("complete random host evidence: {}", root.display());
    std::fs::write(root.join("project.txt"), format!("{project:#?}\n"))?;
    std::fs::write(root.join("lowered-program.txt"), format!("{program:#?}\n"))?;
    for artifact in codegen_csharp::emit(&program)?.files() {
        let path = root.join(artifact.path.as_str());
        std::fs::create_dir_all(path.parent().expect("parent"))?;
        std::fs::write(path, &artifact.contents)?;
    }
    std::fs::create_dir(root.join("Host"))?;
    std::fs::write(
        root.join("Host/Host.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        root.join("Host/Program.cs"),
        include_str!("random/Host.cs.txt"),
    )?;
    std::fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>",
    )?;
    command(
        &root,
        "build",
        &["build", "Host/Host.csproj", "--configuration", "Release"],
    )?;
    command(
        &root,
        "run",
        &[
            "run",
            "--project",
            "Host/Host.csproj",
            "--configuration",
            "Release",
            "--no-build",
        ],
    )?;
    assert_eq!(
        std::fs::read_to_string(root.join("run.stdout.txt"))?.trim(),
        "random host passed"
    );
    let rust_root = root.join("Rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let options = codegen_rust::Options {
        package_name: "random_generated".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            runtime.to_string_lossy().into_owned(),
        ),
    };
    for artifact in codegen_rust::emit(&program, &options)?.files() {
        let path = rust_root.join(artifact.path.as_str());
        std::fs::create_dir_all(path.parent().expect("parent"))?;
        std::fs::write(path, &artifact.contents)?;
    }
    std::fs::write(
        rust_root.join("src/main.rs"),
        include_str!("random/Host.rs.txt"),
    )?;
    let mut rust_command = Command::new("cargo");
    rust_command
        .args(["run", "--quiet", "--offline"])
        .env("CARGO_TARGET_DIR", rust_root.join("target"))
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_INCREMENTAL", "0")
        .current_dir(&rust_root);
    let output = execute(&root, "rust", &mut rust_command)?;
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "random Rust host passed"
    );
    Ok(())
}

fn command(root: &Path, label: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::new("dotnet");
    command.args(args).current_dir(root);
    execute(root, label, &mut command)?;
    Ok(())
}

fn execute(
    root: &Path,
    label: &str,
    command: &mut Command,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let invocation = serde_json::json!({
        "program": command.get_program().to_string_lossy(),
        "arguments": command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "cwd": command.get_current_dir().map(|path| path.to_string_lossy().into_owned()),
        "environment_overrides": command.get_envs().map(|(key, value)| (key.to_string_lossy().into_owned(), value.map(|value| value.to_string_lossy().into_owned()))).collect::<Vec<_>>()
    });
    std::fs::write(
        root.join(format!("{label}.invocation.json")),
        serde_json::to_vec_pretty(&invocation)?,
    )?;
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            std::fs::write(
                root.join(format!("{label}.startup-error.txt")),
                error.to_string(),
            )?;
            return Err(error.into());
        }
    };
    std::fs::write(root.join(format!("{label}.stdout.txt")), &output.stdout)?;
    std::fs::write(root.join(format!("{label}.stderr.txt")), &output.stderr)?;
    std::fs::write(
        root.join(format!("{label}.status.txt")),
        format!("{}\n", output.status),
    )?;
    assert!(
        output.status.success(),
        "{label}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}
