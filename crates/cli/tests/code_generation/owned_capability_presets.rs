//! Every public owned generation preset, applied to every owned conformance
//! corpus subject (models, closed and closed-context Projects, directly
//! constructed Programs) and the owned harness fixtures, against the admission
//! recorded in `fixtures/owned_capability_presets.txt`.
//!
//! Each line records one entry-point family for one subject: the result and the
//! presets that produce it. Results are `ok` (validators), `ok:<hash>` (the
//! lowered Program, the named-input plan or the emitted artifact bytes) or
//! `err:<hash>` of the complete diagnostic `Debug` with a short summary. Set
//! `FERRULE_UPDATE_OWNED_CAPABILITY_PRESETS=1` to rewrite the record after an
//! intended admission change.
use super::*;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Write as _};
use std::fs;

const RECORD: &str = "code_generation/fixtures/owned_capability_presets.txt";
const UPDATE: &str = "FERRULE_UPDATE_OWNED_CAPABILITY_PRESETS";

/// A conformance subject: an authored Project, or a directly constructed
/// Program that bypasses lowering.
pub(super) enum Subject {
    Project(Box<Project>),
    Program(Box<codegen::Program>),
}

pub(super) type Corpus = Vec<(String, Subject)>;

pub(super) fn projects<I, S>(models: I) -> Corpus
where
    I: IntoIterator<Item = (S, Project)>,
    S: Into<String>,
{
    models
        .into_iter()
        .map(|(id, project)| (id.into(), Subject::Project(Box::new(project))))
        .collect()
}

pub(super) fn programs<I, S>(programs: I) -> Corpus
where
    I: IntoIterator<Item = (S, codegen::Program)>,
    S: Into<String>,
{
    programs
        .into_iter()
        .map(|(id, program)| (id.into(), Subject::Program(Box::new(program))))
        .collect()
}

type Lower = fn(&Project) -> Result<codegen::Program, codegen::LowerError>;
type Validate = fn(&codegen::Program) -> Result<(), codegen::ProgramValidationError>;
type Plan =
    for<'a> fn(
        &'a codegen::Program,
    )
        -> Result<codegen::NamedInputAdmissionPlan<'a>, codegen::ProgramValidationError>;
type Rust = fn(
    &codegen::Program,
    &codegen_rust::Options,
) -> Result<codegen::ArtifactSet, codegen_rust::EmitError>;
type CSharp = fn(&codegen::Program) -> Result<codegen::ArtifactSet, codegen_csharp::EmitError>;

/// One public preset: its lowering, program validator, emission validator,
/// named-input plan and both emitters.
pub(super) struct Preset {
    name: &'static str,
    pub(super) lower: Lower,
    validate: Validate,
    emission: Validate,
    plan: Plan,
    rust: Rust,
    pub(super) csharp: CSharp,
}

macro_rules! preset {
    ($name:literal, $lower:ident, $validate:ident, $emission:ident, $plan:ident, $emit:ident) => {
        Preset {
            name: $name,
            lower: codegen::$lower,
            validate: codegen::$validate,
            emission: codegen::$emission,
            plan: codegen::$plan,
            rust: codegen_rust::$emit,
            csharp: codegen_csharp::$emit,
        }
    };
}

