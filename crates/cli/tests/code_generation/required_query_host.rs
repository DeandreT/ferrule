use super::*;

fn source_row(name: &str, score: i64) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(name.into()))),
        ("Score".into(), Instance::Scalar(Value::Int(score))),
    ])
}

fn source_rows() -> Instance {
    Instance::Repeated(vec![
        source_row("Ada", 2),
        source_row("Bea", 4),
        source_row("Cal", 6),
    ])
}

fn output_names(names: &[&str]) -> Instance {
    Instance::Repeated(
        names
            .iter()
            .map(|name| {
                Instance::Group(vec![(
                    "Name".into(),
                    Instance::Scalar(Value::String((*name).into())),
                )])
            })
            .collect(),
    )
}

fn write_required_query(directory: &Path) -> TestResult<PathBuf> {
    let schema = SchemaNode::group("Person", vec![string("Name"), int("Score")]).repeating();
    let Instance::Repeated(rows) = source_rows() else {
        unreachable!()
    };
    format_db::write(&directory.join("people.sqlite"), &schema, &rows)?;
    let design = directory.join("required-query.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><resources><datasources><datasource name="people">
  <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="people.sqlite" name="people">
    <LocalViewStorage><LocalViewElement SQL="SELECT Name, Score FROM Person WHERE Score &gt; :Minimum">
      <PathElement Name="main" Kind="Database"/><PathElement Name="ScoresAboveMinimum" Kind="Select Statement"/>
      <Parameters><Parameter name="Minimum" type="integer"/></Parameters>
    </LocalViewElement></LocalViewStorage>
  </database_connection></datasource></datasources></resources>
  <component name="map" uid="1"><structure><children>
    <component name="Minimum" library="core" uid="2" kind="6">
      <sources><datapoint/></sources><targets><datapoint pos="0" key="10"/></targets>
      <data><input datatype="integer"/><parameter usageKind="input" name="Minimum"/></data>
    </component>
    <component name="catalog" library="db" uid="3" kind="15"><data><root><entry name="document">
      <entry name="ScoresAboveMinimum" type="routine" outkey="20"/>
    </entry></root><database ref="people"><data><selections><selection>
      <PathElement Name="main" Kind="Database"/><PathElement Name="ScoresAboveMinimum" Kind="Select Statement"/>
    </selection></selections></data></database></data></component>
    <component name="ScoresAboveMinimum" library="db" uid="4" kind="28"><data>
      <root><entry name="procedure" inpkey="21"/><entry name="ScoresAboveMinimum">
        <entry name="Minimum" type="attribute" inpkey="22"/>
      </entry></root>
      <root><entry name="ScoresAboveMinimum" outkey="30"><entry name="ScoresAboveMinimum">
        <entry name="Name" type="attribute" outkey="31"/>
        <entry name="Score" type="attribute" outkey="32"/>
      </entry></entry></root>
    </data></component>
    <component name="report" library="text" uid="5" kind="16"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40">
        <entry name="Name" inpkey="41"/>
      </entry></entry></entry></root>
      <text type="csv" outputinstance="report.csv"><settings separator="," quote="&quot;" firstrownames="true">
        <names root="report" block="Rows"><field0 name="Name" type="string"/></names>
      </settings></text>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="22"/></edges></vertex>
    <vertex vertexkey="20"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex>
    <vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex>
  </vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn imported_required_query_host_matches_generated_rust_and_csharp() -> TestResult<()> {
    let directory = TempDir::new("required_query_host")?;
    let design = write_required_query(&directory.0)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = imported.project;
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node,
                Node::RuntimeParameter { name, ty: ScalarType::Int, .. } if name == "Minimum"
            ))
            .count(),
        1
    );
    let source = format_db::read_instance(&directory.0.join("people.sqlite"), &project.source)?;
    assert_eq!(source, source_rows());
    assert!(matches!(
        engine::run(&project, &source),
        Err(engine::EngineError::MissingRuntimeParameter { name, .. }) if name == "Minimum"
    ));
    assert_eq!(
        engine::run(&project, &Instance::Repeated(Vec::new()))?,
        output_names(&[])
    );
    for (value, names) in [
        (Value::Int(3), vec!["Bea", "Cal"]),
        (Value::String(" 5 ".into()), vec!["Cal"]),
        (Value::Null, vec![]),
    ] {
        let mut parameters = engine::RuntimeParameters::new();
        parameters.insert("Minimum", value)?;
        let context = engine::ExecutionContext::new(&design).with_parameters(&parameters);
        assert_eq!(
            engine::run_with_context(&project, &source, &context)?,
            output_names(&names)
        );
    }
    let mut wrong = engine::RuntimeParameters::new();
    wrong.insert("Minimum", Value::Bool(true))?;
    let context = engine::ExecutionContext::new(&design).with_parameters(&wrong);
    assert!(matches!(
        engine::run_with_context(&project, &source, &context),
        Err(engine::EngineError::RuntimeParameterType {
            name,
            expected: ScalarType::Int,
            found: "bool",
            ..
        }) if name == "Minimum"
    ));

    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let rust_output = directory.0.join("rust");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/required_query_host_rust.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .env("RUSTFLAGS", "-Dwarnings")
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust required query host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/required_query_host_csharp.cs.txt"),
    )?;
    let csharp = dotnet_command(&csharp_output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp.status.success(),
        "generated C# required query host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
