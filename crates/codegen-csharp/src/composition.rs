//! Structural composition of the generated C# library.
//!
//! The ordinary mapping renderer yields the `GeneratedMapping` class as typed
//! parts. Companion emitters add their own partial declarations, compile items
//! and runtime sources to a [`Library`], and only [`Library::into_artifacts`]
//! renders the mapping source and project file under the request's
//! [`Identity`]. Nothing searches or rewrites emitted text, so user literals
//! and identifiers that resemble generated declarations are never matched.

use codegen::{
    ArtifactSet, CompatibilityManifest, GENERATOR_VERSION, GeneratedFile, GenerationProfile,
    LibraryIdentity, MANIFEST_PATH, ManifestSelection, RuntimeBinding, RuntimeIdentity,
    RuntimeRecord,
};

use crate::identity::{Identity, Options};
use crate::{EmitError, file, literal, runtime};

const MAPPING_PATH: &str = "GeneratedMapping.cs";
const TARGET_BUILDER_PATH: &str = "GeneratedTargetBuilder.cs";
const COMPATIBILITY_PATH: &str = "GeneratedMapping.Compatibility.cs";
const RUNTIME_IDENTITY_PATH: &str = "Runtime/FerruleRuntimeIdentity.cs";
const RUNTIME_GLOB: &str = "Runtime/**/*.cs";
const PROJECT_HEAD: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
"#;
const PROJECT_PROPERTIES_TAIL: &str = "    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>\n  </PropertyGroup>\n  <ItemGroup>\n";
const PROJECT_TAIL: &str = "  </ItemGroup>\n</Project>\n";

fn namespace_declaration(identity: &Identity) -> String {
    format!("namespace {};\n\n", identity.root_namespace())
}

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

    fn render(&self, identity: &Identity, partial: bool, output: &mut String) {
        output.push_str(&self.types);
        output.push_str(if partial {
            "public static partial class "
        } else {
            "public static class "
        });
        output.push_str(identity.mapping_class());
        output.push_str("\n{\n");
        output.push_str(&self.members);
        output.push_str("}\n");
    }

    /// A complete compilation unit holding only this partial declaration.
    fn render_unit(&self, identity: &Identity) -> String {
        let mut output = namespace_declaration(identity);
        self.render(identity, true, &mut output);
        output
    }
}

/// A companion source file: its own compilation unit extending the mapping.
struct Companion {
    path: &'static str,
    part: ClassPart,
}

/// The project file of one generated library. Each companion is compiled
/// directly after the mapping source, the most recently layered first; an
/// included manifest is an embedded resource after the compile items.
struct ProjectFile<'a> {
    identity: &'a Identity,
    companions: &'a [Companion],
    manifest: ManifestSelection,
}