pub(super) fn presets() -> Vec<Preset> {
    vec![
        Preset {
            name: "ordinary",
            lower: codegen::lower,
            validate: codegen::validate_program,
            emission: codegen::validate_owned_emission,
            plan: codegen::legacy_named_input_admission_plan,
            rust: codegen_rust::emit,
            csharp: codegen_csharp::emit,
        },
        preset!(
            "sources",
            lower,
            validate_program,
            validate_basic_owned_emission,
            legacy_named_input_admission_plan,
            emit_with_basic_owned_sources
        ),
        preset!(
            "filters",
            lower_with_basic_owned_filters,
            validate_program_with_basic_owned_filters,
            validate_basic_owned_filter_emission,
            legacy_named_input_admission_plan_with_basic_owned_filters,
            emit_with_basic_owned_filters
        ),
        preset!(
            "sort",
            lower_with_basic_owned_sort,
            validate_program_with_basic_owned_sort,
            validate_basic_owned_sort_emission,
            legacy_named_input_admission_plan_with_basic_owned_sort,
            emit_with_basic_owned_sort
        ),
        preset!(
            "windows",
            lower_with_basic_owned_windows,
            validate_program_with_basic_owned_windows,
            validate_basic_owned_window_emission,
            legacy_named_input_admission_plan_with_basic_owned_windows,
            emit_with_basic_owned_windows
        ),
        preset!(
            "row_failures",
            lower_with_basic_owned_row_failures,
            validate_program_with_basic_owned_row_failures,
            validate_basic_owned_row_failure_emission,
            legacy_named_input_admission_plan_with_basic_owned_row_failures,
            emit_with_basic_owned_row_failures
        ),
        preset!(
            "group_by",
            lower_with_basic_owned_group_by,
            validate_program_with_basic_owned_group_by,
            validate_basic_owned_group_by_emission,
            legacy_named_input_admission_plan_with_basic_owned_group_by,
            emit_with_basic_owned_group_by
        ),
        preset!(
            "sequence_item_at",
            lower_with_basic_owned_sequence_item_at,
            validate_program_with_basic_owned_sequence_item_at,
            validate_basic_owned_sequence_item_at_emission,
            legacy_named_input_admission_plan_with_basic_owned_sequence_item_at,
            emit_with_basic_owned_sequence_item_at
        ),
        preset!(
            "group_adjacent_by",
            lower_with_basic_owned_group_adjacent_by,
            validate_program_with_basic_owned_group_adjacent_by,
            validate_basic_owned_group_adjacent_by_emission,
            legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by,
            emit_with_basic_owned_group_adjacent_by
        ),
        preset!(
            "group_starting_with",
            lower_with_basic_owned_group_starting_with,
            validate_program_with_basic_owned_group_starting_with,
            validate_basic_owned_group_starting_with_emission,
            legacy_named_input_admission_plan_with_basic_owned_group_starting_with,
            emit_with_basic_owned_group_starting_with
        ),
        preset!(
            "group_ending_with",
            lower_with_basic_owned_group_ending_with,
            validate_program_with_basic_owned_group_ending_with,
            validate_basic_owned_group_ending_with_emission,
            legacy_named_input_admission_plan_with_basic_owned_group_ending_with,
            emit_with_basic_owned_group_ending_with
        ),
        preset!(
            "group_into_blocks",
            lower_with_basic_owned_group_into_blocks,
            validate_program_with_basic_owned_group_into_blocks,
            validate_basic_owned_group_into_blocks_emission,
            legacy_named_input_admission_plan_with_basic_owned_group_into_blocks,
            emit_with_basic_owned_group_into_blocks
        ),
        preset!(
            "failure_sources",
            lower_with_basic_owned_failure_sources,
            validate_program_with_basic_owned_failure_sources,
            validate_basic_owned_failure_source_emission,
            legacy_named_input_admission_plan_with_basic_owned_failure_sources,
            emit_with_basic_owned_failure_sources
        ),
        preset!(
            "path_hierarchy",
            lower_with_basic_owned_path_hierarchy,
            validate_program_with_basic_owned_path_hierarchy,
            validate_basic_owned_path_hierarchy_emission,
            legacy_named_input_admission_plan_with_basic_owned_path_hierarchy,
            emit_with_basic_owned_path_hierarchy
        ),
        preset!(
            "adjacency_tree",
            lower_with_basic_owned_adjacency_tree,
            validate_program_with_basic_owned_adjacency_tree,
            validate_basic_owned_adjacency_tree_emission,
            legacy_named_input_admission_plan_with_basic_owned_adjacency_tree,
            emit_with_basic_owned_adjacency_tree
        ),
        preset!(
            "recursive_collect",
            lower_with_basic_owned_recursive_collect,
            validate_program_with_basic_owned_recursive_collect,
            validate_basic_owned_recursive_collect_emission,
            legacy_named_input_admission_plan_with_basic_owned_recursive_collect,
            emit_with_basic_owned_recursive_collect
        ),
        preset!(
            "recursive_filter",
            lower_with_basic_owned_recursive_filter,
            validate_program_with_basic_owned_recursive_filter,
            validate_basic_owned_recursive_filter_emission,
            legacy_named_input_admission_plan_with_basic_owned_recursive_filter,
            emit_with_basic_owned_recursive_filter
        ),
        preset!(
            "lookup_find",
            lower_with_basic_owned_lookup_find,
            validate_program_with_basic_owned_lookup_find,
            validate_basic_owned_lookup_find_emission,
            legacy_named_input_admission_plan_with_basic_owned_lookup_find,
            emit_with_basic_owned_lookup_find
        ),
        preset!(
            "dynamic_field",
            lower_with_basic_owned_dynamic_field,
            validate_program_with_basic_owned_dynamic_field,
            validate_basic_owned_dynamic_field_emission,
            legacy_named_input_admission_plan_with_basic_owned_dynamic_field,
            emit_with_basic_owned_dynamic_field
        ),
        preset!(
            "dynamic_loading",
            lower_with_basic_owned_dynamic_loading,
            validate_program_with_basic_owned_dynamic_loading,
            validate_basic_owned_dynamic_loading_emission,
            legacy_named_input_admission_plan_with_basic_owned_dynamic_loading,
            emit_with_basic_owned_dynamic_loading
        ),
        preset!(
            "collection_aggregates",
            lower_with_basic_owned_collection_aggregates,
            validate_program_with_basic_owned_collection_aggregates,
            validate_basic_owned_collection_aggregates_emission,
            legacy_named_input_admission_plan_with_basic_owned_collection_aggregates,
            emit_with_basic_owned_collection_aggregates
        ),
        preset!(
            "joins",
            lower_with_basic_owned_joins,
            validate_program_with_basic_owned_joins,
            validate_basic_owned_joins_emission,
            legacy_named_input_admission_plan_with_basic_owned_joins,
            emit_with_basic_owned_joins
        ),
        preset!(
            "sequence_exists",
            lower_with_basic_owned_sequence_exists,
            validate_program_with_basic_owned_sequence_exists,
            validate_basic_owned_sequence_exists_emission,
            legacy_named_input_admission_plan_with_basic_owned_sequence_exists,
            emit_with_basic_owned_sequence_exists
        ),
        preset!(
            "sequence_aggregate",
            lower_with_basic_owned_sequence_aggregate,
            validate_program_with_basic_owned_sequence_aggregate,
            validate_basic_owned_sequence_aggregate_emission,
            legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate,
            emit_with_basic_owned_sequence_aggregate
        ),
        preset!(
            "xml_serialize",
            lower_with_basic_owned_xml_serialize,
            validate_program_with_basic_owned_xml_serialize,
            validate_basic_owned_xml_serialize_emission,
            legacy_named_input_admission_plan_with_basic_owned_xml_serialize,
            emit_with_basic_owned_xml_serialize
        ),
        preset!(
            "xml_mixed_content",
            lower_with_basic_owned_xml_mixed_content,
            validate_program_with_basic_owned_xml_mixed_content,
            validate_basic_owned_xml_mixed_content_emission,
            legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content,
            emit_with_basic_owned_xml_mixed_content
        ),
        preset!(
            "xml_mixed_construction",
            lower_with_basic_owned_xml_mixed_construction,
            validate_program_with_basic_owned_xml_mixed_construction,
            validate_basic_owned_xml_mixed_construction_emission,
            legacy_named_input_admission_plan_with_basic_owned_xml_mixed_construction,
            emit_with_basic_owned_xml_mixed_construction
        ),
        preset!(
            "document_paths",
            lower_with_basic_owned_document_paths,
            validate_program_with_basic_owned_document_paths,
            validate_basic_owned_document_path_emission,
            legacy_named_input_admission_plan_with_basic_owned_document_paths,
            emit_with_basic_owned_document_paths
        ),
        preset!(
            "dynamic_document_paths",
            lower_with_basic_owned_dynamic_document_paths,
            validate_program_with_basic_owned_dynamic_document_paths,
            validate_basic_owned_dynamic_document_path_emission,
            legacy_named_input_admission_plan_with_basic_owned_dynamic_document_paths,
            emit_with_basic_owned_dynamic_document_paths
        ),
        preset!(
            "xml_document_sets",
            lower_with_basic_owned_xml_document_sets,
            validate_program_with_basic_owned_xml_document_sets,
            validate_basic_owned_xml_document_set_emission,
            legacy_named_input_admission_plan_with_basic_owned_xml_document_sets,
            emit_with_basic_owned_xml_document_sets
        ),
    ]
}

