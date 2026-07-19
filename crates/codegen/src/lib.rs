//! Backend-neutral code-generation foundations.
//!
//! [`lower`] converts the deliberately small supported mapping subset into a
//! deterministic [`Program`]. Backend emitters can then return an
//! [`ArtifactSet`] without owning filesystem policy.

mod artifact;
mod diagnostic;
mod lower;
mod model;

pub use artifact::{
    ArtifactPath, ArtifactPathError, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError,
    GeneratedFile,
};
pub use diagnostic::{
    Diagnostic, LowerError, ProjectFeature, ScopeConstructionKind, ScopeFeature,
    UnsupportedNodeKind,
};
pub use lower::lower;
pub use model::{Binding, Expression, ExpressionNode, Program, TargetScope};

#[cfg(test)]
mod tests;
