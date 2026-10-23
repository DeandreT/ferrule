//! Boundary emission over the owned capability corpora, the XML boundary
//! fixtures and directed XML refusal variants, against the results recorded in
//! `fixtures/boundary_emission_plan.txt`.
//!
//! Each line records one family for one subject: lowering, Program validation,
//! the proved XML output mode and prepared boundary plan, ordinary and
//! typed-only Rust and C# emission with their public API inventories, and the
//! companion emitters layered on ordinary emission.
//! Results use the owned capability preset encoding: `ok`, `ok:<hash>` of the
//! lowered Program or artifact bytes, or `err:<hash>` of the complete
//! diagnostic `Debug` with a short summary. Set
//! `FERRULE_UPDATE_BOUNDARY_EMISSION_PLAN=1` to rewrite the record after an
//! intended change.
use super::owned_capability_presets::{Corpus, Subject, accepted, artifacts, digest};
use super::*;
use codegen::CompanionSelection;
use std::fmt::Write as _;
use std::fs;

const RECORD: &str = "code_generation/fixtures/boundary_emission_plan.txt";
const UPDATE: &str = "FERRULE_UPDATE_BOUNDARY_EMISSION_PLAN";

fn options() -> codegen_rust::Options {
    codegen_rust::Options {
        package_name: "boundary-emission-plan".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path("codegen-runtime".into()),
    }
}

/// Public entry points and types of one generated library, in source order.
fn api(set: &codegen::ArtifactSet) -> String {
    let mut names = Vec::new();
    for file in set.files() {
        let path = file.path.as_str();
        if path != "src/lib.rs" && path != "GeneratedMapping.cs" {
            continue;
        }
        let text = String::from_utf8_lossy(&file.contents);
        for line in text.lines() {
            let line = line.trim_start();
            let declaration = ["pub fn ", "pub struct ", "pub enum ", "pub const "]
                .iter()
                .chain(&["public static ", "public sealed record "])
                .find_map(|prefix| line.strip_prefix(prefix));
            let Some(declaration) = declaration else {
                continue;
            };
            let head = declaration
                .split(['(', '<', ':', '{', '='])
                .next()
                .unwrap_or_default();
            if let Some(name) = head.split_whitespace().last() {
                names.push(name.to_owned());
            }
        }
    }
    format!("ok:{} n={}", digest(&names.join("\n")), names.len())
}

type Family = (&'static str, String);

fn emission_families(program: &codegen::Program) -> Vec<Family> {
    let options = options();
    let mut families = vec![
        (
            "validate",
            accepted(&codegen::validate_program(program), |_| "ok".into()),
        ),
        (
            "xml_mode",
            accepted(&program.xml_output_mode(), |mode| format!("ok:{mode:?}")),
        ),
    ];
    families.push((
        "plan",
        accepted(
            &codegen::prepare_boundary_emission(program, CompanionSelection::Complete),
            |plan| match plan.xml() {
                Some(xml) => format!(
                    "ok:{:?}/{:?}/{:?}",
                    xml.mode(),
                    xml.inputs(),
                    xml.named_outputs()
                ),
                None => "ok:None".into(),
            },
        ),
    ));
    let rust = codegen_rust::emit(program, &options);
    families.push(("rust", accepted(&rust, artifacts)));
    families.push(("rust_api", accepted(&rust, api)));
    let csharp = codegen_csharp::emit(program);
    families.push(("csharp", accepted(&csharp, artifacts)));
    families.push(("csharp_api", accepted(&csharp, api)));
    let rust = codegen_rust::emit_with_companions(program, &options, CompanionSelection::TypedOnly);
    families.push(("rust_typed", accepted(&rust, artifacts)));
    families.push(("rust_t_api", accepted(&rust, api)));
    let csharp = codegen_csharp::emit_with_companions(program, CompanionSelection::TypedOnly);
    families.push(("csharp_typed", accepted(&csharp, artifacts)));
    families.push(("csharp_t_api", accepted(&csharp, api)));
    let csv = codegen::CsvOutputPolicy::default();
    families.push((
        "rust_csv",
        accepted(
            &codegen_rust::emit_with_csv_output(program, &options, &csv),
            artifacts,
        ),
    ));
    families.push((
        "csharp_csv",
        accepted(
            &codegen_csharp::emit_with_csv_output(program, &csv),
            artifacts,
        ),
    ));
    families.push((
        "rust_json5",
        accepted(&codegen_rust::emit_with_json5(program, &options), artifacts),
    ));
    families.push((
        "csharp_json5",
        accepted(&codegen_csharp::emit_with_json5(program), artifacts),
    ));
    families
}

fn subject_block(corpus: &str, id: &str, subject: &Subject) -> String {
    let (kind, families) = match subject {
        Subject::Program(program) => ("program", emission_families(program)),
        Subject::Project(project) => {
            let lowered = codegen::lower(project);
            let mut families = vec![(
                "lower",
                accepted(&lowered, |program| {
                    format!("ok:{}", digest(&format!("{program:?}")))
                }),
            )];
            if let Ok(program) = &lowered {
                families.extend(emission_families(program));
            }
            ("project", families)
        }
    };
    let mut output = format!("\n## {corpus}/{id} ({kind})\n");
    for (family, result) in families {
        let _ = writeln!(output, "{family:<12} {result}");
    }
    output
}

fn xml_fixture_corpus() -> TestResult<Corpus> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen/src/tests/fixtures");
    let mut paths = fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "json")
    });
    paths.sort();
    let mut corpus = Vec::new();
    for path in paths {
        let id = path
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or("fixture name")?
            .to_owned();
        let project: Project = serde_json::from_str(&fs::read_to_string(&path)?)?;
        corpus.push((id, Subject::Project(Box::new(project))));
    }
    Ok(corpus)
}

