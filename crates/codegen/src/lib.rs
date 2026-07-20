//! Backend-neutral code-generation foundations.
//!
//! [`lower`] converts the deliberately small supported mapping subset into a
//! deterministic [`Program`]. Backend emitters can then return an
//! [`ArtifactSet`] without owning filesystem policy.

mod artifact;
mod diagnostic;
mod join;
mod lower;
mod model;
mod validate;

pub use artifact::{
    ArtifactPath, ArtifactPathError, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError,
    GeneratedFile,
};
pub use diagnostic::{
    Diagnostic, FailureRuleFeature, LowerError, ScopeConstructionKind, ScopeFeature,
    UnsupportedNodeKind, UnsupportedSequenceKind,
};
pub use join::{
    InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan, JoinPlanError, JoinSource,
    JoinSourceCardinality,
};
pub use lower::lower;
pub use model::{
    AggregateFunction, AggregateValue, Binding, Expression, ExpressionNode, FailureIteration,
    FailureRule, FailureSelection, GeneratedSequence, GroupingPlan, IterationOutput, IterationPlan,
    IterationSource, NamedSourceProgram, NamedTargetProgram, Program, RuntimeValue,
    SUPPORTED_SCALAR_CALLS, ScalarFunction, SequenceWindow, SortFilterOrder, SortKey, SortPlan,
    SourceIteration, TargetConstruction, TargetScope,
};
pub use validate::{
    GroupingExpressionRole, JoinKeySide, ProgramValidationError, RecursiveSequencePathRole,
    SequenceExpressionRole, SequenceOwner, validate_program,
};

#[cfg(test)]
mod tests;
