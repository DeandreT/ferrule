//! Rust generation requests and their optional compatibility manifest.
//!
//! [`Generator`] runs the same emitters as the free functions. With
//! [`ManifestSelection::Included`] it adds `ferrule-manifest.json` and
//! `src/ferrule_compatibility.rs` to the complete library and declares that
//! module at the end of `src/lib.rs`; nothing else changes. The module embeds
//! the manifest and refuses, at compile time, a bound `codegen-runtime` whose
//! source identity differs from the one the library was generated for, or a
//! manifest file that differs from the embedded record.

use codegen::{
    ArtifactPath, ArtifactSet, CompanionSelection, CompatibilityManifest,
    CsvDocumentBoundaryPolicy, CsvOutputPolicy, DocumentProgram, GENERATOR_VERSION, GeneratedFile,
    GenerationProfile, LibraryIdentity, MANIFEST_PATH, ManifestSelection, Program, RuntimeBinding,
    RuntimeRecord, X12BoundaryPolicy,
};

use crate::identity::{PackageName, runtime_identity};
use crate::{
    CsvDocumentEmitError, EmitError, Json5EmitError, Options, PrimaryDocumentEmitError,
    RuntimeDependency, X12EmitError, rust_string,
};

const MODULE_PATH: &str = "src/ferrule_compatibility.rs";
const LIBRARY_PATH: &str = "src/lib.rs";
const MODULE_DECLARATION: &str = "\nmod ferrule_compatibility;\npub use ferrule_compatibility::{COMPATIBILITY_MANIFEST, RUNTIME_IDENTITY};\n";

/// One Rust generation request: the project options plus the manifest
/// selection. Without a manifest every method returns exactly the artifacts
/// of the corresponding free function.
#[derive(Debug, Clone, Copy)]
pub struct Generator<'a> {
    options: &'a Options,
    manifest: ManifestSelection,
}

impl<'a> Generator<'a> {
    pub fn new(options: &'a Options) -> Self {
        Self {
            options,
            manifest: ManifestSelection::Omitted,
        }
    }

    pub fn with_manifest(self, manifest: ManifestSelection) -> Self {
        Self { manifest, ..self }
    }

    /// [`crate::emit`].
    pub fn emit(&self, program: &Program) -> Result<ArtifactSet, EmitError> {
        self.emit_with_companions(program, CompanionSelection::Complete)
    }

    /// [`crate::emit_with_companions`].
    pub fn emit_with_companions(
        &self,
        program: &Program,
        companions: CompanionSelection,
    ) -> Result<ArtifactSet, EmitError> {
        let artifacts = crate::emit_with_companions(program, self.options, companions)?;
        let profile = match companions {
            CompanionSelection::Complete => GenerationProfile::Complete,
            CompanionSelection::TypedOnly => GenerationProfile::TypedOnly,
        };
        self.finish(artifacts, profile)
    }

    /// [`crate::emit_with_csv_output`].
    pub fn emit_with_csv_output(
        &self,
        program: &Program,
        policy: &CsvOutputPolicy,
    ) -> Result<ArtifactSet, EmitError> {
        let artifacts = crate::emit_with_csv_output(program, self.options, policy)?;
        self.finish(artifacts, GenerationProfile::CsvOutput)
    }

    /// [`crate::emit_with_csv_document_adapters`].
    pub fn emit_with_csv_document_adapters(
        &self,
        program: &Program,
        policy: &CsvDocumentBoundaryPolicy,
    ) -> Result<ArtifactSet, CsvDocumentEmitError> {
        let artifacts = crate::emit_with_csv_document_adapters(program, self.options, policy)?;
        let profile = if policy.target.raw_delimited_crlf {
            GenerationProfile::RawDelimitedDocument
        } else {
            GenerationProfile::CsvDocument
        };
        self.finish(artifacts, profile)
            .map_err(CsvDocumentEmitError::Emit)
    }

    /// [`crate::emit_with_primary_csv_documents`].
    pub fn emit_with_primary_csv_documents(
        &self,
        document: &DocumentProgram,
        policy: &CsvDocumentBoundaryPolicy,
    ) -> Result<ArtifactSet, CsvDocumentEmitError> {
        let artifacts = crate::emit_with_primary_csv_documents(document, self.options, policy)?;
        let profile = if policy.target.raw_delimited_crlf {
            GenerationProfile::PrimaryRawDelimitedDocument
        } else {
            GenerationProfile::PrimaryCsvDocument
        };
        self.finish(artifacts, profile)
            .map_err(CsvDocumentEmitError::Emit)
    }