/// Project-level XML format requests that lowering admits, narrows or refuses.
fn xml_request_corpus(fixtures: &Corpus) -> Corpus {
    let xml = || mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    let root_view = || mapping::FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: true,
        ..Default::default()
    };
    let hints = || ir::XmlSchemaHints {
        no_namespace_location: Some("plan.xsd".into()),
        locations: Vec::new(),
    };
    let mut corpus: Vec<(String, Project)> = Vec::new();
    let base = project();
    corpus.push(("base/json".into(), base.clone()));
    let mut ordinary = base.clone();
    ordinary.source_options = xml();
    ordinary.target_options = xml();
    corpus.push(("base/ordinary".into(), ordinary.clone()));
    let mut hinted = ordinary.clone();
    hinted.target_options.xml_schema_hints = Some(hints());
    corpus.push(("base/ordinary-hints".into(), hinted));
    let mut source_only = base.clone();
    source_only.source_options = xml();
    corpus.push(("base/ordinary-source-only".into(), source_only));
    let mut json_hints = base.clone();
    json_hints.target_options.xml_schema_hints = Some(hints());
    corpus.push(("base/json-hints".into(), json_hints));
    let mut observed = base.clone();
    observed.source_options = root_view();
    observed.target_options = xml();
    corpus.push(("base/root-view".into(), observed.clone()));
    let mut observed_hints = observed.clone();
    observed_hints.target_options.xml_schema_hints = Some(hints());
    corpus.push(("base/root-view-hints".into(), observed_hints));
    let mut observed_target = observed.clone();
    observed_target.target_options.json_lines = true;
    corpus.push(("base/root-view-target-options".into(), observed_target));
    for (allow, view) in [(true, false), (false, true)] {
        let mut mixed = base.clone();
        mixed.source_options = xml();
        mixed.source_options.xml_allow_inactive_root_type_members = allow;
        mixed.source_options.xml_root_view_read_policy = view;
        corpus.push((format!("base/mixed-flags-{allow}-{view}"), mixed));
    }
    let nested = nested_iteration_project();
    corpus.push(("nested/json".into(), nested.clone()));
    let mut nested_xml = nested;
    nested_xml.source_options = xml();
    nested_xml.target_options = xml();
    corpus.push(("nested/ordinary".into(), nested_xml));
    for depth in [40, 65] {
        let mut node = SchemaNode::scalar(format!("N{depth}"), ScalarType::String);
        for level in (1..depth).rev() {
            node = SchemaNode::group(format!("N{level}"), vec![node]);
        }
        let mut deep = ordinary.clone();
        deep.source = node;
        deep.graph = Graph::default();
        deep.graph.nodes.insert(
            0,
            Node::Const {
                value: Value::String("mapped".into()),
            },
        );
        deep.target = SchemaNode::group("Target", vec![string("Out")]);
        deep.root = Scope {
            bindings: vec![Binding {
                target_field: "Out".into(),
                node: 0,
            }],
            ..Scope::default()
        };
        corpus.push((format!("base/ordinary-depth-{depth}"), deep));
    }
    for (id, subject) in fixtures {
        let Subject::Project(project) = subject else {
            continue;
        };
        if let Some(source) = project.extra_sources.first() {
            let mut named = (**project).clone();
            named.extra_sources[0].options = root_view();
            corpus.push((format!("{id}/named-root-view"), named));
            let mut mixed = (**project).clone();
            mixed.extra_sources[0]
                .options
                .xml_allow_inactive_root_type_members =
                !source.options.xml_allow_inactive_root_type_members;
            corpus.push((format!("{id}/named-mixed-flags"), mixed));
            let mut json = (**project).clone();
            json.extra_sources[0].options = Default::default();
            corpus.push((format!("{id}/named-json"), json));
        }
        if !project.extra_targets.is_empty() {
            let mut hinted = (**project).clone();
            hinted.extra_targets[0].options.xml_schema_hints = Some(hints());
            corpus.push((format!("{id}/named-target-hints"), hinted));
            let mut json = (**project).clone();
            json.extra_targets[0].options = Default::default();
            corpus.push((format!("{id}/named-target-json"), json));
        }
        let mut target_json = (**project).clone();
        target_json.target_options = Default::default();
        corpus.push((format!("{id}/target-json"), target_json));
        let mut flipped = (**project).clone();
        flipped.source_options = if project.source_options.xml_root_view_read_policy {
            xml()
        } else {
            root_view()
        };
        corpus.push((format!("{id}/flipped-input-profile"), flipped));
    }
    super::owned_capability_presets::projects(corpus)
}

