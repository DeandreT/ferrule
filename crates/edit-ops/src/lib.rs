//! Ferrule-owned, deterministic edits for native and WebAssembly hosts.
//! Operations plan existing editor commands; native file bytes are never patched.

mod history;
mod identity;
mod operation;
mod planner;
mod scalar;

pub use history::EditSession;
pub use identity::{FunctionId, Index, NodeId};
pub use operation::*;
pub use planner::plan;
pub use scalar::Scalar;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("invalid scalar {scalar:?}")]
    InvalidScalar { scalar: Scalar },
    #[error("node {0:?} does not exist")]
    MissingNode(NodeId),
    #[error("node identity {0:?} is already occupied")]
    OccupiedNode(NodeId),
    #[error("node {node:?} has no editable input {input:?}")]
    Input { node: NodeId, input: Index },
    #[error("connecting {source_node:?} to {node:?} would create a cycle")]
    Cycle { node: NodeId, source_node: NodeId },
    #[error("unknown builtin function {function}")]
    UnknownFunction { function: String },
    #[error("builtin {function} does not accept {count} arguments")]
    Arity { function: String, count: usize },
    #[error("failure rule {0:?} does not exist")]
    MissingRule(Index),
    #[error("scope does not exist: {0:?}")]
    MissingScope(ScopeRef),
    #[error("binding does not exist: {0:?}")]
    MissingBinding(BindingRef),
    #[error("edit rejected: {0}")]
    Invalid(String),
    #[error(transparent)]
    Command(#[from] editor_ui::GraphEditError),
    #[error("operation {index:?}: {source}")]
    Replay {
        index: Index,
        #[source]
        source: Box<EditError>,
    },
}
