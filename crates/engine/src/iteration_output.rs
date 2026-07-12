use ir::Instance;
use mapping::{IterationOutput, Scope};

use super::{EngineError, dynamic_target::merge_dynamic_fragments};

pub(super) fn finalize_scope_output(
    scope: &Scope,
    produced: Vec<Instance>,
) -> Result<Instance, EngineError> {
    let iterates = scope.source.is_some() || scope.sequence.is_some();
    if !iterates {
        if scope.iteration_output == IterationOutput::First {
            return Err(EngineError::FirstOutputWithoutIteration);
        }
        return produced
            .into_iter()
            .next()
            .ok_or(EngineError::FilteredNonRepeatingScope);
    }
    if scope.merge_dynamic_fields {
        if scope.iteration_output != IterationOutput::Repeated {
            return Err(EngineError::ConflictingIterationOutput);
        }
        return merge_dynamic_fragments(produced);
    }
    match scope.iteration_output {
        IterationOutput::Repeated => Ok(Instance::Repeated(produced)),
        IterationOutput::MappedSequence => Ok(Instance::MappedSequence(produced)),
        IterationOutput::First => Ok(produced
            .into_iter()
            .next()
            .unwrap_or_else(|| Instance::Group(Vec::new()))),
    }
}
