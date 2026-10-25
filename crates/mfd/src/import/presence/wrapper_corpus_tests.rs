//! Engine validation of nested-array wrapper frames over every in-tree schema.
//!
//! Every saved project, every saved schema embedded in a JSON fixture, every
//! JSON Schema and XSD fixture, and every `.mfd` fixture imported under both
//! profiles must satisfy the shared frame invariant, and engine validation
//! must report no malformed-frame issue for any of them.

use std::path::{Path, PathBuf};

use ir::{SchemaKind, SchemaNode};
use mapping::{Graph, Project, Scope};

use crate::{ImportOptions, ImportProfile};

/// Leading text of engine's malformed nested-array wrapper frame issue.
const WRAPPER_FRAME_ISSUE: &str = "JSON nested-array wrapper metadata";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files(directory: &Path, extension: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        let hidden_or_build = path.file_name().is_some_and(|name| {
            name.to_string_lossy().starts_with('.') || name == "target" || name == "node_modules"
        });
        if hidden_or_build {
            continue;
        }
        if path.is_dir() {
            files(&path, extension, found);
        } else if path.extension().is_some_and(|found| found == extension) {
            found.push(path);
        }
    }
}

fn workspace_files(extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for directory in ["crates", "conformance", "examples", "docs"] {
        files(&workspace().join(directory), extension, &mut found);
    }
    found
}

/// Wrapper frames anywhere in `schema`, including predicate schemas.
fn frames(node: &SchemaNode) -> usize {
    let mut count = usize::from(node.json_array_wrapper);
    if let SchemaKind::Group {
        children, dynamic, ..
    } = &node.kind
    {
        count += children.iter().map(frames).sum::<usize>();
        count += dynamic.as_deref().map_or(0, frames);
    }
    for predicate in node
        .json_contains
        .iter()
        .flat_map(|terms| terms.as_slice())
        .filter_map(|term| term.predicate().as_schema())
        .chain(
            node.json_dependent_schemas
                .iter()
                .flat_map(|terms| terms.as_slice())
                .filter_map(|term| term.predicate().as_schema()),
        )
    {
        count += frames(predicate);
    }
    count
}

/// A project whose only content is `schema` on both boundaries.
fn schema_project(schema: SchemaNode) -> Project {
    Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn boundary_schemas(project: &Project) -> Vec<&SchemaNode> {
    let mut schemas = vec![&project.source, &project.target];
    schemas.extend(project.extra_sources.iter().map(|source| &source.schema));
    schemas.extend(project.extra_targets.iter().map(|target| &target.schema));
    schemas
}

#[derive(Default)]
struct Corpus {
    projects: usize,
    schemas: usize,
    frames: usize,
}

impl Corpus {
    fn check(&mut self, origin: &Path, project: &Project) {
        self.projects += 1;
        for schema in boundary_schemas(project) {
            self.schemas += 1;
            self.frames += frames(schema);
            assert!(
                schema.json_array_wrappers_tree_is_valid(),
                "{}",
                origin.display()
            );
        }
        let refused = engine::validate(project)
            .into_iter()
            .filter(|issue| issue.message.starts_with(WRAPPER_FRAME_ISSUE))
            .collect::<Vec<_>>();
        assert!(refused.is_empty(), "{}: {refused:?}", origin.display());
    }
}

/// Saved projects and saved schemas embedded anywhere in one JSON value.
fn embedded(value: &serde_json::Value, origin: &Path, corpus: &mut Corpus) {
    match value {
        serde_json::Value::Object(object) => {
            if object.contains_key("graph")
                && let Ok(project) = serde_json::from_value::<Project>(value.clone())
            {
                corpus.check(origin, &project);
                return;
            }
            if object.contains_key("name")
                && object.contains_key("kind")
                && let Ok(schema) = serde_json::from_value::<SchemaNode>(value.clone())
            {
                corpus.check(origin, &schema_project(schema));
                return;
            }
            for nested in object.values() {
                embedded(nested, origin, corpus);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                embedded(item, origin, corpus);
            }
        }
        _ => {}
    }
}

#[test]
fn every_saved_project_and_json_fixture_schema_has_valid_wrapper_frames() {
    let mut corpus = Corpus::default();
    let mut json_schemas = 0;
    for path in workspace_files("json") {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            embedded(&value, &path, &mut corpus);
        }
        if path.components().any(|part| part.as_os_str() == "fixtures")
            && let Ok(schema) = format_json::json_schema::import(&path)
        {
            json_schemas += 1;
            corpus.check(&path, &schema_project(schema));
        }
    }
    assert!(corpus.projects > 600, "{}", corpus.projects);
    assert!(json_schemas > 300, "{json_schemas}");
    assert!(corpus.frames > 30, "{}", corpus.frames);
}

#[test]
fn every_xsd_fixture_root_has_valid_wrapper_frames() {
    let mut corpus = Corpus::default();
    for path in workspace_files("xsd") {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        // Every global element of each fixture is a separately imported root.
        let roots = document
            .root_element()
            .children()
            .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "element")))
            .filter_map(|node| node.attribute("name"))
            .filter_map(|name| format_xml::xsd::import_root(&path, Some(name)).ok())
            .collect::<Vec<_>>();
        for schema in roots {
            corpus.check(&path, &schema_project(schema));
        }
    }
    assert!(corpus.projects > 100, "{}", corpus.projects);
}

#[test]
fn every_imported_mfd_fixture_has_valid_wrapper_frames() {
    let mut corpus = Corpus::default();
    let paths = workspace_files("mfd");
    assert!(paths.len() > 250, "{}", paths.len());
    for path in &paths {
        for profile in [ImportProfile::BestEffort, ImportProfile::Executable] {
            let Ok(outcome) = crate::import_with_profile(path, &ImportOptions::default(), profile)
            else {
                continue;
            };
            corpus.check(path, &outcome.imported.project);
        }
    }
    assert!(corpus.projects > 400, "{}", corpus.projects);
}
