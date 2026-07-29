use std::sync::Arc;

use crate::{
    CompileError, CompileErrorKind, MAX_INSTRUCTIONS,
    parser::{CharacterClass, Expression},
};

const UNSET: usize = usize::MAX;

#[derive(Clone, Debug)]
pub(crate) struct Program {
    pub(crate) instructions: Vec<Instruction>,
    pub(crate) start: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum Instruction {
    Consume {
        matcher: CharacterMatcher,
        next: usize,
    },
    Split {
        first: usize,
        second: usize,
    },
    Jump {
        next: usize,
    },
    AssertStart {
        next: usize,
    },
    AssertEnd {
        next: usize,
    },
    Match,
}

#[derive(Clone, Debug)]
pub(crate) enum CharacterMatcher {
    Literal(char),
    Class(Arc<CharacterClass>),
    Dot,
}

impl CharacterMatcher {
    pub(crate) fn matches(&self, value: char) -> bool {
        match self {
            Self::Literal(expected) => *expected == value,
            Self::Class(class) => class.contains(value),
            Self::Dot => !matches!(value, '\n' | '\r' | '\u{2028}' | '\u{2029}'),
        }
    }
}

#[derive(Clone, Copy)]
enum Patch {
    Next(usize),
    Second(usize),
}

struct Fragment {
    start: usize,
    outgoing: Vec<Patch>,
}

pub(crate) fn compile(expression: &Expression) -> Result<Program, CompileError> {
    let expected_instructions = instruction_count(expression)?;
    let mut compiler = Compiler {
        instructions: Vec::with_capacity(expected_instructions),
    };
    let fragment = compiler.compile_expression(expression)?;
    let matched = compiler.add(Instruction::Match)?;
    compiler.patch(&fragment.outgoing, matched)?;
    debug_assert_eq!(compiler.instructions.len(), expected_instructions);
    Ok(Program {
        instructions: compiler.instructions,
        start: fragment.start,
    })
}

/// Counts the exact compiled instruction length without expanding repeated
/// syntax into an NFA.
pub(crate) fn instruction_count(expression: &Expression) -> Result<usize, CompileError> {
    let instructions = count_expression(expression)?;
    checked_add(instructions, 1)
}

fn count_expression(expression: &Expression) -> Result<usize, CompileError> {
    match expression {
        Expression::Empty
        | Expression::Literal(_)
        | Expression::Class(_)
        | Expression::Dot
        | Expression::Start
        | Expression::End => Ok(1),
        Expression::Group(nested) => count_expression(nested),
        Expression::Concatenation(expressions) => {
            if expressions.is_empty() {
                return Ok(1);
            }
            expressions.iter().try_fold(0, |total, expression| {
                checked_add(total, count_expression(expression)?)
            })
        }
        Expression::Alternation(expressions) => {
            if expressions.is_empty() {
                return Ok(1);
            }
            let branches = expressions.iter().try_fold(0, |total, expression| {
                checked_add(total, count_expression(expression)?)
            })?;
            checked_add(branches, expressions.len().saturating_sub(1))
        }
        Expression::Repeat {
            expression,
            minimum,
            maximum,
        } => {
            let body = count_expression(expression)?;
            let minimum = usize::try_from(*minimum).map_err(|_| instruction_limit_error())?;
            let required = checked_mul(body, minimum)?;
            let repeated = match maximum {
                Some(maximum) => {
                    let maximum =
                        usize::try_from(*maximum).map_err(|_| instruction_limit_error())?;
                    let optional_count = maximum
                        .checked_sub(minimum)
                        .ok_or_else(instruction_limit_error)?;
                    checked_mul(checked_add(body, 1)?, optional_count)?
                }
                None => checked_add(body, 1)?,
            };
            let total = checked_add(required, repeated)?;
            if minimum == 0 && matches!(maximum, Some(0)) {
                Ok(1)
            } else {
                Ok(total)
            }
        }
    }
}

fn checked_add(left: usize, right: usize) -> Result<usize, CompileError> {
    let value = left
        .checked_add(right)
        .ok_or_else(instruction_limit_error)?;
    (value <= MAX_INSTRUCTIONS)
        .then_some(value)
        .ok_or_else(instruction_limit_error)
}

fn checked_mul(left: usize, right: usize) -> Result<usize, CompileError> {
    let value = left
        .checked_mul(right)
        .ok_or_else(instruction_limit_error)?;
    (value <= MAX_INSTRUCTIONS)
        .then_some(value)
        .ok_or_else(instruction_limit_error)
}

fn instruction_limit_error() -> CompileError {
    CompileError::new(CompileErrorKind::InstructionLimitExceeded, 0)
}

struct Compiler {
    instructions: Vec<Instruction>,
}

impl Compiler {
    fn compile_expression(&mut self, expression: &Expression) -> Result<Fragment, CompileError> {
        match expression {
            Expression::Empty => {
                let index = self.add(Instruction::Jump { next: UNSET })?;
                Ok(Fragment {
                    start: index,
                    outgoing: vec![Patch::Next(index)],
                })
            }
            Expression::Literal(value) => self.consume(CharacterMatcher::Literal(*value)),
            Expression::Class(class) => self.consume(CharacterMatcher::Class(Arc::clone(class))),
            Expression::Dot => self.consume(CharacterMatcher::Dot),
            Expression::Start => self.assertion(true),
            Expression::End => self.assertion(false),
            Expression::Group(nested) => self.compile_expression(nested),
            Expression::Concatenation(expressions) => self.compile_concatenation(expressions),
            Expression::Alternation(expressions) => self.compile_alternation(expressions),
            Expression::Repeat {
                expression,
                minimum,
                maximum,
            } => self.compile_repetition(expression, *minimum, *maximum),
        }
    }

