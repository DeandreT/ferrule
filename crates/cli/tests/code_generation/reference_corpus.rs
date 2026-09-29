//! Opt-in generated-backend execution against eight local, gitignored mappings.
//! Run with `cargo test -p cli --features codegen-tests --test code_generation
//! reference_corpus -- --ignored --nocapture` when the local sample corpus and
//! .NET 10 SDK are available. No sample contents are copied into this test.

use super::*;

#[derive(Clone, Copy)]
enum SourceKind {
    Json,
    Xml,
    FlexText,
    Csv,
}

#[derive(Clone, Copy)]
enum TargetKind {
    Json,
    Xml,
}

struct CorpusCase {
    sample: &'static str,
    input: &'static str,
    source_kind: SourceKind,
    target_kind: TargetKind,
}

const CASES: [CorpusCase; 8] = [
    CorpusCase {
        sample: "EmployeesToJSONObject.mfd",
        input: "Altova_Hierarchical.json",
        source_kind: SourceKind::Json,
        target_kind: TargetKind::Json,
    },
    CorpusCase {
        sample: "Altova_Hierarchical_JSON.mfd",
        input: "Altova_Hierarchical.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Json,
    },
    CorpusCase {
        sample: "Altova_Hierarchical_FLF.mfd",
        input: "Altova_Hierarchical_FLF.txt",
        source_kind: SourceKind::FlexText,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "BuildHierarchyFromTextfile.mfd",
        input: "People.txt",
        source_kind: SourceKind::Csv,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "GroupTemperaturesByYear.mfd",
        input: "Temperatures.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "SortByMultipleKeys.mfd",
        input: "OrgChart.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "FindHighestTemperatures.mfd",
        input: "Temperatures.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "PositionInFilteredSequence.mfd",
        input: "BranchOffices.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
];

#[test]
#[ignore = "requires the local ignored ReferenceSamples corpus and .NET 10 SDK"]
fn generated_rust_and_csharp_execute_local_samples_like_engine() -> TestResult<()> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let directory = TempDir::new("reference_corpus")?;
    let rust_target = directory.0.join("rust-target");
    for (index, case) in CASES.iter().enumerate() {
        let case_dir = directory.0.join(format!("case-{index}"));
        std::fs::create_dir(&case_dir)?;
        run_case(&samples, &case_dir, &rust_target, case)?;
    }
    println!(
        "{} local mappings compiled and executed in generated Rust and C#",
        CASES.len()
    );
    Ok(())
}

