use super::*;
use crate::IterationOutput;

fn segment(output: IterationOutput) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: output == IterationOutput::Repeated,
        iteration: Some(IterationPlan::new(
            SourceIteration::new(vec!["Rows".into()]),
            None,
            None,
            Vec::new(),
            output,
        )),
        construction: TargetConstruction::Group,
        bindings: vec![Binding {
            target_field: "Value".into(),
            expression: 1,
            target_type: ScalarType::Int,
            repeating: false,
        }],
        children: Vec::new(),
    }
}

fn sequence_program(output: IterationOutput) -> Program {
    let mut program = program();
    let repeating = output == IterationOutput::Repeated;
    let row = SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    set_target_fields(
        &mut program,
        vec![if repeating { row.repeating() } else { row }],
    );
    program.root.bindings.clear();
    program.root.children = vec![TargetScope {
        target_field: "Row".into(),
        repeating,
        iteration: Some(IterationPlan::concatenate(
            segment(output),
            vec![segment(output)],
            output,
        )),
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    }];
    program
}

#[test]
fn accepts_nonempty_ordered_scope_sequences() {
    assert_eq!(
        validate_program(&sequence_program(IterationOutput::Repeated)),
        Ok(())
    );
    assert_eq!(
        validate_program(&sequence_program(IterationOutput::MappedSequence)),
        Ok(())
    );
}

#[test]
fn rejects_scope_sequence_wrapper_content() {
    let mut program = sequence_program(IterationOutput::Repeated);
    program.root.children[0].bindings.push(Binding {
        target_field: "Value".into(),
        expression: 1,
        target_type: ScalarType::Int,
        repeating: false,
    });

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidScopeSequenceWrapper {
            target_path: vec!["Row".into()],
        })
    );
}

#[test]
fn rejects_scope_sequence_segment_output_mismatches() {
    let mut program = sequence_program(IterationOutput::MappedSequence);
    program.root.children[0].iteration = Some(IterationPlan::concatenate(
        segment(IterationOutput::Repeated),
        vec![segment(IterationOutput::MappedSequence)],
        IterationOutput::MappedSequence,
    ));

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidScopeSequenceSegment {
            target_path: vec!["Row".into()],
            segment: 0,
        })
    );
}

#[test]
fn rejects_scope_sequences_targeting_scalars() {
    let mut program = sequence_program(IterationOutput::Repeated);
    set_target_fields(
        &mut program,
        vec![SchemaNode::scalar("Row", ScalarType::Int).repeating()],
    );

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::ScopeSequenceRequiresGroupTarget {
            target_path: vec!["Row".into()],
        })
    );
}
