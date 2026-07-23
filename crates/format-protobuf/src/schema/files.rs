use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::{MAX_SCHEMA_BYTES, ProtobufError};

use super::model::Layout;
use super::parser::parse_file;
use super::resolve::{RawFile, RawFileContext, RawSchema};

/// Maximum number of reachable files in one schema graph.
pub const MAX_SCHEMA_FILES: usize = 256;

/// Maximum number of import edges between the root and any schema file.
pub const MAX_IMPORT_DEPTH: usize = 64;

/// Maximum total UTF-8 bytes retained by one embedded schema graph.
pub const MAX_SCHEMA_GRAPH_BYTES: usize = 8 * 1024 * 1024;

/// Maximum size of one canonical virtual schema path.
pub const MAX_IMPORT_PATH_BYTES: usize = 1024;

/// One non-root file in a self-contained protobuf schema graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaFile {
    path: String,
    source: String,
}

impl SchemaFile {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn into_parts(self) -> (String, String) {
        (self.path, self.source)
    }
}

/// A root schema and every reachable import, stored independently of disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaBundle {
    root_path: String,
    root_source: String,
    imports: Vec<SchemaFile>,
}

impl SchemaBundle {
    /// Reads one traversal-confined schema graph relative to `base`.
    pub fn read_relative(base: &Path, root_path: &str) -> Result<Self, ProtobufError> {
        let base = std::fs::canonicalize(base).map_err(|error| {
            ProtobufError::schema(format!(
                "could not resolve protobuf schema base `{}`: {error}",
                base.display()
            ))
        })?;
        if !base.is_dir() {
            return Err(ProtobufError::schema(format!(
                "protobuf schema base `{}` is not a directory",
                base.display()
            )));
        }
        let root_path = canonical_schema_path(root_path)?;
        let mut loader = FileLoader::new(base);
        loader.load(&root_path, 0)?;
        loader.finish(root_path)
    }

    pub fn root_path(&self) -> &str {
        &self.root_path
    }

    pub fn root_source(&self) -> &str {
        &self.root_source
    }

    pub fn imports(&self) -> &[SchemaFile] {
        &self.imports
    }

    pub fn layout(&self) -> Result<Layout, ProtobufError> {
        parse_layout(
            &self.root_path,
            &self.root_source,
            self.imports
                .iter()
                .map(|file| (file.path.as_str(), file.source.as_str())),
        )
    }

    pub fn into_parts(self) -> (String, String, Vec<SchemaFile>) {
        (self.root_path, self.root_source, self.imports)
    }
}

pub(super) fn parse_layout<'a>(
    root_path: &str,
    root_source: &str,
    imports: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Layout, ProtobufError> {
    let root_path = canonical_schema_path(root_path)?;
    let mut sources = BTreeMap::new();
    insert_source(&mut sources, root_path.clone(), root_source)?;
    for (path, source) in imports {
        insert_source(&mut sources, canonical_schema_path(path)?, source)?;
    }
    if sources.len() > MAX_SCHEMA_FILES {
        return Err(ProtobufError::schema(format!(
            "schema graph exceeds the {MAX_SCHEMA_FILES}-file limit"
        )));
    }
    let total_bytes = sources
        .values()
        .try_fold(0_usize, |total, source| total.checked_add(source.len()))
        .ok_or_else(|| ProtobufError::schema("schema graph byte count overflowed"))?;
    if total_bytes > MAX_SCHEMA_GRAPH_BYTES {
        return Err(ProtobufError::schema(format!(
            "schema graph exceeds the {MAX_SCHEMA_GRAPH_BYTES}-byte limit"
        )));
    }

    let mut graph = GraphBuilder::new(sources);
    graph.visit(&root_path, 0)?;
    graph.resolve()
}

