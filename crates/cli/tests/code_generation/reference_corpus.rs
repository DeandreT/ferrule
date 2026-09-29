//! Opt-in generated-backend execution against thirteen local, gitignored mappings.
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
    Protobuf,
}

#[derive(Clone, Copy)]
enum TargetKind {
    Json,
    Xml,
    Csv,
}

struct CorpusCase {
    sample: &'static str,
    input: &'static str,
    source_kind: SourceKind,
    target_kind: TargetKind,
}

const CASES: [CorpusCase; 13] = [
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
    CorpusCase {
        sample: "StringJoin.mfd",
        input: "BranchOffices.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "ClassifyTemperatures.mfd",
        input: "Temperatures.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "TokenizeString2.mfd",
        input: "AltovaTools.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "Tutorial/JoinPeopleInfo.mfd",
        input: "Tutorial/People.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "Tutorial/ReadProtocolBuffers.mfd",
        input: "Tutorial/assets.bin",
        source_kind: SourceKind::Protobuf,
        target_kind: TargetKind::Csv,
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
    let named_input = (sample == "Tutorial/JoinPeopleInfo.mfd")
        .then_some(("Addresses", "Tutorial/Addresses.xml"));
    assert_eq!(
        project.extra_sources.len(),
        usize::from(named_input.is_some()),
        "{sample}: unexpected named inputs"
    );
    assert!(
        match case.target_kind {
            TargetKind::Json => project.target_options.json_document,
            TargetKind::Xml => project.target_options.xml_document,
            TargetKind::Csv => {
                project.target_options.tabular_kind == Some(mapping::TabularBoundaryKind::Csv)
            }
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
            SourceKind::Protobuf => project.source_options.protobuf.is_some(),
        },
        "{sample}: unexpected input format"
    );
    assert_eq!(
        project
            .source_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(OsStr::to_str),
        Path::new(case.input).file_name().and_then(OsStr::to_str),
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
        SourceKind::Protobuf => {
            let options = project.source_options.protobuf.as_ref().unwrap();
            assert!(options.imports.is_empty(), "{sample}: protobuf imports");
            let layout = format_protobuf::Layout::parse(&options.schema)?;
            format_protobuf::read(&input_path, &layout, &options.root_message)?
        }
    };
    // Generated hosts expose a schema-shaped JSON API. Preserve native XML,
    // FlexText, and CSV readers' typed instances while crossing that host API.
    let source_json = match case.source_kind {
        SourceKind::Json => std::fs::read_to_string(&input_path)?,
        SourceKind::Xml | SourceKind::FlexText | SourceKind::Csv | SourceKind::Protobuf => {
            format_json::to_string(&project.source, &source)?
        }
    };
    let mut named_sources = Vec::new();
    let mut named_input_paths = Vec::new();
    if let Some((name, input)) = named_input {
        let named_source = &project.extra_sources[0];
        assert_eq!(named_source.name, name, "{sample}: named input identity");
        assert_eq!(
            Path::new(&named_source.path)
                .file_name()
                .and_then(OsStr::to_str),
            Path::new(input).file_name().and_then(OsStr::to_str),
            "{sample}: named input instance"
        );
        assert!(
            named_source.options.xml_document,
            "{sample}: named input format"
        );
        assert!(
            named_source.dynamic_path.is_none(),
            "{sample}: static named input"
        );
        let instance = format_xml::read(&samples.join(input), &named_source.schema)?;
        let named_json = format_json::to_string(&named_source.schema, &instance)?;
        let named_path = case_dir.join("named-source.json");
        std::fs::write(&named_path, named_json)?;
        named_sources.push((name.to_owned(), instance));
        named_input_paths.push((name, named_path));
    }
    let expected = if named_sources.is_empty() {
        engine::run(&project, &source)?
    } else {
        engine::run_with_sources(&project, &source, named_sources)?
    };
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
    if sample == "StringJoin.mfd" {
        let info = expected_json["Info"].as_array().expect("one joined notice");
        assert_eq!(info.len(), 1, "{sample}: one notice");
        let title = info[0]["Title"].as_str().expect("joined title");
        assert!(
            title.starts_with("Dear Vernon, Frank, Loby, "),
            "{sample}: {title}"
        );
        assert!(
            title.ends_with(", Valentin, Carl, Mark"),
            "{sample}: {title}"
        );
        assert_eq!(title.split(", ").count(), 21, "{sample}: joined contacts");
        assert_eq!(info[0]["Description"]["#text"], "You are all promoted.");
    }
    if sample == "ClassifyTemperatures.mfd" {
        let data = expected_json["data"]
            .as_array()
            .expect("classified readings");
        assert_eq!(data.len(), 60, "{sample}: all readings retain source order");
        assert_eq!(data[0]["month"], "2006-01");
        assert_eq!(data[0]["desc"], "low");
        assert_eq!(data[30]["month"], "2008-07");
        assert_eq!(data[30]["desc"], "high");
        assert_eq!(data[32]["month"], "2008-09");
        assert_eq!(data[32]["temp"], 20.0);
        assert!(
            data[32].get("desc").is_none(),
            "{sample}: threshold 20 has no class"
        );
        assert_eq!(data[59]["month"], "2010-12");
        assert_eq!(data[59]["desc"], "low");
        assert_eq!(
            data.iter()
                .filter(|reading| reading["desc"] == "low")
                .count(),
            16,
            "{sample}: low classifications"
        );
        assert_eq!(
            data.iter()
                .filter(|reading| reading["desc"] == "high")
                .count(),
            11,
            "{sample}: high classifications"
        );
        assert_eq!(
            data.iter()
                .filter(|reading| reading.get("desc").is_none())
                .count(),
            33,
            "{sample}: unclassified middle readings"
        );
    }
    if sample == "TokenizeString2.mfd" {
        let rows = expected_json.as_array().expect("concatenated CSV rows");
        assert_eq!(rows.len(), 10, "{sample}: heading and nine tools");
        assert_eq!(
            rows.iter()
                .enumerate()
                .filter_map(|(index, row)| (index != 2).then_some(row["Tool"].as_str()))
                .collect::<Vec<_>>(),
            vec![
                Some("Tool"),
                Some("XMLSpy"),
                Some("StyleVision"),
                Some("UModel"),
                Some("DatabaseSpy"),
                Some("DiffDog"),
                Some("SchemaAgent"),
                Some("SemanticWorks"),
                Some("Authentic"),
            ],
            "{sample}: ordered source tools"
        );
        assert!(
            rows[2]["Tool"]
                .as_str()
                .is_some_and(|tool| !tool.is_empty())
        );
        assert_eq!(
            rows.iter()
                .map(|row| row["ExistsInMissionKit"].as_str())
                .collect::<Vec<_>>(),
            vec![
                Some("MissionKit for Enterprise XML Developers"),
                Some("Y"),
                Some("Y"),
                Some("Y"),
                Some("N"),
                Some("N"),
                Some("Y"),
                Some("Y"),
                Some("Y"),
                Some("N"),
            ],
            "{sample}: lookup-fed token existence"
        );
    }
    if sample == "Tutorial/JoinPeopleInfo.mfd" {
        let rows = expected_json["Row"].as_array().expect("joined people");
        assert_eq!(rows.len(), 3, "{sample}: only matched people remain");
        let mut keys = std::collections::BTreeSet::new();
        for row in rows {
            let first = row["FirstName"].as_str().expect("joined first name");
            let last = row["LastName"].as_str().expect("joined last name");
            assert!(!first.is_empty() && !last.is_empty());
            assert!(
                keys.insert((first, last)),
                "{sample}: joined names are distinct"
            );
            assert!(row["City"].as_str().is_some_and(|city| !city.is_empty()));
            assert!(
                row["Street"]
                    .as_str()
                    .is_some_and(|street| !street.is_empty())
            );
            assert!(row["Email"].as_str().is_some_and(|email| !email.is_empty()));
        }
    }
    if sample == "Tutorial/ReadProtocolBuffers.mfd" {
        let rows = expected_json.as_array().expect("CSV painting rows");
        assert_eq!(rows.len(), 7, "{sample}: one row per protobuf painting");
        assert!(rows.iter().all(|row| {
            ["Name", "Period", "Dimensions", "Format", "Location"]
                .into_iter()
                .all(|field| row[field].as_str().is_some_and(|value| !value.is_empty()))
        }));
        assert_eq!(rows[0]["Dimensions"], "61.4 in x 67.8 in");
        assert_eq!(rows[6]["Dimensions"], "11 in x 51.2 in");
        assert_eq!(
            rows.iter()
                .filter(|row| row["Location"] == "Museum")
                .count(),
            4,
            "{sample}: first value-map branch"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row["Location"] == "Temple")
                .count(),
            1,
            "{sample}: second value-map branch"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row["Location"] == "Private collection")
                .count(),
            2,
            "{sample}: third value-map branch"
        );
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
        r#"use ferrule_generated_mapping::{NamedJsonInput, execute_json_with_sources};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input_path = args.next().expect("input path");
    let input = std::fs::read_to_string(input_path)?;
    let named_args: Vec<_> = args.collect();
    assert_eq!(named_args.len() % 2, 0, "named input arguments are pairs");
    let named_documents = named_args
        .chunks_exact(2)
        .map(|pair| {
            Ok((
                pair[0].to_string_lossy().into_owned(),
                std::fs::read_to_string(&pair[1])?,
            ))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    let named_inputs = named_documents
        .iter()
        .map(|(name, document)| NamedJsonInput { name, document })
        .collect::<Vec<_>>();
    print!("{}", execute_json_with_sources(&input, &named_inputs)?);
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
    let mut rust_run_command = Command::new("cargo");
    rust_run_command
        .args(["run", "--quiet", "--"])
        .arg(&generated_input)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target);
    for (name, path) in &named_input_paths {
        rust_run_command.arg(name).arg(path);
    }
    let rust_run = rust_run_command.isolated_output()?;
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
if ((args.Length - 1) % 2 != 0)
{
    throw new ArgumentException("Named input arguments are pairs.");
}
var namedInputs = new List<NamedJsonInput>();
for (var index = 1; index < args.Length; index += 2)
{
    namedInputs.Add(new NamedJsonInput(args[index], File.ReadAllText(args[index + 1])));
}
Console.Out.Write(GeneratedMapping.ExecuteJsonWithSources(input, namedInputs));
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
    let mut csharp_run_command = dotnet_command(&csharp_output);
    csharp_run_command
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
        .current_dir(&csharp_output);
    for (name, path) in &named_input_paths {
        csharp_run_command.arg(name).arg(path);
    }
    let csharp_run = csharp_run_command.isolated_output()?;
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