    fn consume(&mut self, matcher: CharacterMatcher) -> Result<Fragment, CompileError> {
        let index = self.add(Instruction::Consume {
            matcher,
            next: UNSET,
        })?;
        Ok(Fragment {
            start: index,
            outgoing: vec![Patch::Next(index)],
        })
    }

    fn assertion(&mut self, start: bool) -> Result<Fragment, CompileError> {
        let instruction = if start {
            Instruction::AssertStart { next: UNSET }
        } else {
            Instruction::AssertEnd { next: UNSET }
        };
        let index = self.add(instruction)?;
        Ok(Fragment {
            start: index,
            outgoing: vec![Patch::Next(index)],
        })
    }

    fn compile_concatenation(
        &mut self,
        expressions: &[Expression],
    ) -> Result<Fragment, CompileError> {
        let mut result: Option<Fragment> = None;
        for expression in expressions {
            let next = self.compile_expression(expression)?;
            result = Some(match result {
                Some(current) => self.concatenate(current, next)?,
                None => next,
            });
        }
        match result {
            Some(fragment) => Ok(fragment),
            None => self.compile_expression(&Expression::Empty),
        }
    }

    fn compile_alternation(
        &mut self,
        expressions: &[Expression],
    ) -> Result<Fragment, CompileError> {
        let mut result: Option<Fragment> = None;
        for expression in expressions {
            let branch = self.compile_expression(expression)?;
            result = Some(match result {
                Some(current) => {
                    let split = self.add(Instruction::Split {
                        first: current.start,
                        second: branch.start,
                    })?;
                    let mut outgoing = current.outgoing;
                    outgoing.extend(branch.outgoing);
                    Fragment {
                        start: split,
                        outgoing,
                    }
                }
                None => branch,
            });
        }
        match result {
            Some(fragment) => Ok(fragment),
            None => self.compile_expression(&Expression::Empty),
        }
    }

    fn compile_repetition(
        &mut self,
        expression: &Expression,
        minimum: u32,
        maximum: Option<u32>,
    ) -> Result<Fragment, CompileError> {
        let mut result: Option<Fragment> = None;
        for _ in 0..minimum {
            let required = self.compile_expression(expression)?;
            result = Some(match result {
                Some(current) => self.concatenate(current, required)?,
                None => required,
            });
        }

        match maximum {
            Some(maximum) => {
                for _ in minimum..maximum {
                    let body = self.compile_expression(expression)?;
                    let split = self.add(Instruction::Split {
                        first: body.start,
                        second: UNSET,
                    })?;
                    let mut outgoing = body.outgoing;
                    outgoing.push(Patch::Second(split));
                    let optional = Fragment {
                        start: split,
                        outgoing,
                    };
                    result = Some(match result {
                        Some(current) => self.concatenate(current, optional)?,
                        None => optional,
                    });
                }
            }
            None => {
                let body = self.compile_expression(expression)?;
                let split = self.add(Instruction::Split {
                    first: body.start,
                    second: UNSET,
                })?;
                self.patch(&body.outgoing, split)?;
                let repeated = Fragment {
                    start: split,
                    outgoing: vec![Patch::Second(split)],
                };
                result = Some(match result {
                    Some(current) => self.concatenate(current, repeated)?,
                    None => repeated,
                });
            }
        }

        match result {
            Some(fragment) => Ok(fragment),
            None => self.compile_expression(&Expression::Empty),
        }
    }

    fn concatenate(&mut self, first: Fragment, second: Fragment) -> Result<Fragment, CompileError> {
        self.patch(&first.outgoing, second.start)?;
        Ok(Fragment {
            start: first.start,
            outgoing: second.outgoing,
        })
    }

    fn add(&mut self, instruction: Instruction) -> Result<usize, CompileError> {
        if self.instructions.len() >= MAX_INSTRUCTIONS {
            return Err(CompileError::new(
                CompileErrorKind::InstructionLimitExceeded,
                0,
            ));
        }
        let index = self.instructions.len();
        self.instructions.push(instruction);
        Ok(index)
    }

    fn patch(&mut self, outgoing: &[Patch], target: usize) -> Result<(), CompileError> {
        for patch in outgoing {
            let instruction = match patch {
                Patch::Next(index) | Patch::Second(index) => self
                    .instructions
                    .get_mut(*index)
                    .ok_or_else(|| CompileError::new(CompileErrorKind::UnexpectedToken, 0))?,
            };
            match (patch, instruction) {
                (Patch::Next(_), Instruction::Consume { next, .. })
                | (Patch::Next(_), Instruction::Jump { next })
                | (Patch::Next(_), Instruction::AssertStart { next })
                | (Patch::Next(_), Instruction::AssertEnd { next }) => *next = target,
                (Patch::Second(_), Instruction::Split { second, .. }) => *second = target,
                _ => {
                    return Err(CompileError::new(CompileErrorKind::UnexpectedToken, 0));
                }
            }
        }
        Ok(())
    }
}