fn insert_source(
    sources: &mut BTreeMap<String, String>,
    path: String,
    source: &str,
) -> Result<(), ProtobufError> {
    if source.len() > MAX_SCHEMA_BYTES {
        return Err(ProtobufError::schema(format!(
            "protobuf schema `{path}` exceeds the {MAX_SCHEMA_BYTES}-byte limit"
        )));
    }
    if sources.insert(path.clone(), source.to_string()).is_some() {
        return Err(ProtobufError::schema(format!(
            "schema graph contains duplicate file `{path}`"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

struct GraphFile {
    raw: RawFile,
    imports: Vec<(usize, bool)>,
}

struct GraphBuilder {
    sources: BTreeMap<String, String>,
    ids: HashMap<String, usize>,
    states: HashMap<String, VisitState>,
    files: Vec<Option<GraphFile>>,
    paths: Vec<String>,
    stack: Vec<String>,
}

impl GraphBuilder {
    fn new(sources: BTreeMap<String, String>) -> Self {
        Self {
            sources,
            ids: HashMap::new(),
            states: HashMap::new(),
            files: Vec::new(),
            paths: Vec::new(),
            stack: Vec::new(),
        }
    }

    fn visit(&mut self, path: &str, depth: usize) -> Result<usize, ProtobufError> {
        if depth > MAX_IMPORT_DEPTH {
            return Err(ProtobufError::schema(format!(
                "schema import depth exceeds the limit of {MAX_IMPORT_DEPTH} at `{path}`"
            )));
        }
        match self.states.get(path).copied() {
            Some(VisitState::Done) => {
                return self.ids.get(path).copied().ok_or_else(|| {
                    ProtobufError::schema("completed schema file has no graph identifier")
                });
            }
            Some(VisitState::Visiting) => {
                let start = self.stack.iter().position(|item| item == path).unwrap_or(0);
                let mut cycle = self.stack[start..].to_vec();
                cycle.push(path.to_string());
                return Err(ProtobufError::schema(format!(
                    "protobuf import cycle: {}",
                    cycle.join(" -> ")
                )));
            }
            None => {}
        }
        let raw = self
            .sources
            .get(path)
            .ok_or_else(|| ProtobufError::schema(format!("missing imported schema `{path}`")))
            .and_then(|source| parse_file(source))?;
        let id = self.files.len();
        if id >= MAX_SCHEMA_FILES {
            return Err(ProtobufError::schema(format!(
                "reachable schema graph exceeds the {MAX_SCHEMA_FILES}-file limit"
            )));
        }
        self.ids.insert(path.to_string(), id);
        self.states.insert(path.to_string(), VisitState::Visiting);
        self.files.push(None);
        self.paths.push(path.to_string());
        self.stack.push(path.to_string());

        let mut imports = Vec::with_capacity(raw.imports.len());
        let mut seen = BTreeSet::new();
        for import in &raw.imports {
            let imported_path = resolve_import(path, &import.path)?;
            if !seen.insert(imported_path.clone()) {
                return Err(ProtobufError::schema(format!(
                    "schema `{path}` imports `{imported_path}` more than once"
                )));
            }
            let imported = self.visit(&imported_path, depth + 1)?;
            imports.push((imported, import.public));
        }
        self.stack.pop();
        self.states.insert(path.to_string(), VisitState::Done);
        self.files[id] = Some(GraphFile { raw, imports });
        Ok(id)
    }

    fn resolve(self) -> Result<Layout, ProtobufError> {
        let files = self
            .files
            .into_iter()
            .enumerate()
            .map(|(id, file)| {
                file.ok_or_else(|| {
                    ProtobufError::schema(format!(
                        "schema graph file `{}` was not completed",
                        self.paths.get(id).map_or("<unknown>", String::as_str)
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut exported = vec![None; files.len()];
        for id in 0..files.len() {
            public_exports(id, &files, &mut exported)?;
        }

        let mut contexts = Vec::with_capacity(files.len());
        let mut messages = Vec::new();
        let mut enums = Vec::new();
        for (id, file) in files.iter().enumerate() {
            let mut visible = BTreeSet::from([id]);
            for (imported, _) in &file.imports {
                visible.extend(exported[*imported].iter().flatten().copied());
            }
            contexts.push(RawFileContext {
                visible,
                proto3: file.raw.proto3,
            });
            for mut message in file.raw.messages.iter().cloned() {
                message.file = id;
                messages.push(message);
            }
            for mut enumeration in file.raw.enums.iter().cloned() {
                enumeration.file = id;
                enums.push(enumeration);
            }
        }
        if messages.is_empty() {
            return Err(ProtobufError::schema(
                "schema graph must declare at least one message",
            ));
        }
        RawSchema {
            root_package: files.first().and_then(|file| file.raw.package.clone()),
            files: contexts,
            messages,
            enums,
        }
        .resolve()
    }
}

fn public_exports(
    id: usize,
    files: &[GraphFile],
    memo: &mut [Option<BTreeSet<usize>>],
) -> Result<BTreeSet<usize>, ProtobufError> {
    if let Some(exports) = memo.get(id).and_then(Option::as_ref) {
        return Ok(exports.clone());
    }
    let file = files
        .get(id)
        .ok_or_else(|| ProtobufError::schema("import references an invalid schema file"))?;
    let mut exports = BTreeSet::from([id]);
    for (imported, public) in &file.imports {
        if *public {
            exports.extend(public_exports(*imported, files, memo)?);
        }
    }
    if let Some(slot) = memo.get_mut(id) {
        *slot = Some(exports.clone());
    }
    Ok(exports)
}

fn resolve_import(importer: &str, import: &str) -> Result<String, ProtobufError> {
    if import.starts_with('/') || import.contains('\\') {
        return Err(ProtobufError::schema(format!(
            "schema `{importer}` has non-portable import path `{import}`"
        )));
    }
    canonical_schema_path(import)
}

/// Returns the canonical slash-separated virtual path used by schema bundles.
pub fn canonical_schema_path(path: &str) -> Result<String, ProtobufError> {
    if path.is_empty() || path.len() > MAX_IMPORT_PATH_BYTES {
        return Err(ProtobufError::schema(format!(
            "protobuf schema path must contain 1 to {MAX_IMPORT_PATH_BYTES} bytes"
        )));
    }
    if path.starts_with('/') || path.contains('\\') || path.contains('\0') || path.contains(':') {
        return Err(ProtobufError::schema(format!(
            "non-portable protobuf schema path `{path}`"
        )));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(ProtobufError::schema(format!(
                        "protobuf schema path `{path}` escapes its virtual root"
                    )));
                }
            }
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return Err(ProtobufError::schema(format!(
            "protobuf schema path `{path}` does not name a file"
        )));
    }
    let normalized = parts.join("/");
    if normalized.len() > MAX_IMPORT_PATH_BYTES {
        return Err(ProtobufError::schema(format!(
            "protobuf schema path exceeds the {MAX_IMPORT_PATH_BYTES}-byte limit"
        )));
    }
    Ok(normalized)
}

struct FileLoader {
    base: PathBuf,
    sources: BTreeMap<String, String>,
    physical_paths: HashMap<PathBuf, String>,
    states: HashMap<String, VisitState>,
    stack: Vec<String>,
    total_bytes: usize,
}

impl FileLoader {
    fn new(base: PathBuf) -> Self {
        Self {
            base,
            sources: BTreeMap::new(),
            physical_paths: HashMap::new(),
            states: HashMap::new(),
            stack: Vec::new(),
            total_bytes: 0,
        }
    }

    fn load(&mut self, path: &str, depth: usize) -> Result<(), ProtobufError> {
        if depth > MAX_IMPORT_DEPTH {
            return Err(ProtobufError::schema(format!(
                "schema import depth exceeds the limit of {MAX_IMPORT_DEPTH} at `{path}`"
            )));
        }
        match self.states.get(path).copied() {
            Some(VisitState::Done) => return Ok(()),
            Some(VisitState::Visiting) => {
                let start = self.stack.iter().position(|item| item == path).unwrap_or(0);
                let mut cycle = self.stack[start..].to_vec();
                cycle.push(path.to_string());
                return Err(ProtobufError::schema(format!(
                    "protobuf import cycle: {}",
                    cycle.join(" -> ")
                )));
            }
            None => {}
        }
        if self.sources.len() >= MAX_SCHEMA_FILES {
            return Err(ProtobufError::schema(format!(
                "schema graph exceeds the {MAX_SCHEMA_FILES}-file limit"
            )));
        }
        let candidate = self.base.join(path);
        let physical = std::fs::canonicalize(&candidate).map_err(|error| {
            ProtobufError::schema(format!(
                "could not resolve imported protobuf schema `{path}`: {error}"
            ))
        })?;
        if !physical.starts_with(&self.base) {
            return Err(ProtobufError::schema(format!(
                "imported protobuf schema `{path}` escapes its configured base"
            )));
        }
        if let Some(existing) = self.physical_paths.get(&physical)
            && existing != path
        {
            return Err(ProtobufError::schema(format!(
                "protobuf files `{existing}` and `{path}` resolve to the same file"
            )));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&physical)
            .map_err(|error| {
                ProtobufError::schema(format!("could not open protobuf schema `{path}`: {error}"))
            })?
            .take((MAX_SCHEMA_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                ProtobufError::schema(format!("could not read protobuf schema `{path}`: {error}"))
            })?;
        if bytes.len() > MAX_SCHEMA_BYTES {
            return Err(ProtobufError::schema(format!(
                "protobuf schema `{path}` exceeds the {MAX_SCHEMA_BYTES}-byte limit"
            )));
        }
        self.total_bytes = self
            .total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| ProtobufError::schema("schema graph byte count overflowed"))?;
        if self.total_bytes > MAX_SCHEMA_GRAPH_BYTES {
            return Err(ProtobufError::schema(format!(
                "schema graph exceeds the {MAX_SCHEMA_GRAPH_BYTES}-byte limit"
            )));
        }
        let source = String::from_utf8(bytes).map_err(|_| {
            ProtobufError::schema(format!("protobuf schema `{path}` is not valid UTF-8"))
        })?;
        let raw = parse_file(&source)?;
        self.states.insert(path.to_string(), VisitState::Visiting);
        self.stack.push(path.to_string());
        self.physical_paths.insert(physical, path.to_string());
        self.sources.insert(path.to_string(), source);

        let mut seen = BTreeSet::new();
        for import in raw.imports {
            let imported_path = resolve_import(path, &import.path)?;
            if !seen.insert(imported_path.clone()) {
                return Err(ProtobufError::schema(format!(
                    "schema `{path}` imports `{imported_path}` more than once"
                )));
            }
            self.load(&imported_path, depth + 1)?;
        }
        self.stack.pop();
        self.states.insert(path.to_string(), VisitState::Done);
        Ok(())
    }

    fn finish(mut self, root_path: String) -> Result<SchemaBundle, ProtobufError> {
        let root_source = self.sources.remove(&root_path).ok_or_else(|| {
            ProtobufError::schema(format!("root protobuf schema `{root_path}` was not loaded"))
        })?;
        let imports = self
            .sources
            .into_iter()
            .map(|(path, source)| SchemaFile { path, source })
            .collect();
        Ok(SchemaBundle {
            root_path,
            root_source,
            imports,
        })
    }
}