/// Stable FNV-1a over a byte stream.
pub(super) struct Fnv(u64);

impl Fnv {
    pub(super) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    pub(super) fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self
    }
    pub(super) fn finish(&self) -> String {
        format!("{:016x}", self.0)
    }
}

pub(super) fn digest(text: &str) -> String {
    Fnv::new().bytes(text.as_bytes()).finish()
}

pub(super) fn refusal(error: &(impl Debug + Display)) -> String {
    let debug = format!("{error:?}");
    // The owned capability reason, or the start of the message, names the cause.
    let summary = match debug.split_once("reason: ") {
        Some((_, rest)) => rest
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or_default()
            .to_owned(),
        None => error
            .to_string()
            .split_whitespace()
            .take(6)
            .collect::<Vec<_>>()
            .join("_"),
    };
    format!("err:{} {summary}", digest(&debug))
}

pub(super) fn accepted<T, E: Debug + Display>(
    result: &Result<T, E>,
    value: impl Fn(&T) -> String,
) -> String {
    match result {
        Ok(value_) => value(value_),
        Err(error) => refusal(error),
    }
}

pub(super) fn artifacts(set: &codegen::ArtifactSet) -> String {
    let mut hash = Fnv::new();
    for file in set.files() {
        let path = file.path.as_str();
        hash.bytes(path.as_bytes()).bytes(&[0]);
        // Fixed C# runtime sources are not generated; every generated file is.
        if !path.starts_with("Runtime/") {
            hash.bytes(&file.contents).bytes(&[0]);
        }
    }
    format!("ok:{}", hash.finish())
}

