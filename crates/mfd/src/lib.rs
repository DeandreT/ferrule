//! Best-effort interop with MapForce `.mfd` mapping designs.
//!
//! `.mfd` files are XML documents describing schema components (entry trees
//! with integer port keys), function components, and a flat port-to-port
//! connection graph. [`import`] converts the supported subset -- XML
//! components (resolvable XSDs incl. local includes/imports, bounded DTDs,
//! attributes, simple-content values, and element refs), requestless static
//! HTTP GET calls with typed XML responses, captured-response HTTP POST calls
//! with typed JSON request/response projections, JSON
//! components (JSON Schema incl. local `$ref`, or the entry tree as
//! fallback), CSV text components (inline delimiter/header settings), and EDI
//! text component graphs (entry-tree fallback, explicitly non-executable
//! until a positional EDI schema is supplied),
//! proto2 binary target components (typed from their referenced `.proto`),
//! single-table SQLite database components (schema introspected from the
//! referenced database when reachable), the common core functions,
//! aggregates (count/sum/avg/min/max/string-join/item-at, converted to
//! collection-reducing graph nodes), constants, if-else, value-map, and
//! filter- and group-by-driven iteration -- into a
//! ferrule [`mapping::Project`], collecting a warning for every construct
//! it has to skip rather than failing. [`export`] writes a ferrule project
//! back out as a `.mfd` (plus generated XSD / JSON Schema files next to
//! it) for the exportable subset, picking each side's component family from the
//! project's instance-path extension. Static HTTP XML sources and complete
//! document-copy edges also round-trip through their canonical components.
//! Captured-response boundaries reject export rather than publishing a
//! design that would imply live POST or opaque UDF execution.
//! A source-less design driven by one opaque user call can retain that call's
//! JSON-shaped public result as a typed external input that requires a local
//! captured result instance at run time.
//!
//! The format knowledge comes from reading real `.mfd` files; nothing here
//! embeds or copies proprietary reference content. MapForce is a trademark of
//! its owner; ferrule is unaffiliated.

mod canonical_function;
mod export;
mod import;

pub use export::export;
pub use import::{Imported, import};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MfdError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml parse error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("schema export error: {0}")]
    SchemaExport(#[from] format_xml::XmlFormatError),
    #[error("not a MapForce design: {0}")]
    NotMfd(&'static str),
    #[error("cannot import: {0}")]
    UnsupportedImport(String),
    #[error("cannot export: {0}")]
    Unsupported(String),
}
