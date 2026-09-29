//! Best-effort interop with `.mfd` mapping designs.
//!
//! `.mfd` files are XML documents describing schema components (entry trees
//! with integer port keys), function components, and a flat port-to-port
//! connection graph. [`import`] converts the supported subset -- XML
//! components (resolvable XSDs incl. local includes/imports, bounded DTDs with
//! internal content-model parameter entities, attributes, simple-content
//! values, and element refs), requestless static
//! HTTP GET calls with typed XML responses, captured-response HTTP POST calls
//! with typed JSON request/response projections, JSON
//! components (JSON Schema incl. local `$ref`, or the entry tree as
//! fallback), CSV text components (inline delimiter/header settings), and EDI
//! text component graphs whose external configuration directories or adjacent
//! bounded ZIP packages compile into portable positional runtime schemas
//! (with a non-executable entry-tree fallback when no configuration resolves),
//! proto2 binary target components (typed from their referenced `.proto`),
//! single-table SQLite database components (schema introspected from the
//! referenced database when reachable), including structured XML documents
//! serialized compactly into text columns and validated local relation
//! declarations when physical foreign keys are absent, the common core functions,
//! structured XML string serializers with subtree, namespace, and declaration
//! semantics,
//! aggregates (count/sum/avg/min/max/string-join/item-at, converted to
//! collection-reducing graph nodes), bounded one-parameter aggregate templates
//! from adjacent XSLT extension modules, bounded invariant numeric-format
//! wrappers from adjacent C#/Java source, scalar arithmetic functions from
//! adjacent XQuery modules, constants, if-else, value-map, and
//! filter- and group-by-driven iteration, including filters applied to formed
//! groups -- into a
//! ferrule [`mapping::Project`], collecting a warning for every construct
//! it has to skip rather than failing. [`export`] writes a ferrule project
//! back out as a `.mfd` (plus generated XSD / JSON Schema files next to
//! it) for the exportable subset, picking each side's component family from the
//! project's instance-path extension. Static HTTP XML sources, complete
//! document-copy edges, and structured XML string serializers also round-trip
//! through their canonical components.
//! [`preflight_export`] reports known Ferrule extension dependencies and
//! captured-response behavior differences. [`ExportProfile::NativeMfd`]
//! rejects those designs before publication; ordinary [`export`] preserves
//! Ferrule's existing round-trip representation.
//! [`import_with_profile`] can require an executable project. It rejects any
//! import warning, unresolved runtime resource, or engine validation issue
//! while ordinary [`import`] keeps a partial design available for repair.
//! A source-less design driven by one opaque user call can retain that call's
//! JSON-shaped public result as a typed external input that requires a local
//! captured result instance at run time.
//!
//! The format knowledge comes from reading real `.mfd` files; nothing here
//! embeds or copies proprietary reference content. Ferrule is independent of
//! the application that produces those files.

mod canonical_function;
mod export;
mod import;
mod resource;

pub use export::{
    ExportCompatibility, ExportCompatibilityFeature, ExportCompatibilityIssue, ExportProfile,
    ExportReport, export, export_with_profile, preflight_export,
};
pub use import::{
    ImportIssue, ImportIssueKind, ImportOptions, ImportOutcome, ImportProfile, ImportReport,
    Imported, ImportedPipeline, assess_import, import, import_pipeline,
    import_pipeline_with_options, import_with_options, import_with_profile,
};
pub use resource::PackageManifest;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MfdError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml parse error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("schema export error: {0}")]
    SchemaExport(#[from] format_xml::XmlFormatError),
    #[error("JSON schema export error: {0}")]
    JsonSchemaExport(#[from] format_json::JsonFormatError),
    #[error("not an .mfd design: {0}")]
    NotMfd(&'static str),
    #[error("cannot import: {0}")]
    UnsupportedImport(String),
    #[error("resource resolution error: {0}")]
    Resource(String),
    #[error("cannot export: {0}")]
    Unsupported(String),
    #[error("native .mfd export rejected: {0}")]
    IncompatibleExport(Box<ExportReport>),
    #[error("executable import rejected: {0}")]
    IncompatibleImport(Box<ImportReport>),
}