/// Results for one subject, per family and preset, in preset order.
fn record_subject(
    presets: &[Preset],
    options: &codegen_rust::Options,
    subject: &Subject,
) -> Vec<(&'static str, Vec<String>)> {
    let mut families = Vec::new();
    let program = match subject {
        Subject::Program(program) => (**program).clone(),
        Subject::Project(project) => {
            let mut first: Option<(codegen::Program, String)> = None;
            let mut results = Vec::new();
            for preset in presets {
                results.push(match (preset.lower)(project) {
                    Err(error) => refusal(&error),
                    // Hash each distinct lowered Program once.
                    Ok(program) => match &first {
                        Some((seen, hash)) if *seen == program => hash.clone(),
                        _ => {
                            let hash = format!("ok:{}", digest(&format!("{program:?}")));
                            first.get_or_insert((program, hash.clone()));
                            hash
                        }
                    },
                });
            }
            families.push(("lower", results));
            // Every successful lowering yields the same Program; validators
            // and emitters see it whichever preset lowered it.
            match first {
                Some((program, _)) => program,
                None => return families,
            }
        }
    };
    let each = |run: &dyn Fn(&Preset) -> String| presets.iter().map(run).collect::<Vec<_>>();
    families.push((
        "validate",
        each(&|preset| accepted(&(preset.validate)(&program), |_| "ok".into())),
    ));
    families.push((
        "emission",
        each(&|preset| accepted(&(preset.emission)(&program), |_| "ok".into())),
    ));
    families.push((
        "plan",
        each(&|preset| {
            accepted(&(preset.plan)(&program), |plan| {
                format!("ok:{}", digest(&format!("{plan:?}")))
            })
        }),
    ));
    families.push((
        "rust",
        each(&|preset| accepted(&(preset.rust)(&program, options), artifacts)),
    ));
    families.push((
        "csharp",
        each(&|preset| accepted(&(preset.csharp)(&program), artifacts)),
    ));
    families
}

fn subject_block(
    presets: &[Preset],
    options: &codegen_rust::Options,
    corpus: &str,
    id: &str,
    subject: &Subject,
) -> String {
    let kind = match subject {
        Subject::Project(_) => "project",
        Subject::Program(_) => "program",
    };
    let mut output = format!("\n## {corpus}/{id} ({kind})\n");
    for (family, results) in record_subject(presets, options, subject) {
        let mut grouped = BTreeMap::<String, (usize, Vec<u8>)>::new();
        for (index, result) in results.into_iter().enumerate() {
            grouped
                .entry(result)
                .or_insert_with(|| (index, vec![b'.'; presets.len()]))
                .1[index] = b'x';
        }
        let mut lines = grouped.into_iter().collect::<Vec<_>>();
        lines.sort_by_key(|(_, (first, _))| *first);
        for (result, (_, mask)) in lines {
            let mask = String::from_utf8(mask).unwrap_or_default();
            let _ = writeln!(output, "{family:<8} {mask} {result}");
        }
    }
    output
}

