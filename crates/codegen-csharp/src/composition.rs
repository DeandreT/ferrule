//! Structural composition of the generated C# library.
//!
//! The ordinary mapping renderer yields the `GeneratedMapping` class as typed
//! parts. Companion emitters add their own partial declarations, compile items
//! and runtime sources to a [`Library`], and only [`Library::into_artifacts`]
//! renders the mapping source and project file. Nothing searches or rewrites
//! emitted text, so user literals and identifiers that resemble generated
//! declarations are never matched.

use codegen::{ArtifactSet, GeneratedFile};

use crate::{EmitError, file, runtime};

const NAMESPACE: &str = "namespace Ferrule.Generated;\n\n";
const MAPPING_PATH: &str = "GeneratedMapping.cs";
const TARGET_BUILDER_PATH: &str = "GeneratedTargetBuilder.cs";
const RUNTIME_GLOB: &str = "Runtime/**/*.cs";
const PROJECT_HEAD: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
    <RootNamespace>Ferrule.Generated</RootNamespace>
    <AssemblyName>Ferrule.Generated</AssemblyName>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
  </PropertyGroup>
  <ItemGroup>
"#;
const PROJECT_TAIL: &str = "  </ItemGroup>\n</Project>\n";

/// One `GeneratedMapping` declaration: the top-level types its compilation
/// unit declares before the class, and the class members.
#[derive(Debug, Default)]
pub(crate) struct ClassPart {
    pub(crate) types: String,
    pub(crate) members: String,
}

impl ClassPart {
    pub(crate) fn members(members: String) -> Self {
        Self {
            types: String::new(),
            members,
        }
    }

    fn render(&self, partial: bool, output: &mut String) {
        output.push_str(&self.types);
        output.push_str(if partial {
            "public static partial class GeneratedMapping\n{\n"
        } else {
            "public static class GeneratedMapping\n{\n"
        });
        output.push_str(&self.members);
        output.push_str("}\n");
    }

    /// A complete compilation unit holding only this partial declaration.
    fn render_unit(&self) -> String {
        let mut output = String::from(NAMESPACE);
        self.render(true, &mut output);
        output
    }
}

/// A companion source file: its own compilation unit extending the mapping.
struct Companion {
    path: &'static str,
    part: ClassPart,
}

/// The project file of one generated library. Each companion is compiled
/// directly after the mapping source, the most recently layered first.
struct ProjectFile<'a> {
    companions: &'a [Companion],
}

impl ProjectFile<'_> {
    fn render(&self) -> String {
        let mut output = String::from(PROJECT_HEAD);
        let items = std::iter::once(MAPPING_PATH)
            .chain(self.companions.iter().rev().map(|companion| companion.path))
            .chain([TARGET_BUILDER_PATH, RUNTIME_GLOB]);
        for item in items {
            output.push_str("    <Compile Include=\"");
            output.push_str(item);
            output.push_str("\" />\n");
        }
        output.push_str(PROJECT_TAIL);
        output
    }
}

/// A generated library under composition around the ordinary mapping class.
pub(crate) struct Library {
    mapping: ClassPart,
    /// Further partial declarations rendered after the class in its own source.
    mapping_extensions: Vec<ClassPart>,
    /// Companion sources in layering order.
    companions: Vec<Companion>,
    /// Optional runtime sources beyond the fixed runtime.
    runtime: Vec<(&'static str, &'static str)>,
    dynamic_input_document_outputs: bool,
}

impl Library {
    pub(crate) fn new(mapping: ClassPart, dynamic_input_document_outputs: bool) -> Self {
        Self {
            mapping,
            mapping_extensions: Vec::new(),
            companions: Vec::new(),
            runtime: Vec::new(),
            dynamic_input_document_outputs,
        }
    }

    /// Declare `members` in a further partial declaration of the mapping source.
    pub(crate) fn extend_mapping(&mut self, members: String) {
        self.mapping_extensions.push(ClassPart::members(members));
    }

    /// Add a companion source declaring `part` as a partial declaration.
    pub(crate) fn add_companion(&mut self, path: &'static str, part: ClassPart) {
        self.companions.push(Companion { path, part });
    }

    pub(crate) fn add_runtime(&mut self, sources: &[(&'static str, &'static str)]) {
        self.runtime.extend_from_slice(sources);
    }

    fn render_mapping(&self) -> String {
        // The class is partial exactly when another declaration extends it.
        let partial = !self.companions.is_empty() || !self.mapping_extensions.is_empty();
        let mut output = String::from(NAMESPACE);
        self.mapping.render(partial, &mut output);
        for extension in &self.mapping_extensions {
            output.push('\n');
            extension.render(true, &mut output);
        }
        output
    }

    pub(crate) fn into_artifacts(self) -> Result<ArtifactSet, EmitError> {
        let project = ProjectFile {
            companions: &self.companions,
        }
        .render();
        let mut files = library_files(
            project,
            self.render_mapping(),
            self.dynamic_input_document_outputs,
        )?;
        for companion in &self.companions {
            files.push(file(companion.path, companion.part.render_unit())?);
        }
        for (path, source) in self.runtime {
            files.push(file(path, source)?);
        }
        Ok(ArtifactSet::new(files)?)
    }
}

/// Artifacts for a complete mapping compilation unit that declares every
/// part it needs, such as the Basic owned mappings.
pub(crate) fn unit_artifacts(
    mapping: String,
    dynamic_input_document_outputs: bool,
) -> Result<ArtifactSet, EmitError> {
    let project = ProjectFile { companions: &[] }.render();
    let files = library_files(project, mapping, dynamic_input_document_outputs)?;
    Ok(ArtifactSet::new(files)?)
}

fn library_files(
    project: String,
    mapping: String,
    dynamic_input_document_outputs: bool,
) -> Result<Vec<GeneratedFile>, EmitError> {
    let mut files = Vec::with_capacity(
        runtime::SOURCES.len() + 3 + usize::from(dynamic_input_document_outputs),
    );
    files.push(file("Ferrule.Generated.csproj", project)?);
    files.push(file(MAPPING_PATH, mapping)?);
    files.push(file(TARGET_BUILDER_PATH, runtime::TARGET_BUILDER)?);
    for (path, source) in runtime::SOURCES {
        files.push(file(path, source)?);
    }
    if dynamic_input_document_outputs {
        let (path, source) = runtime::DYNAMIC_INPUT_DOCUMENT_OUTPUTS_SOURCE;
        files.push(file(path, source)?);
    }
    Ok(files)
}
