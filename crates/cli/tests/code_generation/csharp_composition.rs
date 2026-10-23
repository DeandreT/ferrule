//! Every C# emission route, applied to the owned conformance corpora and the
//! authored generation fixtures, against `fixtures/csharp_composition.txt`.
//!
//! Each line records one route for one subject: `ok:<hash> files=<n>` over every
//! emitted path and the digest of its bytes (runtime sources included), with
//! separate digests of the mapping source and the project file, or `err:<hash>`
//! of the complete refusal. The `owned` line combines every owned preset's
//! C# result for the subject.
//! Saved projects run through the public generation entry points of every C#
//! profile, so a refusal also records the order in which it was reached. Set
//! `FERRULE_UPDATE_CSHARP_COMPOSITION=1` to rewrite the record after an intended
//! emission change.
use super::owned_capability_presets::{Fnv, Preset, Subject, corpora, digest, presets};
use super::*;
use cli::GenerateProfile;
use mapping::FormatOptions;
use std::fmt::{Debug, Write as _};
use std::fs;

const RECORD: &str = "code_generation/fixtures/csharp_composition.txt";
const UPDATE: &str = "FERRULE_UPDATE_CSHARP_COMPOSITION";

/// User text resembling the class header, the project compile entry and the
/// generated node declarations that emitted C# is composed from.
const MARKERS: &str = "public static class GeneratedMapping\n{\n    <Compile Include=\"GeneratedMapping.cs\" />\n\n    private static global::Ferrule.Runtime.FerruleValue Node_1(\n    private static global::Ferrule.Runtime.FerruleValue UserFunction_1_Node_\n}\n";

const PROFILES: [(&str, GenerateProfile); 9] = [
    ("ordinary", GenerateProfile::Ordinary),
    ("csv_output", GenerateProfile::CsvOutput),
    ("csv_document", GenerateProfile::CsvDocument),
    ("raw_delimited", GenerateProfile::RawDelimitedDocument),
    ("json5", GenerateProfile::Json5),
    (
        "x12",
        GenerateProfile::X12 {
            source_envelope: None,
            target_envelope: None,
        },
    ),
    (
        "static_documents",
        GenerateProfile::StaticDocuments {
            source_envelope: None,
            target_envelope: None,
        },
    ),
    (
        "csv_x12",
        GenerateProfile::CsvX12 {
            target_envelope: None,
        },
    ),
    ("csv_json", GenerateProfile::CsvJson),
];

/// Digests of fixed runtime sources, which recur unchanged in every library.
static RUNTIME_DIGESTS: Mutex<BTreeMap<String, (Vec<u8>, String)>> = Mutex::new(BTreeMap::new());

fn file_digest(path: &str, bytes: &[u8]) -> String {
    if !path.starts_with("Runtime/") {
        return Fnv::new().bytes(bytes).finish();
    }
    let mut digests = RUNTIME_DIGESTS.lock().expect("runtime digests");
    match digests.get(path) {
        Some((seen, digest)) if seen.as_slice() == bytes => digest.clone(),
        _ => {
            let digest = Fnv::new().bytes(bytes).finish();
            digests.insert(path.to_owned(), (bytes.to_vec(), digest.clone()));
            digest
        }
    }
}

fn files(mut files: Vec<(String, Vec<u8>)>) -> String {
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut all = Fnv::new();
    let named = |path: &str| {
        files
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map_or_else(|| "-".into(), |(path, bytes)| file_digest(path, bytes))
    };
    for (path, bytes) in &files {
        all.bytes(path.as_bytes())
            .bytes(&[0])
            .bytes(file_digest(path, bytes).as_bytes())
            .bytes(&[0]);
    }
    format!(
        "ok:{} files={} mapping={} project={}",
        all.finish(),
        files.len(),
        named("GeneratedMapping.cs"),
        named("Ferrule.Generated.csproj"),
    )
}