fn run_case(
    samples: &Path,
    case_dir: &Path,
    rust_target: &Path,
    case: &CorpusCase,
) -> TestResult<()> {
    let sample = case.sample;
    let mapping_path = samples.join(sample);
    let input_path = samples.join(case.input);
    let imported = mfd::import_with_options(
        &mapping_path,
        &mfd::ImportOptions::default().with_package_root(samples),
    )?;
    assert!(
        imported.warnings.is_empty(),
        "{sample}: import warnings: {:?}",
        imported.warnings
    );
    let project = imported.project;
    assert!(
        project.runtime_dependencies().is_empty(),
        "{sample}: host dependencies prevent deterministic execution: {:?}",
        project.runtime_dependencies()
    );
    assert!(
        project.extra_sources.is_empty(),
        "{sample}: expected one input"
    );
    assert!(
        match case.target_kind {
            TargetKind::Json => project.target_options.json_document,
            TargetKind::Xml => project.target_options.xml_document,
        },
        "{sample}: unexpected output format"
    );
    assert!(
        match case.source_kind {
            SourceKind::Json => project.source_options.json_document,
            SourceKind::Xml => project.source_options.xml_document,
            SourceKind::FlexText => project.source_options.flextext.is_some(),
            SourceKind::Csv => {
                project.source_options.tabular_kind == Some(mapping::TabularBoundaryKind::Csv)
            }
        },
        "{sample}: unexpected input format"
    );
    assert_eq!(
        project
            .source_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(OsStr::to_str),
        Some(case.input),
        "{sample}: unexpected source instance"
    );
    let validation = engine::validate(&project);
    assert!(validation.is_empty(), "{sample}: {validation:?}");

    let source = match case.source_kind {
        SourceKind::Json => format_json::read(&input_path, &project.source)?,
        SourceKind::Xml => format_xml::read(&input_path, &project.source)?,
        SourceKind::FlexText => format_flextext::read(
            &input_path,
            &project.source,
            project.source_options.flextext.as_ref().unwrap(),
        )?,
        SourceKind::Csv => Instance::Repeated(format_csv::read(
            &input_path,
            &project.source,
            project.source_options.delimiter,
            project.source_options.has_header_row.unwrap_or(true),
        )?),
    };
    // Generated hosts expose a schema-shaped JSON API. Preserve native XML,
    // FlexText, and CSV readers' typed instances while crossing that host API.
    let source_json = match case.source_kind {
        SourceKind::Json => std::fs::read_to_string(&input_path)?,
        SourceKind::Xml | SourceKind::FlexText | SourceKind::Csv => {
            format_json::to_string(&project.source, &source)?
        }
    };
    let expected = engine::run(&project, &source)?;
    let expected_json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &expected)?)?;
    if sample == "BuildHierarchyFromTextfile.mfd" {
        let Instance::Repeated(rows) = &source else {
            panic!("{sample}: CSV source should contain repeated rows");
        };
        assert_eq!(rows.len(), 30, "{sample}: source row count");
        assert_eq!(expected_json["Name"], "Organization Chart");
        let offices = expected_json["Office"].as_array().expect("office array");
        assert_eq!(offices.len(), 2, "{sample}: company groups");
        let departments = offices
            .iter()
            .flat_map(|office| office["Department"].as_array().expect("department array"))
            .collect::<Vec<_>>();
        assert_eq!(departments.len(), 7, "{sample}: department groups");
        assert_eq!(
            departments
                .iter()
                .map(|department| department["Person"].as_array().expect("person array").len())
                .sum::<usize>(),
            30,
            "{sample}: mapped people"
        );
    }
    if sample == "GroupTemperaturesByYear.mfd" {
        let years = expected_json["YearlyStats"]
            .as_array()
            .expect("yearly temperature groups");
        assert_eq!(years.len(), 5, "{sample}: one group per year");
        assert_eq!(
            years
                .iter()
                .map(|year| year["Year"].as_i64())
                .collect::<Vec<_>>(),
            vec![Some(2006), Some(2007), Some(2008), Some(2009), Some(2010)],
            "{sample}: groups retain first-seen year order"
        );
        assert_eq!(years[0]["MinimumTemp"], -3.6);
        assert_eq!(years[0]["MaximumTemp"], 23.2);
        assert_eq!(years[0]["AverageTemp"], 11.375);
    }
    if sample == "SortByMultipleKeys.mfd" {
        assert_eq!(expected_json["Name"], "Share Ranking");
        let offices = expected_json["Office"].as_array().expect("office array");
        assert_eq!(offices.len(), 1, "{sample}: source has one office");
        let people = offices[0]["Person"].as_array().expect("person array");
        assert_eq!(people.len(), 16, "{sample}: all department members");
        assert_eq!(people[0]["Shares"], 2000);
        assert_eq!(people[0]["Last"], "Landis");
        assert_eq!(people[1]["Shares"], 2000);
        assert_eq!(people[1]["Last"], "Martin");
        assert_eq!(people[2]["Last"], "Martin");
        assert_eq!(people[3]["Last"], "Martin");
        assert_eq!(people[1]["First"]["#text"], "Alex");
        assert_eq!(people[2]["First"]["#text"], "Joe");
        assert_eq!(people[3]["First"]["#text"], "Susan");
        assert_eq!(people[4]["Shares"], 1500);
        assert_eq!(people[4]["Last"], "Butler");
        assert_eq!(people[5]["Shares"], 1500);
        assert_eq!(people[5]["Last"], "Callaby");
    }
    if sample == "FindHighestTemperatures.mfd" {
        let data = expected_json["data"]
            .as_array()
            .expect("selected temperatures");
        assert_eq!(data.len(), 10, "{sample}: top ten temperatures");
        assert_eq!(
            data.iter()
                .map(|item| item["temp"].as_f64().expect("numeric temperature"))
                .collect::<Vec<_>>(),
            vec![24.0, 23.8, 23.2, 22.7, 22.3, 22.3, 21.5, 21.4, 21.1, 20.7],
            "{sample}: selected temperatures retain descending order"
        );
        assert_eq!(data[0]["month"], "2008-07");
        assert_eq!(data[9]["month"], "2007-08");
    }
    if sample == "PositionInFilteredSequence.mfd" {
        let contacts = expected_json["Contact"]
            .as_array()
            .expect("filtered contacts");
        assert_eq!(contacts.len(), 8, "{sample}: last names after M");
        assert_eq!(
            contacts
                .iter()
                .map(|contact| contact["ID"].as_str().expect("position ID").to_owned())
                .collect::<Vec<_>>(),
            (1..=8).map(|id| id.to_string()).collect::<Vec<_>>(),
            "{sample}: filtered positions are compact and one-based"
        );
        assert_eq!(contacts[0]["First"], "Loby");
        assert_eq!(contacts[0]["Last"], "Matise");
        assert_eq!(contacts[7]["First"], "Mark");
        assert_eq!(contacts[7]["Last"], "Redgreen");
    }

    let generated_input = case_dir.join("source.json");
    std::fs::write(&generated_input, source_json)?;
    let project_path = case_dir.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let rust_output = case_dir.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        r#"fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_path = std::env::args_os().nth(1).expect("input path");
    let input = std::fs::read_to_string(input_path)?;
    print!("{}", ferrule_generated_mapping::execute_json(&input)?);
    Ok(())
}
"#,
    )?;
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let rust_run = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .arg(&generated_input)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    let rust_json: serde_json::Value = serde_json::from_slice(&rust_run.stdout)?;
    assert_eq!(
        rust_json, expected_json,
        "{sample}: generated Rust differs from engine"
    );

    let csharp_output = case_dir.join("csharp");
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
        r#"using Ferrule.Generated;

var input = File.ReadAllText(args[0]);
Console.Out.Write(GeneratedMapping.ExecuteJson(input));
"#,
    )?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let csharp_run = dotnet_command(&csharp_output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--no-build",
            "--no-restore",
            "--",
        ])
        .arg(&generated_input)
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    let csharp_json: serde_json::Value = serde_json::from_slice(&csharp_run.stdout)?;
    assert_eq!(
        csharp_json, expected_json,
        "{sample}: generated C# differs from engine"
    );
    println!("{sample}: generated Rust and C# match the interpreter");
    Ok(())
}
