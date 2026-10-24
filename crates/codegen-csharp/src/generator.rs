//! C# generation requests under a configured identity and manifest selection.

use codegen::{
    ArtifactSet, CompanionSelection, CsvDocumentBoundaryPolicy, CsvJsonBoundaryPolicy,
    CsvOutputPolicy, CsvX12BoundaryPolicy, DocumentProgram, GenerationProfile, Program,
    StaticDocumentBoundaryPolicy, X12BoundaryPolicy,
};

use crate::{
    CsvDocumentEmitError, CsvJsonEmitError, CsvX12EmitError, EmitError, Json5EmitError, Options,
    PrimaryDocumentEmitError, StaticDocumentEmitError, X12EmitError, csv_document, csv_json,
    csv_output, csv_x12, json5_output, primary_csv_document, primary_xml_document,
    static_documents, x12_output,
};

/// One C# generation request. Each method runs the emitter of the free
/// function with the same name; under the default [`Options`] it returns
/// exactly that function's artifacts. A configured identity names the
/// project file, assembly, root namespace and mapping class; an included
/// manifest adds `ferrule-manifest.json` as an embedded resource, the vendored
/// runtime identity and a compatibility check that runs before the mapping
/// class is first used.
#[derive(Debug, Clone, Copy)]
pub struct Generator<'a> {
    options: &'a Options,
}

impl<'a> Generator<'a> {
    pub fn new(options: &'a Options) -> Self {
        Self { options }
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
        let profile = match companions {
            CompanionSelection::Complete => GenerationProfile::Complete,
            CompanionSelection::TypedOnly => GenerationProfile::TypedOnly,
        };
        crate::library_with_companions(program, companions)?
            .into_artifacts_with(self.options, profile)
    }

    /// [`crate::emit_with_csv_output`].
    pub fn emit_with_csv_output(
        &self,
        program: &Program,
        policy: &CsvOutputPolicy,
    ) -> Result<ArtifactSet, EmitError> {
        csv_output::emit(program, policy, self.options)
    }

    /// [`crate::emit_with_csv_document_adapters`].
    pub fn emit_with_csv_document_adapters(
        &self,
        program: &Program,
        policy: &CsvDocumentBoundaryPolicy,
    ) -> Result<ArtifactSet, CsvDocumentEmitError> {
        csv_document::emit(program, policy, self.options)
    }

    /// [`crate::emit_with_primary_csv_documents`].
    pub fn emit_with_primary_csv_documents(
        &self,
        document: &DocumentProgram,
        policy: &CsvDocumentBoundaryPolicy,
    ) -> Result<ArtifactSet, CsvDocumentEmitError> {
        primary_csv_document::emit(document, policy, self.options)
    }

    /// [`crate::emit_with_primary_xml_documents`].
    pub fn emit_with_primary_xml_documents(
        &self,
        document: &DocumentProgram,
    ) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
        primary_xml_document::emit(document, self.options)
    }

    /// [`crate::emit_with_primary_json_documents`].
    pub fn emit_with_primary_json_documents(
        &self,
        document: &DocumentProgram,
    ) -> Result<ArtifactSet, PrimaryDocumentEmitError> {
        crate::primary_json_document::emit(document, self.options)
    }

    /// [`crate::emit_with_json5`].
    pub fn emit_with_json5(&self, program: &Program) -> Result<ArtifactSet, Json5EmitError> {
        json5_output::emit(program, self.options)
    }

    /// [`crate::emit_with_x12`].
    pub fn emit_with_x12(
        &self,
        program: &Program,
        policy: &X12BoundaryPolicy,
    ) -> Result<ArtifactSet, X12EmitError> {
        x12_output::emit(program, policy, self.options)
    }

    /// [`crate::emit_with_static_document_adapters`].
    pub fn emit_with_static_document_adapters(
        &self,
        program: &Program,
        policy: &StaticDocumentBoundaryPolicy,
    ) -> Result<ArtifactSet, StaticDocumentEmitError> {
        static_documents::emit(program, policy, self.options)
    }

    /// [`crate::emit_with_csv_x12_adapters`].
    pub fn emit_with_csv_x12_adapters(
        &self,
        program: &Program,
        policy: &CsvX12BoundaryPolicy,
    ) -> Result<ArtifactSet, CsvX12EmitError> {
        csv_x12::emit(program, policy, self.options)
    }

    /// [`crate::emit_with_csv_json_adapters`].
    pub fn emit_with_csv_json_adapters(
        &self,
        program: &Program,
        policy: &CsvJsonBoundaryPolicy,
    ) -> Result<ArtifactSet, CsvJsonEmitError> {
        csv_json::emit(program, policy, self.options)
    }
}
