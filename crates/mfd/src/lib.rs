//! Best-effort interop with MapForce `.mfd` mapping designs.
//!
//! `.mfd` files are XML documents describing schema components (entry trees
//! with integer port keys), function components, and a flat port-to-port
//! connection graph. [`import`] converts the supported subset -- XML
//! components (resolvable XSDs incl. attributes and element refs), JSON
//! components (JSON Schema incl. local `$ref`, or the entry tree as
//! fallback), CSV text components (inline delimiter/header settings),
//! single-table SQLite database components (schema introspected from the
//! referenced database when reachable), the common core functions,
//! aggregates (count/sum/avg/min/max/string-join/item-at, converted to
//! collection-reducing graph nodes), constants, if-else, value-map, and
//! filter- and group-by-driven iteration -- into a
//! ferrule [`mapping::Project`], collecting a warning for every construct
//! it has to skip rather than failing. [`export`] writes a ferrule project
//! back out as a `.mfd` (plus generated XSD / JSON Schema files next to
//! it) for the same subset, picking each side's component family from the
//! project's instance-path extension.
//!
//! The format knowledge comes from reading real `.mfd` files; nothing here
//! embeds or copies ReferenceSamples content. MapForce is a trademark of ReferenceSamples
//! GmbH; ferrule is unaffiliated.

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
    #[error("not a MapForce design: {0}")]
    NotMfd(&'static str),
    #[error("cannot export: {0}")]
    Unsupported(String),
}
