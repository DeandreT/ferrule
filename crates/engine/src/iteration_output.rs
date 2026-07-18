use ir::Instance;
use mapping::{IterationOutput, Scope};

use super::{EngineError, dynamic_target::merge_dynamic_fragments};

pub(super) fn finalize_scope_output(
    scope: &Scope,
    target_repeating: bool,
    produced: Vec<Instance>,
) -> Result<Instance, EngineError> {
    let iterates = scope.iterates();
    if !iterates {
        if scope.iteration_output == IterationOutput::First {
            return Err(EngineError::FirstOutputWithoutIteration);
        }
        let produced = produced
            .into_iter()
            .next()
            .ok_or(EngineError::FilteredNonRepeatingScope)?;
        return Ok(if target_repeating {
            Instance::Repeated(vec![produced])
        } else {
            produced
        });
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
