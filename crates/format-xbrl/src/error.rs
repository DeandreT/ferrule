use ir::ScalarType;
use mapping::XbrlFactType;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XbrlFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XBRL parse error: {0}")]
    Parse(#[from] roxmltree::Error),
    #[error("XBRL input exceeds the {limit} MiB size limit")]
    InputLimit { limit: usize },
    #[error("expected an XBRL instance root, found `{found}`")]
    UnexpectedRoot { found: String },
    #[error("XBRL table schema must contain exactly one repeating group")]
    InvalidTableSchema,
    #[error("XBRL schema recursion exceeds the {limit}-level limit")]
    SchemaDepth { limit: usize },
    #[error("XBRL instance exceeds the {limit} context limit")]
    ContextLimit { limit: usize },
    #[error("XBRL instance exceeds the {limit} fact limit")]
    FactLimit { limit: usize },
    #[error("XBRL context has a missing or duplicate id `{id}`")]
    InvalidContextId { id: String },
    #[error("XBRL context `{context}` has duplicate `{concept}` facts")]
    DuplicateFact { context: String, concept: String },
    #[error("cannot parse `{value}` as {ty:?} for XBRL field `{name}`")]
    ScalarParse {
        name: String,
        ty: ScalarType,
        value: String,
    },
    #[error("XBRL write error: {0}")]
    Write(#[from] quick_xml::Error),
    #[error("XBRL target schema and instance disagree at `{path}`")]
    TargetShape { path: String },
    #[error("XBRL target row `{path}` has no entity identifier or period")]
    MissingContext { path: String },
    #[error("XBRL target row `{path}` contains no concrete facts")]
    MissingFacts { path: String },
    #[error("XBRL target has no namespace binding for fact `{path}`")]
    MissingFactNamespace { path: String },
    #[error("XBRL target fact `{path}` has no {fact_type:?} unit")]
    MissingFactUnit {
        path: String,
        fact_type: XbrlFactType,
    },
    #[error("XBRL target has multiple candidate {fact_type:?} units")]
    AmbiguousFactUnit { fact_type: XbrlFactType },
    #[error("XBRL target unit `{id}` has no measure or complete divide definition")]
    InvalidUnit { id: String },
    #[error("XBRL target unit id `{id}` is duplicated")]
    DuplicateUnit { id: String },
}
