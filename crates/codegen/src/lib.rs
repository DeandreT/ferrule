//! Backend-neutral code-generation foundations.
//!
//! [`lower`] converts the deliberately small supported mapping subset into a
//! deterministic [`Program`]. Backend emitters can then return an
//! [`ArtifactSet`] without owning filesystem policy.

mod artifact;
mod diagnostic;
mod lower;
mod model;
mod validate;

pub use artifact::{
    ArtifactPath, ArtifactPathError, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError,
    GeneratedFile,
};
pub use diagnostic::{
    Diagnostic, LowerError, ProjectFeature, ScopeConstructionKind, ScopeFeature,
    UnsupportedNodeKind, UnsupportedSequenceKind,
};
pub use lower::lower;
pub use model::{
    AggregateFunction, AggregateValue, Binding, Expression, ExpressionNode, GeneratedSequence,
    IterationOutput, IterationPlan, IterationSource, Program, SUPPORTED_SCALAR_CALLS,
    ScalarFunction, SequenceWindow, SortFilterOrder, SortKey, SortPlan, SourceIteration,
    TargetScope,
};
pub use validate::{
    ProgramValidationError, SequenceExpressionRole, SequenceOwner, validate_program,
};

#[cfg(test)]
mod tests;