impl ProjectFile<'_> {
    fn render(&self) -> String {
        let mut output = String::from(PROJECT_HEAD);
        for (property, value) in [
            ("RootNamespace", self.identity.root_namespace()),
            ("AssemblyName", self.identity.assembly_name()),
        ] {
            output.push_str(&format!("    <{property}>{value}</{property}>\n"));
        }
        output.push_str(PROJECT_PROPERTIES_TAIL);
        let items = std::iter::once(MAPPING_PATH)
            .chain(self.companions.iter().rev().map(|companion| companion.path))
            .chain([TARGET_BUILDER_PATH, RUNTIME_GLOB]);
        for item in items {
            output.push_str("    <Compile Include=\"");
            output.push_str(item);
            output.push_str("\" />\n");
        }
        if self.manifest == ManifestSelection::Included {
            output.push_str(&format!(
                "    <EmbeddedResource Include=\"{MANIFEST_PATH}\" LogicalName=\"{MANIFEST_PATH}\" />\n"
            ));
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

    fn render_mapping(&self, identity: &Identity) -> String {
        // The class is partial exactly when another declaration extends it.
        let partial = !self.companions.is_empty() || !self.mapping_extensions.is_empty();
        let mut output = namespace_declaration(identity);
        self.mapping.render(identity, partial, &mut output);
        for extension in &self.mapping_extensions {
            output.push('\n');
            extension.render(identity, true, &mut output);
        }
        output
    }

    /// The library under the default identity and without a manifest.
    pub(crate) fn into_artifacts(self) -> Result<ArtifactSet, EmitError> {
        self.into_artifacts_with(&Options::default(), GenerationProfile::Complete)
    }

    /// The library under `options`; `profile` is recorded by an included
    /// manifest, whose compatibility check is the last layered companion.
    pub(crate) fn into_artifacts_with(
        mut self,
        options: &Options,
        profile: GenerationProfile,
    ) -> Result<ArtifactSet, EmitError> {
        let identity = &options.identity;
        let mut compatibility = None;
        if options.manifest == ManifestSelection::Included {
            self.runtime.push(runtime::COMPATIBILITY_SOURCE);
            let runtime_identity = self.runtime_identity();
            let manifest = manifest(identity, profile, runtime_identity.clone())
                .to_json()
                .map_err(EmitError::Manifest)?;
            self.add_companion(
                COMPATIBILITY_PATH,
                compatibility_part(identity, &manifest, &runtime_identity),
            );
            compatibility = Some((manifest, runtime_identity));
        }
        let project = ProjectFile {
            identity,
            companions: &self.companions,
            manifest: options.manifest,
        }
        .render();
        let mut files = library_files(
            identity,
            project,
            self.render_mapping(identity),
            self.dynamic_input_document_outputs,
        )?;
        for companion in &self.companions {
            files.push(file(companion.path, companion.part.render_unit(identity))?);
        }
        for (path, source) in self.runtime {
            files.push(file(path, source)?);
        }
        if let Some((manifest, runtime_identity)) = compatibility {
            files.push(file(
                RUNTIME_IDENTITY_PATH,
                runtime_identity_source(&runtime_identity),
            )?);
            files.push(file(MANIFEST_PATH, manifest)?);
        }
        Ok(ArtifactSet::new(files)?)
    }

    /// The identity of every runtime source this library vendors.
    fn runtime_identity(&self) -> RuntimeIdentity {
        let dynamic = self
            .dynamic_input_document_outputs
            .then_some(runtime::DYNAMIC_INPUT_DOCUMENT_OUTPUTS_SOURCE);
        RuntimeIdentity::of_sources(
            runtime::SOURCES
                .iter()
                .copied()
                .chain(dynamic)
                .chain(self.runtime.iter().copied())
                .map(|(path, source)| (path, source.as_bytes())),
        )
    }
}

fn manifest(
    identity: &Identity,
    profile: GenerationProfile,
    runtime_identity: RuntimeIdentity,
) -> CompatibilityManifest {
    CompatibilityManifest::new(
        RuntimeRecord {
            package: "Ferrule.Runtime".to_owned(),
            version: GENERATOR_VERSION.to_owned(),
            identity: runtime_identity,
            binding: RuntimeBinding::Vendored,
        },
        profile,
        LibraryIdentity::CSharp {
            assembly_name: identity.assembly_name().to_owned(),
            root_namespace: identity.root_namespace().to_owned(),
            mapping_class: identity.mapping_class().to_owned(),
        },
    )
}

/// The members that check the library on first use of the mapping class.
fn compatibility_part(identity: &Identity, manifest: &str, runtime: &RuntimeIdentity) -> ClassPart {
    let class = identity.mapping_class();
    ClassPart::members(format!(
        "    /// <summary>The compatibility manifest this library was generated with.</summary>\n    public const string CompatibilityManifest = {manifest};\n\n    /// <summary>The vendored runtime source identity this library requires.</summary>\n    public const string RuntimeIdentity = {runtime};\n\n    static {class}()\n    {{\n        global::Ferrule.Runtime.FerruleCompatibility.Verify(\n            typeof({class}).Assembly,\n            CompatibilityManifest,\n            RuntimeIdentity,\n            global::Ferrule.Runtime.FerruleRuntimeIdentity.Value);\n    }}\n",
        manifest = literal::string(manifest),
        runtime = literal::string(runtime.as_str()),
    ))
}

fn runtime_identity_source(identity: &RuntimeIdentity) -> String {
    format!(
        "namespace Ferrule.Runtime;\n\n/// <summary>Source identity of the runtime vendored into this library.</summary>\ninternal static class FerruleRuntimeIdentity\n{{\n    internal const string Value = {};\n}}\n",
        literal::string(identity.as_str())
    )
}

/// Artifacts for a complete mapping compilation unit that declares every
/// part it needs, such as the Basic owned mappings.
pub(crate) fn unit_artifacts(
    mapping: String,
    dynamic_input_document_outputs: bool,
) -> Result<ArtifactSet, EmitError> {
    let identity = Identity::default();
    let project = ProjectFile {
        identity: &identity,
        companions: &[],
        manifest: ManifestSelection::Omitted,
    }
    .render();
    let files = library_files(&identity, project, mapping, dynamic_input_document_outputs)?;
    Ok(ArtifactSet::new(files)?)
}

fn library_files(
    identity: &Identity,
    project: String,
    mapping: String,
    dynamic_input_document_outputs: bool,
) -> Result<Vec<GeneratedFile>, EmitError> {
    let mut files = Vec::with_capacity(
        runtime::SOURCES.len() + 3 + usize::from(dynamic_input_document_outputs),
    );
    files.push(file(&identity.project_path(), project)?);
    files.push(file(MAPPING_PATH, mapping)?);
    let mut target_builder = namespace_declaration(identity);
    target_builder.push_str(runtime::TARGET_BUILDER_BODY);
    files.push(file(TARGET_BUILDER_PATH, target_builder)?);
    for (path, source) in runtime::SOURCES {
        files.push(file(path, source)?);
    }
    if dynamic_input_document_outputs {
        let (path, source) = runtime::DYNAMIC_INPUT_DOCUMENT_OUTPUTS_SOURCE;
        files.push(file(path, source)?);
    }
    Ok(files)
}