fn artifacts<T: Artifacts, E: Debug>(result: Result<T, E>) -> String {
    match result {
        Ok(set) => set.record(),
        Err(error) => {
            let debug = format!("{error:?}");
            let summary = debug
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .find(|word| !word.is_empty())
                .unwrap_or_default()
                .to_owned();
            format!("err:{} {summary}", digest(&debug))
        }
    }
}

trait Artifacts {
    fn record(self) -> String;
}

impl Artifacts for codegen::ArtifactSet {
    fn record(self) -> String {
        files(
            self.into_files()
                .into_iter()
                .map(|file| (file.path.as_str().to_owned(), file.contents))
                .collect(),
        )
    }
}

/// A refusal-only result has nothing to record when it succeeds.
impl Artifacts for () {
    fn record(self) -> String {
        "ok".into()
    }
}

/// One saved project through every public C# generation profile.
fn generated(directory: &Path, project: &Path) -> Vec<(&'static str, String)> {
    PROFILES
        .iter()
        .map(|(name, profile)| {
            let output = directory.join(name);
            let result = cli::generate_project_with_profile(
                project,
                &output,
                GenerateTarget::CSharp,
                *profile,
            );
            let line = match result {
                Ok(_) => match artifact_files(&output) {
                    Ok(files_) => files(files_),
                    Err(error) => format!("unreadable:{error}"),
                },
                Err(error) => {
                    // The complete cause chain, without host paths.
                    let text = format!("{error:#}")
                        .replace(&directory.display().to_string(), "<output>")
                        .replace(&project.display().to_string(), "<project>");
                    let summary = text
                        .split_whitespace()
                        .take(6)
                        .collect::<Vec<_>>()
                        .join("_");
                    format!("err:{} {summary}", digest(&text))
                }
            };
            let _ = fs::remove_dir_all(&output);
            (*name, line)
        })
        .collect()
}

/// Program-level emitters for one conformance subject. A Project subject
/// uses the Program of the first preset lowering that admits it.
fn emitted(presets: &[Preset], subject: &Subject) -> Vec<(&'static str, String)> {
    let (program, project) = match subject {
        Subject::Program(program) => ((**program).clone(), None),
        Subject::Project(project) => {
            match presets
                .iter()
                .find_map(|preset| (preset.lower)(project).ok())
            {
                Some(program) => (program, Some(project.as_ref())),
                None => return vec![("lower", artifacts(codegen::lower(project).map(|_| ())))],
            }
        }
    };
    let target_options = project.map(|project| &project.target_options);
    let csv_output = codegen::CsvOutputPolicy::from_format_options(
        target_options.unwrap_or(&FormatOptions::default()),
    );
    let csv_document = project.map_or_else(
        || Ok(codegen::CsvDocumentBoundaryPolicy::default()),
        codegen::CsvDocumentBoundaryPolicy::from_project,
    );
    // Every owned preset's C# library, combined into one line.
    let mut owned = Fnv::new();
    for preset in presets {
        owned
            .bytes(artifacts((preset.csharp)(&program)).as_bytes())
            .bytes(&[0]);
    }
    vec![
        ("owned", format!("all:{}", owned.finish())),
        ("emit", artifacts(codegen_csharp::emit(&program))),
        (
            "emit_typed_only",
            artifacts(codegen_csharp::emit_with_companions(
                &program,
                codegen::CompanionSelection::TypedOnly,
            )),
        ),
        (
            "json5",
            artifacts(codegen_csharp::emit_with_json5(&program)),
        ),
        (
            "csv_output",
            match csv_output {
                Ok(policy) => artifacts(codegen_csharp::emit_with_csv_output(&program, &policy)),
                Err(error) => artifacts::<codegen::ArtifactSet, _>(Err(error)),
            },
        ),
        (
            "csv_document",
            match csv_document {
                Ok(policy) => artifacts(codegen_csharp::emit_with_csv_document_adapters(
                    &program, &policy,
                )),
                Err(error) => artifacts::<codegen::ArtifactSet, _>(Err(error)),
            },
        ),
    ]
}