    /// [`crate::emit_with_primary_xml_documents`].
    pub fn emit_with_primary_xml_documents(
        &self,
        document: &DocumentProgram,
    ) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
        let artifacts = crate::emit_with_primary_xml_documents(document, self.options)?;
        self.finish(artifacts, GenerationProfile::PrimaryXmlDocument)
            .map_err(PrimaryDocumentEmitError::Emit)
    }

    /// [`crate::emit_with_primary_json_documents`].
    pub fn emit_with_primary_json_documents(
        &self,
        document: &DocumentProgram,
    ) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
        let artifacts = crate::emit_with_primary_json_documents(document, self.options)?;
        self.finish(artifacts, GenerationProfile::PrimaryJsonDocument)
            .map_err(PrimaryDocumentEmitError::Emit)
    }

    /// [`crate::emit_with_json5`].
    pub fn emit_with_json5(&self, program: &Program) -> Result<ArtifactSet, Json5EmitError> {
        let artifacts = crate::emit_with_json5(program, self.options)?;
        self.finish(artifacts, GenerationProfile::Json5)
            .map_err(Json5EmitError::Ordinary)
    }

    /// [`crate::emit_with_x12`].
    pub fn emit_with_x12(
        &self,
        program: &Program,
        policy: &X12BoundaryPolicy,
    ) -> Result<ArtifactSet, X12EmitError> {
        let artifacts = crate::emit_with_x12(program, self.options, policy)?;
        self.finish(artifacts, GenerationProfile::X12)
            .map_err(X12EmitError::Ordinary)
    }

    /// The manifest this request records for `profile`.
    pub fn manifest(&self, profile: GenerationProfile) -> Result<CompatibilityManifest, EmitError> {
        let package = PackageName::new(&self.options.package_name)
            .map_err(|_| EmitError::InvalidPackageName(self.options.package_name.clone()))?;
        let binding = match &self.options.runtime_dependency {
            RuntimeDependency::Path(path) => RuntimeBinding::Path { path: path.clone() },
            RuntimeDependency::Version(version) => RuntimeBinding::Version {
                version: version.clone(),
            },
            RuntimeDependency::Git {
                repository,
                revision,
            } => RuntimeBinding::Git {
                repository: repository.clone(),
                revision: revision.clone(),
            },
        };
        Ok(CompatibilityManifest::new(
            RuntimeRecord {
                package: "codegen-runtime".to_owned(),
                version: GENERATOR_VERSION.to_owned(),
                identity: runtime_identity().map_err(EmitError::Manifest)?,
                binding,
            },
            profile,
            LibraryIdentity::Rust {
                package_name: package.as_str().to_owned(),
                crate_name: package.crate_name(),
            },
        ))
    }

    fn finish(
        &self,
        artifacts: ArtifactSet,
        profile: GenerationProfile,
    ) -> Result<ArtifactSet, EmitError> {
        if self.manifest == ManifestSelection::Omitted {
            return Ok(artifacts);
        }
        let manifest = self.manifest(profile)?;
        let json = manifest.to_json().map_err(EmitError::Manifest)?;
        let mut files = artifacts.into_files();
        for file in &mut files {
            if file.path.as_str() == LIBRARY_PATH {
                file.contents
                    .extend_from_slice(MODULE_DECLARATION.as_bytes());
            }
        }
        files.push(GeneratedFile::new(
            ArtifactPath::new(MODULE_PATH)?,
            render_module(&json, manifest.runtime.identity.as_str()),
        ));
        files.push(GeneratedFile::new(ArtifactPath::new(MANIFEST_PATH)?, json));
        Ok(ArtifactSet::new(files)?)
    }
}

fn render_module(manifest: &str, runtime: &str) -> String {
    format!(
        "// Generated by ferrule. Do not edit.\n\
         //! Compatibility record of this generated library.\n\n\
         /// The compatibility manifest this library was generated with; it\n\
         /// equals `ferrule-manifest.json`.\n\
         pub const COMPATIBILITY_MANIFEST: &str = {manifest};\n\n\
         /// The `codegen-runtime` source identity this library requires.\n\
         pub const RUNTIME_IDENTITY: &str = {runtime_literal};\n\n\
         const _: () = assert!(\n    codegen_runtime::runtime_identity_matches(RUNTIME_IDENTITY),\n    {runtime_refusal}\n);\n\n\
         const _: () = assert!(\n    same_bytes(include_bytes!(\"../{MANIFEST_PATH}\"), COMPATIBILITY_MANIFEST.as_bytes()),\n    \"ferrule: {MANIFEST_PATH} differs from the manifest this library was generated with\"\n);\n\n\
         const fn same_bytes(left: &[u8], right: &[u8]) -> bool {{\n    if left.len() != right.len() {{\n        return false;\n    }}\n    let mut index = 0;\n    while index < left.len() {{\n        if left[index] != right[index] {{\n            return false;\n        }}\n        index += 1;\n    }}\n    true\n}}\n",
        manifest = rust_string(manifest),
        runtime_literal = rust_string(runtime),
        runtime_refusal = rust_string(&format!(
            "ferrule: this generated mapping requires codegen-runtime {runtime}; the bound runtime has a different source identity. Bind the runtime it was generated for or regenerate the mapping."
        )),
    )
}