/// Directly edited lowered XML Programs whose boundary policy no longer
/// matches the proved shape, plus the same Programs without a boundary.
fn xml_program_corpus(fixtures: &Corpus) -> Corpus {
    let mut corpus: Vec<(String, codegen::Program)> = Vec::new();
    for (id, subject) in fixtures {
        let Subject::Project(project) = subject else {
            continue;
        };
        let Ok(program) = codegen::lower(project) else {
            continue;
        };
        let Some(boundary) = program.xml_boundary.clone() else {
            continue;
        };
        let mut without = program.clone();
        without.xml_boundary = None;
        corpus.push((format!("{id}/no-boundary"), without));
        let mut mixed = program.clone();
        if let Some(policy) = mixed.xml_boundary.as_mut() {
            policy.input.allow_inactive_root_type_members = !policy.input.root_view_policy;
        }
        corpus.push((format!("{id}/mixed-input-flags"), mixed));
        let mut swapped = program.clone();
        if let Some(policy) = swapped.xml_boundary.as_mut() {
            let root_view = !boundary.input.root_view_policy;
            policy.input = codegen::XmlInputPolicy {
                allow_inactive_root_type_members: root_view,
                root_view_policy: root_view,
            };
        }
        corpus.push((format!("{id}/swapped-input-profile"), swapped));
        if !boundary.extra_outputs.is_empty() {
            let mut renamed = program.clone();
            if let Some(policy) = renamed.xml_boundary.as_mut() {
                policy.extra_outputs[0].name.push_str("_renamed");
            }
            corpus.push((format!("{id}/renamed-output-policy"), renamed));
            let mut dropped = program.clone();
            if let Some(policy) = dropped.xml_boundary.as_mut() {
                policy.extra_outputs.pop();
            }
            corpus.push((format!("{id}/dropped-output-policy"), dropped));
        }
        if !boundary.extra_inputs.is_empty() {
            let mut dropped = program.clone();
            if let Some(policy) = dropped.xml_boundary.as_mut() {
                policy.extra_inputs.pop();
            }
            corpus.push((format!("{id}/dropped-input-policy"), dropped));
        }
        let mut extra = program.clone();
        if let Some(policy) = extra.xml_boundary.as_mut() {
            policy.extra_outputs.push(codegen::NamedXmlOutputPolicy {
                name: "undeclared".into(),
                output: policy.output.clone(),
            });
        }
        corpus.push((format!("{id}/undeclared-output-policy"), extra));
    }
    super::owned_capability_presets::programs(corpus)
}

/// Typed-only controls: a JSON descriptor beyond the embedded limit, beside
/// the same mapping with an ordinary descriptor.
fn typed_only_control_corpus() -> Corpus {
    let mut oversized = project();
    let ir::SchemaKind::Group { children, .. } = &mut oversized.source.kind else {
        return Vec::new();
    };
    for index in 0..24_000 {
        children.push(string(&format!(
            "Padding{index:05}_beyond_the_embedded_json_descriptor_limit"
        )));
    }
    super::owned_capability_presets::projects([
        ("ordinary-descriptor", project()),
        ("oversized-json-descriptor", oversized),
    ])
}

fn render(corpora: Vec<(&'static str, Corpus)>) -> String {
    let mut output = String::from(
        "# Boundary emission over the owned corpora, XML fixtures and refusal variants.\n\
         # Each line: family, then the result.\n",
    );
    let subjects = corpora
        .iter()
        .flat_map(|(corpus, subjects)| {
            subjects
                .iter()
                .map(move |(id, subject)| (*corpus, id, subject))
        })
        .collect::<Vec<_>>();
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get().min(8));
    let next = AtomicUsize::new(0);
    let blocks = Mutex::new(vec![String::new(); subjects.len()]);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((corpus, id, subject)) = subjects.get(index) else {
                        break;
                    };
                    let block = subject_block(corpus, id, subject);
                    blocks.lock().expect("record blocks")[index] = block;
                }
            });
        }
    });
    for block in blocks.into_inner().expect("record blocks") {
        output.push_str(&block);
    }
    output
}

#[test]
fn xml_and_json_boundary_emission_matches_recorded_results() -> TestResult<()> {
    let fixtures = xml_fixture_corpus()?;
    let mut corpora = super::owned_capability_presets::corpora()?;
    corpora.push(("xml-requests", xml_request_corpus(&fixtures)));
    corpora.push(("xml-programs", xml_program_corpus(&fixtures)));
    corpora.push(("typed-only-controls", typed_only_control_corpus()));
    corpora.push(("xml-fixtures", fixtures));
    let actual = render(corpora);
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
            "boundary emission differs from {RECORD} ({} actual vs {} recorded lines):\n{}",
            actual.lines().count(),
            expected.lines().count(),
            mismatches.join("\n")
        )
        .into());
    }
    Ok(())
}