fn document_subjects() -> Vec<(String, Vec<(&'static str, String)>)> {
    let mut subjects = Vec::new();
    for (name, build) in [
        (
            "primary_filenames",
            super::primary_filenames::document as fn(bool) -> mapping::DocumentProject,
        ),
        (
            "primary_csv_acquisition",
            super::primary_csv_acquisition::document,
        ),
    ] {
        for literal in [false, true] {
            let document = build(literal);
            let lines = match codegen::lower_document(&document) {
                Err(error) => vec![("lower", artifacts::<codegen::ArtifactSet, _>(Err(error)))],
                Ok(program) => {
                    let mut lines = vec![(
                        "primary_filename",
                        artifacts(codegen_csharp::emit_with_primary_filename(&program)),
                    )];
                    for (label, preserve) in [("primary_csv", false), ("primary_csv_empty", true)] {
                        let policy = codegen::CsvInputPolicy {
                            preserve_empty_strings: preserve,
                            ..Default::default()
                        };
                        lines.push((
                            label,
                            artifacts(codegen_csharp::emit_with_primary_csv_acquisition(
                                &program, &policy,
                            )),
                        ));
                    }
                    lines
                }
            };
            subjects.push((format!("documents/{name}/{literal}"), lines));
        }
    }
    subjects
}

/// The CSV fixture with literal and parameter-name text that resembles the
/// composed C# parts. Old textual composition matched these shapes.
fn marker_project(options: FormatOptions) -> Project {
    let mut project = super::csv_output::csv_project(options);
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String(MARKERS.into()),
        },
    );
    if let Some(Node::RuntimeParameterDefault { name, .. }) = project.graph.nodes.get_mut(&10) {
        *name = MARKERS.replace('\n', " ");
    }
    project
}

/// A JSON mapping whose field names and constant resemble the composed parts.
fn json_marker_project() -> Project {
    let source_field = "\n    private static global::Ferrule.Runtime.FerruleValue Node_1(";
    let target_field = "public static class GeneratedMapping\n{";
    Project {
        source: SchemaNode::group("Input", vec![string(source_field)]),
        target: SchemaNode::group("Output", vec![string(target_field), string("Copied")]),
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::Const {
                        value: Value::String(MARKERS.into()),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec![source_field.into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: target_field.into(),
                    node: 1,
                },
                Binding {
                    target_field: "Copied".into(),
                    node: 2,
                },
            ],
            ..Default::default()
        },
    }
}

fn x12_segment(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| string(&format!("{name}{index:02}")))
            .collect(),
    )
}

