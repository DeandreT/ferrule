//! Bounded, portable matching for Ferrule's supported JSON Schema `pattern`
//! language.

mod compiler;
mod matcher;
mod parser;

use std::fmt;

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_PARSE_DEPTH: usize = 256;
pub const MAX_AST_NODES: usize = 8_192;
pub const MAX_INSTRUCTIONS: usize = 16_384;
pub const DEFAULT_MATCH_WORK_LIMIT: u64 = 100_000_000;

/// A validated portable JSON Schema pattern compiled to a bounded Thompson NFA.
#[derive(Clone, Debug)]
pub struct PortableJsonPattern {
    source: String,
    program: compiler::Program,
}

impl PortableJsonPattern {
    /// Validates portable syntax and returns its exact compiled instruction
    /// count without materializing the NFA.
    pub fn validate(source: &str) -> Result<PatternValidation, CompileError> {
        let expression = parse_source(source)?;
        let instruction_count = compiler::instruction_count(&expression)?;
        Ok(PatternValidation { instruction_count })
    }

    pub fn compile(source: &str) -> Result<Self, CompileError> {
        let expression = parse_source(source)?;
        let program = compiler::compile(&expression)?;
        Ok(Self {
            source: source.to_owned(),
            program,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn instruction_count(&self) -> usize {
        self.program.instructions.len()
    }

    /// Returns the deterministic work charge for matching `value`.
    ///
    /// One work unit is charged for every compiled instruction for each input
    /// Unicode scalar, with an empty input charged as one scalar.
    pub fn work_estimate(&self, value: &str) -> u64 {
        let scalar_count = value.chars().count().max(1);
        let scalar_count = u64::try_from(scalar_count).unwrap_or(u64::MAX);
        let instruction_count = u64::try_from(self.instruction_count()).unwrap_or(u64::MAX);
        scalar_count.saturating_mul(instruction_count)
    }

    pub fn is_match(&self, value: &str) -> Result<bool, MatchError> {
        let mut remaining_work = DEFAULT_MATCH_WORK_LIMIT;
        self.is_match_with_budget(value, &mut remaining_work)
    }

    pub fn is_match_with_budget(
        &self,
        value: &str,
        remaining_work: &mut u64,
    ) -> Result<bool, MatchError> {
        let required = self.work_estimate(value);
        if required > *remaining_work {
            return Err(MatchError::WorkLimitExceeded {
                required,
                limit: *remaining_work,
            });
        }
        *remaining_work -= required;
        Ok(matcher::is_match(&self.program, value))
    }
}

fn parse_source(source: &str) -> Result<parser::Expression, CompileError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(CompileError::new(
            CompileErrorKind::SourceTooLong,
            MAX_SOURCE_BYTES,
        ));
    }
    parser::parse(source)
}

/// Exact bounded metadata produced by count-only pattern validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatternValidation {
    instruction_count: usize,
}

impl PatternValidation {
    pub const fn instruction_count(self) -> usize {
        self.instruction_count
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileError {
    kind: CompileErrorKind,
    byte_offset: usize,
}

impl CompileError {
    pub(crate) const fn new(kind: CompileErrorKind, byte_offset: usize) -> Self {
        Self { kind, byte_offset }
    }

    pub const fn kind(&self) -> CompileErrorKind {
        self.kind
    }

    pub const fn byte_offset(&self) -> usize {
        self.byte_offset
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "portable JSON pattern error at byte {}: {}",
            self.byte_offset, self.kind
        )
    }
}

impl std::error::Error for CompileError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileErrorKind {
    SourceTooLong,
    ParseDepthExceeded,
    AstNodeLimitExceeded,
    InstructionLimitExceeded,
    UnexpectedEnd,
    UnexpectedToken,
    UnsupportedConstruct,
    InvalidEscape,
    InvalidUnicodeEscape,
    InvalidCharacterClass,
    InvalidRange,
    InvalidQuantifier,
    QuantifiedAssertion,
}

impl fmt::Display for CompileErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SourceTooLong => "source exceeds 64 KiB",
            Self::ParseDepthExceeded => "parse nesting exceeds 256 groups",
            Self::AstNodeLimitExceeded => "syntax tree exceeds 8192 nodes",
            Self::InstructionLimitExceeded => "compiled program exceeds 16384 instructions",
            Self::UnexpectedEnd => "unexpected end of pattern",
            Self::UnexpectedToken => "unexpected syntax token",
            Self::UnsupportedConstruct => "construct is outside the portable pattern language",
            Self::InvalidEscape => "invalid or unsupported escape",
            Self::InvalidUnicodeEscape => "invalid Unicode escape",
            Self::InvalidCharacterClass => "invalid character class",
            Self::InvalidRange => "invalid character class range",
            Self::InvalidQuantifier => "invalid quantifier",
            Self::QuantifiedAssertion => "zero-width assertions cannot be quantified",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchError {
    WorkLimitExceeded { required: u64, limit: u64 },
}

impl fmt::Display for MatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkLimitExceeded { required, limit } => {
                write!(
                    formatter,
                    "pattern match requires {required} work units, limit is {limit}"
                )
            }
        }
    }
}

impl std::error::Error for MatchError {}
