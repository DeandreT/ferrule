use crate::compiler::{Instruction, Program};

pub(crate) fn is_match(program: &Program, value: &str) -> bool {
    let scalar_count = value.chars().count();
    let final_terminator_start = final_terminator_start(value, scalar_count);
    let capacity = program.instructions.len();
    let mut current = Vec::with_capacity(capacity);
    let mut next = Vec::with_capacity(capacity);
    let mut current_marks = vec![0_u32; capacity];
    let mut next_marks = vec![0_u32; capacity];
    let mut current_generation = 1_u32;
    let mut next_generation = 1_u32;
    let mut closure_stack = Vec::with_capacity(capacity);

    add_state(
        program,
        program.start,
        0,
        scalar_count,
        final_terminator_start,
        &mut current,
        &mut current_marks,
        current_generation,
        &mut closure_stack,
    );
    if contains_match(program, &current) {
        return true;
    }

    let mut position = 0_usize;
    for value in value.chars() {
        position = position.saturating_add(1);
        next.clear();
        advance_generation(&mut next_generation, &mut next_marks);

        for state in &current {
            if let Some(Instruction::Consume {
                matcher,
                next: target,
            }) = program.instructions.get(*state)
                && matcher.matches(value)
            {
                add_state(
                    program,
                    *target,
                    position,
                    scalar_count,
                    final_terminator_start,
                    &mut next,
                    &mut next_marks,
                    next_generation,
                    &mut closure_stack,
                );
            }
        }

        std::mem::swap(&mut current, &mut next);
        std::mem::swap(&mut current_marks, &mut next_marks);
        std::mem::swap(&mut current_generation, &mut next_generation);

        add_state(
            program,
            program.start,
            position,
            scalar_count,
            final_terminator_start,
            &mut current,
            &mut current_marks,
            current_generation,
            &mut closure_stack,
        );
        if contains_match(program, &current) {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn add_state(
    program: &Program,
    start: usize,
    position: usize,
    scalar_count: usize,
    final_terminator_start: Option<usize>,
    states: &mut Vec<usize>,
    marks: &mut [u32],
    generation: u32,
    stack: &mut Vec<usize>,
) {
    stack.clear();
    stack.push(start);
    while let Some(index) = stack.pop() {
        let Some(mark) = marks.get_mut(index) else {
            continue;
        };
        if *mark == generation {
            continue;
        }
        *mark = generation;
        let Some(instruction) = program.instructions.get(index) else {
            continue;
        };
        match instruction {
            Instruction::Consume { .. } | Instruction::Match => states.push(index),
            Instruction::Split { first, second } => {
                stack.push(*second);
                stack.push(*first);
            }
            Instruction::Jump { next } => stack.push(*next),
            Instruction::AssertStart { next } if position == 0 => stack.push(*next),
            Instruction::AssertEnd { next }
                if position == scalar_count || final_terminator_start == Some(position) =>
            {
                stack.push(*next);
            }
            Instruction::AssertStart { .. } | Instruction::AssertEnd { .. } => {}
        }
    }
}

fn contains_match(program: &Program, states: &[usize]) -> bool {
    states
        .iter()
        .any(|state| matches!(program.instructions.get(*state), Some(Instruction::Match)))
}

fn final_terminator_start(value: &str, scalar_count: usize) -> Option<usize> {
    if value.ends_with("\r\n") {
        return scalar_count.checked_sub(2);
    }
    match value.chars().next_back() {
        Some('\n' | '\r' | '\u{2028}' | '\u{2029}') => scalar_count.checked_sub(1),
        _ => None,
    }
}

fn advance_generation(generation: &mut u32, marks: &mut [u32]) {
    if *generation == u32::MAX {
        marks.fill(0);
        *generation = 1;
    } else {
        *generation += 1;
    }
}