/// An X12 004010 interchange copied through a mapping between the given
/// endpoint paths; the note segment carries the marker text.
fn x12_project(source_path: &str, target_path: &str) -> Project {
    let fixed = |mut segment: SchemaNode, index: usize, value: &str| {
        if let ir::SchemaKind::Group { children, .. } = &mut segment.kind {
            children[index].fixed = Some(value.into());
        }
        segment
    };
    let schema = SchemaNode::group(
        "Interchange",
        vec![
            fixed(x12_segment("ISA", 16), 11, "00401"),
            fixed(x12_segment("GS", 8), 7, "004010"),
            fixed(x12_segment("ST", 2), 0, "940"),
            x12_segment("NTE", 2),
            x12_segment("SE", 2),
            x12_segment("GE", 2),
            x12_segment("IEA", 2),
        ],
    );
    Project {
        source: schema.clone(),
        target: schema,
        source_path: Some(source_path.into()),
        target_path: Some(target_path.into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    }
}

/// Saved-project subjects: authored in-memory fixtures and every tracked
/// project file of the generation tests, the codegen boundaries and examples.
fn saved_subjects() -> TestResult<Vec<(String, Project)>> {
    let mut subjects = Vec::new();
    let csv_options = [
        ("default", FormatOptions::default()),
        (
            "custom",
            FormatOptions {
                delimiter: Some(';'),
                csv_quote: Some('\''),
                csv_utf8_bom: true,
                ..Default::default()
            },
        ),
        (
            "disabled",
            FormatOptions {
                delimiter: Some('\t'),
                csv_quote_disabled: true,
                has_header_row: Some(false),
                ..Default::default()
            },
        ),
    ];
    for (mode, options) in &csv_options {
        subjects.push((
            format!("csv_output/{mode}"),
            super::csv_output::csv_project(options.clone()),
        ));
        subjects.push((
            format!("csv_output/named/{mode}"),
            super::csv_output::named_csv_project(options.clone()),
        ));
        subjects.push((format!("markers/{mode}"), marker_project(options.clone())));
    }
    subjects.push(("markers/json".into(), json_marker_project()));
    for (source, target) in [
        ("input.x12", "output.x12"),
        ("input.json", "output.x12"),
        ("input.x12", "output.json"),
    ] {
        subjects.push((
            format!("x12/{source}/{target}"),
            x12_project(source, target),
        ));
    }
    for mixed in [false, true] {
        subjects.push((
            format!("csv_output/dynamic/{mixed}"),
            super::csv_output::dynamic::dynamic_csv_project(mixed),
        ));
    }
    for grouped in [false, true] {
        for unquoted in [false, true] {
            subjects.push((
                format!("csv_document/{grouped}/{unquoted}"),
                super::csv_document_adapters::document_project(grouped, unquoted),
            ));
        }
    }
    subjects.push((
        "csv_row_provenance".into(),
        super::csv_row_provenance::project(),
    ));
    for fixture in super::json5_companions::projects::FIXTURES {
        subjects.push((
            format!("json5/{fixture}"),
            super::json5_companions::projects::project(fixture),
        ));
    }
    for (id, project, _) in super::filter_map_csharp::fixtures() {
        subjects.push((format!("filter_map/{id}"), project));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for directory in [
        "crates/cli/tests/code_generation",
        "crates/cli/tests/fixtures",
        "crates/cli/tests/csv_x12_native",
        "crates/codegen/src",
        "conformance",
        "examples",
    ] {
        collect_json(&root.join(directory), &mut paths)?;
    }
    paths.sort();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(project) = mapping::project_file::decode_str(&text) {
            let relative = path
                .strip_prefix(&root)?
                .to_string_lossy()
                .replace('\\', "/");
            subjects.push((relative, project));
        }
    }
    Ok(subjects)
}

fn collect_json(directory: &Path, paths: &mut Vec<PathBuf>) -> TestResult<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_json(&path, paths)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn block(title: &str, lines: Vec<(&'static str, String)>) -> String {
    let mut output = format!("\n## {title}\n");
    for (family, line) in lines {
        let _ = writeln!(output, "{family:<16} {line}");
    }
    output
}

/// Records `jobs` on a few threads, keeping their order.
fn parallel<T: Sync>(jobs: &[T], run: impl Fn(usize, &T) -> String + Sync) -> Vec<String> {
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get().min(4));
    let next = AtomicUsize::new(0);
    let blocks = Mutex::new(vec![String::new(); jobs.len()]);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(index) else {
                        break;
                    };
                    let block = run(index, job);
                    blocks.lock().expect("record blocks")[index] = block;
                }
            });
        }
    });
    blocks.into_inner().expect("record blocks")
}