fn render(presets: &[Preset], corpora: Vec<(&'static str, Corpus)>) -> String {
    let options = codegen_rust::Options {
        package_name: "owned-capability-presets".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path("codegen-runtime".into()),
    };
    let mut output = String::from(
        "# Owned generation presets over the owned conformance corpora.\n\
         # Each line: family, the presets producing the result (x, in the\n\
         # column order below), then the result.\n",
    );
    for (index, preset) in presets.iter().enumerate() {
        let _ = writeln!(output, "{:<8} {}x {}", "#", ".".repeat(index), preset.name);
    }
    let subjects = corpora
        .iter()
        .flat_map(|(corpus, subjects)| {
            subjects
                .iter()
                .map(move |(id, subject)| (*corpus, id, subject))
        })
        .collect::<Vec<_>>();
    // Subjects are independent; record them on a few threads, in order.
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
                    let block = subject_block(presets, &options, corpus, id, subject);
                    blocks.lock().expect("record blocks")[index] = block;
                }
            });
        }
    });
    let blocks = blocks.into_inner().expect("record blocks");
    for block in blocks {
        output.push_str(&block);
    }
    output
}

/// Declared `sqlite_read_v1` query inputs, which code generation refuses
/// before any owned admission: a primary query, a named query beside an
/// ordinary primary, and a named query walked by an owned iteration.
fn sqlite_read_corpus() -> TestResult<Corpus> {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/fixtures/sqlite-read-v1");
    let profile: Json = serde_json::from_str(&fs::read_to_string(fixtures.join("profile.json"))?)?;
    let requests: Json =
        serde_json::from_str(&fs::read_to_string(fixtures.join("requests.json"))?)?;
    let fields = requests["schema"]["fields"]
        .as_array()
        .ok_or("sqlite fields")?
        .iter()
        .map(|field| serde_json::json!({"name": field[0], "kind": {"kind": "scalar", "ty": field[1]}}))
        .collect::<Vec<_>>();
    let orders = serde_json::json!({"name": "orders", "repeating": true, "kind": {"kind": "group", "children": fields}});
    let query = serde_json::json!({"sqlite_read": profile});
    let mut nodes = serde_json::Map::new();
    let mut bindings = Vec::new();
    for (id, field) in ["id", "label", "score", "active", "ratio"]
        .iter()
        .enumerate()
    {
        nodes.insert(
            id.to_string(),
            serde_json::json!({"kind": "source_field", "path": [field]}),
        );
        bindings.push(serde_json::json!({"target_field": field, "node": id}));
    }
    let primary = serde_json::json!({
        "source": orders, "target": orders, "source_path": "orders.db", "target_path": "out.json",
        "source_options": query, "graph": {"nodes": nodes}, "root": {"source": [], "bindings": bindings},
    });
    let mut named = primary.clone();
    named["source"] = serde_json::json!({"name": "doc", "kind": {"kind": "group", "children": [
        {"name": "Value", "kind": {"kind": "scalar", "ty": "string"}}
    ]}});
    named["source_path"] = serde_json::json!("primary.json");
    named["source_options"] = serde_json::json!({});
    named["extra_sources"] = serde_json::json!([{
        "name": "orders", "path": "orders.db", "schema": orders, "options": query
    }]);
    named["root"]["source"] = serde_json::json!(["orders"]);
    let mut owned: Project = serde_json::from_value(named.clone())?;
    owned.root.iteration = ScopeIteration::OwnedSource {
        source: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Named {
                name: "orders".into(),
            },
            path: vec![],
        },
        output_path: None,
    };
    Ok(projects([
        ("primary-query", serde_json::from_value(primary)?),
        ("named-query", serde_json::from_value(named)?),
        ("owned-named-query", owned),
    ]))
}

/// The imported undeclared-JSON-member designs of
/// `crates/mfd/tests/fixtures/json-undeclared-members`.
fn json_undeclared_members_corpus() -> TestResult<Corpus> {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/json-undeclared-members");
    let mut corpus = Vec::new();
    for design in ["open", "typed", "nested", "matching", "closed"] {
        if let Ok(imported) = mfd::import(&directory.join(format!("{design}.mfd"))) {
            corpus.push((design, imported.project));
        }
    }
    Ok(projects(corpus))
}