fn render() -> TestResult<String> {
    let mut output = String::from(
        "# C# emission routes over the owned conformance corpora and the authored\n\
         # generation fixtures. Each line: route, then the result.\n",
    );
    let corpora = corpora()?;
    let subjects = corpora
        .iter()
        .flat_map(|(corpus, subjects)| {
            subjects
                .iter()
                .map(move |(id, subject)| (format!("{corpus}/{id}"), subject))
        })
        .collect::<Vec<_>>();
    let presets = presets();
    for block_ in parallel(&subjects, |_, (title, subject)| {
        block(title, emitted(&presets, subject))
    }) {
        output.push_str(&block_);
    }
    for (title, lines) in document_subjects() {
        output.push_str(&block(&title, lines));
    }
    let directory = TempDir::new("csharp_composition")?;
    let saved = saved_subjects()?;
    let blocks = parallel(&saved, |index, (title, project)| {
        let root = directory.0.join(index.to_string());
        let lines = match fs::create_dir(&root)
            .map_err(|error| error.to_string())
            .and_then(|()| {
                let path = root.join("project.json");
                mapping::project_file::encode_pretty(project)
                    .map_err(|error| error.to_string())
                    .and_then(|text| fs::write(&path, text).map_err(|error| error.to_string()))
                    .map(|()| path)
            }) {
            Ok(path) => generated(&root, &path),
            Err(error) => vec![("save", format!("unsaved:{error}"))],
        };
        let _ = fs::remove_dir_all(&root);
        block(&format!("saved/{title}"), lines)
    });
    for block_ in blocks {
        output.push_str(&block_);
    }
    Ok(output)
}

#[test]
fn csharp_composition_matches_recorded_artifacts() -> TestResult<()> {
    let actual = render()?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(RECORD);
    if std::env::var_os(UPDATE).is_some() {
        fs::write(&path, &actual)?;
        return Ok(());
    }
    let expected = fs::read_to_string(&path)?;
    if actual != expected {
        let mismatches = actual
            .lines()
            .zip(expected.lines())
            .enumerate()
            .filter(|(_, (actual, expected))| actual != expected)
            .take(20)
            .map(|(line, (actual, expected))| {
                format!(
                    "line {}:\n  actual   {actual}\n  recorded {expected}",
                    line + 1
                )
            })
            .collect::<Vec<_>>();
        return Err(format!(
            "C# emission differs from {RECORD} ({} actual vs {} recorded lines):\n{}",
            actual.lines().count(),
            expected.lines().count(),
            mismatches.join("\n")
        )
        .into());
    }
    Ok(())
}

/// The JSON marker mapping, composed with the JSON5 companion, compiles and
/// returns every marker field name and value unchanged through both the
/// ordinary JSON method and the companion.
#[test]
#[ignore = "builds and runs a generated C# host with the .NET SDK"]
fn csharp_composed_marker_library_compiles_and_preserves_text() -> TestResult<()> {
    let directory = TempDir::new("csharp_composition_markers")?;
    let project = json_marker_project();
    let path = directory.0.join("project.json");
    fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let output = directory.0.join("generated");
    cli::generate_project_with_json5_adapters(&path, &output, GenerateTarget::CSharp)?;
    assert!(output.join("GeneratedMapping.Json5.cs").exists());

    let source_field = "\n    private static global::Ferrule.Runtime.FerruleValue Node_1(";
    let target_field = "public static class GeneratedMapping\n{";
    let input = serde_json::json!({ source_field: MARKERS }).to_string();
    fs::write(output.join("input.json"), &input)?;
    let harness = output.join("Harness");
    fs::create_dir(&harness)?;
    fs::write(
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
    fs::write(
        harness.join("Program.cs"),
        r#"using Ferrule.Generated;

var input = File.ReadAllText(args[0]);
File.WriteAllText(args[1], GeneratedMapping.ExecuteJson(input));
File.WriteAllText(args[2], GeneratedMapping.ExecuteJson5(input));
"#,
    )?;
    let command = dotnet_command(&output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--",
            "input.json",
            "json.txt",
            "json5.txt",
        ])
        .current_dir(&output)
        .isolated_output()?;
    assert!(
        command.status.success(),
        "generated C# marker host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    let expected = serde_json::json!({ target_field: MARKERS, "Copied": MARKERS });
    for name in ["json.txt", "json5.txt"] {
        let actual: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(output.join(name))?)?;
        assert_eq!(actual, expected, "{name}");
    }
    Ok(())
}