pub(super) fn corpora() -> TestResult<Vec<(&'static str, Corpus)>> {
    Ok(vec![
        (
            "basic-owned-filter",
            super::basic_owned_filter::capability_corpus(),
        ),
        (
            "basic-owned-sort",
            super::basic_owned_sort::capability_corpus(),
        ),
        (
            "basic-owned-windows",
            super::basic_owned_windows::capability_corpus(),
        ),
        (
            "basic-owned-nested-alias",
            super::basic_owned_nested_alias::capability_corpus(),
        ),
        (
            "basic-owned-occurrence",
            super::basic_owned_occurrence::capability_corpus(),
        ),
        ("sqlite-read-v1", sqlite_read_corpus()?),
        ("json-undeclared-members", json_undeclared_members_corpus()?),
        (
            "harness/basic-owned-row-failures",
            super::basic_owned_row_failures::capability_corpus()?,
        ),
        (
            "harness/basic-owned-json-adapters",
            super::basic_owned_json_adapters::capability_corpus()?,
        ),
        (
            "basic-owned-scalar-construction",
            super::basic_owned_scalar_construction::capability_corpus()?,
        ),
        (
            "basic-owned-group-by",
            super::basic_owned_group_by::capability_corpus()?,
        ),
        (
            "basic-owned-sequence-item-at",
            super::basic_owned_sequence_item_at::capability_corpus()?,
        ),
        (
            "basic-owned-group-adjacent-by",
            super::basic_owned_group_adjacent_by::capability_corpus()?,
        ),
        (
            "basic-owned-group-starting-with",
            super::basic_owned_group_starting_with::capability_corpus()?,
        ),
        (
            "basic-owned-group-ending-with",
            super::basic_owned_group_ending_with::capability_corpus()?,
        ),
        (
            "basic-owned-group-into-blocks",
            super::basic_owned_group_into_blocks::capability_corpus()?,
        ),
        (
            "basic-owned-sequence-exists",
            super::basic_owned_sequence_exists::capability_corpus()?,
        ),
        (
            "basic-owned-sequence-aggregate",
            super::basic_owned_sequence_aggregate::capability_corpus()?,
        ),
        (
            "basic-owned-failure-sources",
            super::basic_owned_failure_sources::capability_corpus()?,
        ),
        (
            "basic-owned-path-hierarchy",
            super::basic_owned_path_hierarchy::capability_corpus()?,
        ),
        (
            "basic-owned-adjacency-tree",
            super::basic_owned_adjacency_tree::capability_corpus()?,
        ),
        (
            "basic-owned-recursive-collect",
            super::basic_owned_recursive_collect::capability_corpus()?,
        ),
        (
            "basic-owned-recursive-filter",
            super::basic_owned_recursive_filter::capability_corpus()?,
        ),
        (
            "basic-owned-lookup-find",
            super::basic_owned_lookup_find::capability_corpus()?,
        ),
        (
            "basic-owned-dynamic-field",
            super::basic_owned_dynamic_field::capability_corpus()?,
        ),
        (
            "basic-owned-dynamic-loading",
            super::basic_owned_dynamic_loading::capability_corpus()?,
        ),
        (
            "basic-owned-collection-aggregates",
            super::basic_owned_collection_aggregates::capability_corpus()?,
        ),
        (
            "basic-owned-joins",
            super::basic_owned_joins::capability_corpus()?,
        ),
        (
            "basic-owned-xml-serialize",
            super::basic_owned_xml_serialize::capability_corpus()?,
        ),
        (
            "basic-owned-xml-mixed-content",
            super::basic_owned_xml_mixed_content::capability_corpus()?,
        ),
        (
            "basic-owned-xml-mixed-construction",
            super::basic_owned_xml_mixed_construction::capability_corpus()?,
        ),
        (
            "basic-owned-document-path",
            super::basic_owned_document_path::capability_corpus(),
        ),
        (
            "basic-owned-dynamic-document-path",
            super::basic_owned_dynamic_document_path::capability_corpus(),
        ),
        (
            "basic-owned-xml-document-set",
            super::basic_owned_xml_document_set::capability_corpus(),
        ),
    ])
}

#[test]
fn owned_capability_presets_match_recorded_admission() -> TestResult<()> {
    let actual = render(&presets(), corpora()?);
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
            "owned preset admission differs from {RECORD} ({} actual vs {} recorded lines):\n{}",
            actual.lines().count(),
            expected.lines().count(),
            mismatches.join("\n")
        )
        .into());
    }
    Ok(())
}
