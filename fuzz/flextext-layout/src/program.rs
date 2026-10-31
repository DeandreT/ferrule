//! A compact byte program that describes one FlexText layout and its input.
//!
//! Every byte string decodes to some program: reads past the end yield zero,
//! and every selector is reduced modulo its range. Commands are built through
//! the public `mapping` constructors, so constructor rejections (empty
//! strings, duplicate trim characters, invalid dialects or regular
//! expressions) are outcomes too. The bytes left after the command tree are
//! the input text.
//!
//! [`Encoder`] writes the same format and is used to author the seeds.

use std::num::NonZeroU32;

use ir::ScalarType;
use mapping::{
    DelimitedDialect, DelimitedRecordField, FixedWidthRecordField, FlexCommand, FlexLineEnding,
    FlexTextLayoutError, MAX_FLEXTEXT_LAYOUT_DEPTH, MAX_FLEXTEXT_LAYOUT_STRING_BYTES,
    MAX_FLEXTEXT_REGEX_PATTERN_BYTES, ManySplitter, OnceSplitter, StoreTrim, SwitchArm, TrimSide,
};

/// Opcodes selecting the next command.
pub mod op {
    pub const STORE: u8 = 0;
    pub const IGNORE: u8 = 1;
    pub const SPLIT_ONCE: u8 = 2;
    pub const SPLIT_MANY: u8 = 3;
    pub const FIXED_WIDTH: u8 = 4;
    pub const DELIMITED: u8 = 5;
    pub const SWITCH: u8 = 6;
    /// `count` nested single-child wrappers around one inner command.
    pub const CHAIN: u8 = 7;
    /// One switch with up to [`super::MAX_WIDE_ARMS`] generated arms.
    pub const WIDE_SWITCH: u8 = 8;
    pub const COUNT: u8 = 9;
}

/// Wrapper kinds used by [`op::CHAIN`].
pub mod chain {
    /// `SplitOnce` on `|` with the inner command first and `Ignore` second.
    pub const SPLIT_ONCE: u8 = 0;
    /// `SplitMany` on line feeds.
    pub const SPLIT_MANY: u8 = 1;
    /// A first-match `Switch` whose only branch is its default.
    pub const SWITCH_DEFAULT: u8 = 2;
    /// An all-possible `Switch` whose only branch is one arm on prefix `c`.
    pub const SWITCH_ARM: u8 = 3;
    pub const COUNT: u8 = 4;
}

/// Arm kinds used by [`op::WIDE_SWITCH`].
pub mod wide {
    /// Literal prefixes `p<i>;`, each arm storing `s<i>`.
    pub const LITERAL: u8 = 0;
    /// Distinct regular expressions `^r<i>;`, each arm storing `s<i>`. The
    /// count is capped just past the distinct-regex limit.
    pub const REGEX: u8 = 1;
    pub const COUNT: u8 = 2;
}

/// Commands a program may declare, excluding generated wide-switch arms.
pub const MAX_PROGRAM_COMMANDS: usize = 1_024;
/// Longest chain; enough to pass the layout depth limit.
pub const MAX_CHAIN: usize = MAX_FLEXTEXT_LAYOUT_DEPTH + 6;
/// Most generated wide-switch arms per program; enough to pass the layout
/// node limit.
pub const MAX_WIDE_ARMS: usize = 4_200;
/// Most generated regular-expression arms per program; enough to pass the
/// distinct-regex limit.
pub const MAX_WIDE_REGEX_ARMS: usize = crate::MAX_SWITCH_REGEXES + 2;
/// Decoded string operands per program may total this many bytes: just past
/// the layout string limit, so one operand at or over a size limit fits. An
/// at- or over-limit selector that would pass it decodes as the pool's first
/// entry instead.
pub const MAX_PROGRAM_STRING_BYTES: usize = MAX_FLEXTEXT_LAYOUT_STRING_BYTES + 256;
/// Decoder recursion is cut off here; deeper commands become a `Store`.
pub const MAX_DECODE_DEPTH: usize = MAX_FLEXTEXT_LAYOUT_DEPTH + 2;
/// Largest raw string or pattern read from the program.
const MAX_RAW_BYTES: usize = 63;

pub const NAME_POOL: &[&str] = &[
    "a",
    "b",
    "c",
    "d",
    "é",
    "名前",
    "two words",
    "line\nbreak",
    "",
];

pub const TEXT_POOL: &[&str] = &[
    ",", ";", "|", "||", "\t", " ", "\n", "\r\n", "\r", "\"", "é", "日本", "\u{feff}", "😀", "ab",
    "AB", "#", "", "\0",
];

pub const REGEX_POOL: &[&str] = &[
    "^H",
    "a|b",
    r"\d+",
    "(?i)é",
    "^$",
    "(?m)^X$",
    "[[:alpha:]]+",
    r"\p{Greek}",
    r"\bword\b",
    ".",
    r"\x{FEFF}",
    "(",
    "[",
    "a{1000}{1000}",
    "",
];

pub const CHAR_POOL: &[char] = &[
    '"', '\'', '\\', ' ', '0', '_', ',', '|', 'é', '日', '😀', '\t', '\n', '\r', '\0',
];

pub const COUNT_POOL: &[u32] = &[1, 2, 3, 4, 8, 64, u32::MAX];

pub const TYPES: [ScalarType; 4] = [
    ScalarType::String,
    ScalarType::Int,
    ScalarType::Float,
    ScalarType::Bool,
];

/// String selector: past the pool come a raw string, a string exactly at a
/// size limit, and a string one byte over it.
const RAW: usize = 0;
const AT_LIMIT: usize = 1;
const OVER_LIMIT: usize = 2;
const STRING_EXTRAS: usize = 3;

/// One decoded program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub root_name: String,
    pub line_ending: FlexLineEnding,
    pub write_bom: bool,
    /// The command tree, or the first constructor rejection met while
    /// building it.
    pub command: Result<FlexCommand, FlexTextLayoutError>,
    /// Remaining bytes as text (invalid UTF-8 replaced by U+FFFD).
    pub text: String,
}

impl Program {
    pub fn decode(bytes: &[u8]) -> Self {
        let mut decoder = Decoder {
            bytes,
            pos: 0,
            commands: 0,
            string_bytes: 0,
            wide_arms: 0,
            wide_regex_arms: 0,
            error: None,
        };
        let header = decoder.byte();
        let line_ending = if header & 1 == 0 {
            FlexLineEnding::Lf
        } else {
            FlexLineEnding::Crlf
        };
        let write_bom = header & 2 != 0;
        let root_name = decoder.name();
        let command = decoder.command(1);
        let text = String::from_utf8_lossy(&bytes[decoder.pos..]).into_owned();
        Self {
            root_name,
            line_ending,
            write_bom,
            command: match decoder.error {
                Some(error) => Err(error),
                None => Ok(command),
            },
            text,
        }
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    commands: usize,
    string_bytes: usize,
    wide_arms: usize,
    wide_regex_arms: usize,
    error: Option<FlexTextLayoutError>,
}

impl Decoder<'_> {
    fn byte(&mut self) -> u8 {
        let byte = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos = (self.pos + 1).min(self.bytes.len());
        byte
    }

    fn select(&mut self, range: usize) -> usize {
        usize::from(self.byte()) % range
    }

    fn u16(&mut self) -> usize {
        usize::from(u16::from_le_bytes([self.byte(), self.byte()]))
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }

    fn raw(&mut self) -> String {
        let length = self.select(MAX_RAW_BYTES + 1);
        let start = self.pos;
        let end = (start + length).min(self.bytes.len());
        self.pos = end;
        String::from_utf8_lossy(&self.bytes[start..end]).into_owned()
    }

    fn pooled(&mut self, pool: &[&str], limit: usize) -> String {
        let index = self.select(pool.len() + STRING_EXTRAS);
        let value = match index.checked_sub(pool.len()) {
            None => pool[index].to_string(),
            Some(RAW) => self.raw(),
            Some(extra) => {
                let length = if extra == AT_LIMIT { limit } else { limit + 1 };
                if self.string_bytes + length <= MAX_PROGRAM_STRING_BYTES {
                    "x".repeat(length)
                } else {
                    pool[0].to_string()
                }
            }
        };
        self.string_bytes += value.len();
        value
    }

    fn name(&mut self) -> String {
        self.pooled(NAME_POOL, MAX_FLEXTEXT_LAYOUT_STRING_BYTES)
    }

    fn text(&mut self) -> String {
        self.pooled(TEXT_POOL, MAX_FLEXTEXT_LAYOUT_STRING_BYTES)
    }

    fn pattern(&mut self) -> String {
        self.pooled(REGEX_POOL, MAX_FLEXTEXT_REGEX_PATTERN_BYTES)
    }

    fn character(&mut self) -> char {
        let index = self.select(CHAR_POOL.len() + 1);
        CHAR_POOL
            .get(index)
            .copied()
            .unwrap_or_else(|| char::from_u32(self.u32()).unwrap_or(char::REPLACEMENT_CHARACTER))
    }

    fn count(&mut self) -> NonZeroU32 {
        let index = self.select(COUNT_POOL.len() + 1);
        let value = COUNT_POOL.get(index).copied().unwrap_or_else(|| self.u32());
        NonZeroU32::new(value).unwrap_or(NonZeroU32::MIN)
    }

    fn ty(&mut self) -> ScalarType {
        TYPES[self.select(TYPES.len())]
    }

    /// Keeps the first constructor rejection and substitutes a placeholder so
    /// decoding stays total.
    fn built<T>(&mut self, result: Result<T, FlexTextLayoutError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.error.get_or_insert(error);
                None
            }
        }
    }

    fn leaf() -> FlexCommand {
        FlexCommand::store("a", ScalarType::String, None)
    }

    fn command(&mut self, depth: usize) -> FlexCommand {
        if depth > MAX_DECODE_DEPTH || self.commands >= MAX_PROGRAM_COMMANDS {
            return Self::leaf();
        }
        self.commands += 1;
        match self.byte() % op::COUNT {
            op::STORE => {
                let name = self.name();
                let ty = self.ty();
                let trim = match self.select(4) {
                    0 => None,
                    side => {
                        let side = [TrimSide::Left, TrimSide::Right, TrimSide::Both][side - 1];
                        let characters = self.text();
                        self.built(StoreTrim::new(side, characters))
                    }
                };
                FlexCommand::store(name, ty, trim)
            }
            op::IGNORE => FlexCommand::Ignore,
            op::SPLIT_ONCE => {
                let name = self.name();
                let splitter = match self.select(5) {
                    0 => OnceSplitter::FixedLines(self.count()),
                    1 => OnceSplitter::FixedColumns(self.count()),
                    2 => OnceSplitter::Delimiter(self.text()),
                    3 => OnceSplitter::LineStartingWith(self.text()),
                    _ => OnceSplitter::LineContaining(self.text()),
                };
                let first = Box::new(self.command(depth + 1));
                let second = Box::new(self.command(depth + 1));
                FlexCommand::SplitOnce {
                    name,
                    splitter,
                    first,
                    second,
                }
            }
            op::SPLIT_MANY => {
                let name = self.name();
                let splitter = match self.select(3) {
                    0 => ManySplitter::FixedLines(self.count()),
                    1 => ManySplitter::Delimiter(self.text()),
                    _ => ManySplitter::LinesStartingWith(self.text()),
                };
                let child = Box::new(self.command(depth + 1));
                FlexCommand::SplitMany {
                    name,
                    splitter,
                    child,
                }
            }
            op::FIXED_WIDTH => {
                let name = self.name();
                let count = self.select(5);
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count {
                    let field_name = self.name();
                    let ty = self.ty();
                    let width = self.count();
                    if let Some(field) =
                        self.built(FixedWidthRecordField::new(field_name, ty, width))
                    {
                        fields.push(field);
                    }
                }
                let fill_char = self.character();
                let flags = self.byte();
                FlexCommand::FixedWidthRecords {
                    name,
                    fields,
                    fill_char,
                    record_delimiters: flags & 1 != 0,
                    treat_empty_as_absent: flags & 2 != 0,
                }
            }
            op::DELIMITED => {
                let name = self.name();
                let field_separator = self.text();
                let record_separator = self.text();
                let quote = self.character();
                let escape = self.character();
                let dialect = self.built(DelimitedDialect::new_with_field_separator(
                    field_separator,
                    record_separator,
                    quote,
                    escape,
                ));
                let count = self.select(5);
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count {
                    let field_name = self.name();
                    let ty = self.ty();
                    if let Some(field) = self.built(DelimitedRecordField::new(field_name, ty)) {
                        fields.push(field);
                    }
                }
                match dialect {
                    Some(dialect) => FlexCommand::DelimitedRecords {
                        name,
                        dialect,
                        fields,
                    },
                    None => FlexCommand::Ignore,
                }
            }
            op::SWITCH => {
                let name = self.name();
                let flags = self.byte();
                let count = self.select(5);
                let mut arms = Vec::with_capacity(count);
                for _ in 0..count {
                    let regex = self.byte() & 1 != 0;
                    let condition = if regex { self.pattern() } else { self.text() };
                    let command = self.command(depth + 1);
                    let arm = if regex {
                        SwitchArm::new_contains_regex(condition, command)
                    } else {
                        SwitchArm::new(condition, command)
                    };
                    if let Some(arm) = self.built(arm) {
                        arms.push(arm);
                    }
                }
                let default = (flags & 2 != 0).then(|| Box::new(self.command(depth + 1)));
                FlexCommand::Switch {
                    name,
                    mode: switch_mode(flags),
                    arms,
                    default,
                }
            }
            op::CHAIN => {
                let count = self.select(MAX_CHAIN + 1);
                let kind = self.byte() % chain::COUNT;
                let inner_depth = depth + count;
                let mut command = if inner_depth > MAX_DECODE_DEPTH {
                    Self::leaf()
                } else {
                    self.command(inner_depth)
                };
                for level in (0..count).rev() {
                    command = wrap(kind, format!("c{level}"), command);
                }
                command
            }
            _ => {
                let name = self.name();
                let flags = self.byte();
                let mut count =
                    (self.u16() % (MAX_WIDE_ARMS + 1)).min(MAX_WIDE_ARMS - self.wide_arms);
                let kind = self.byte() % wide::COUNT;
                if kind == wide::REGEX {
                    count = count.min(MAX_WIDE_REGEX_ARMS - self.wide_regex_arms);
                    self.wide_regex_arms += count;
                }
                self.wide_arms += count;
                let mut arms = Vec::with_capacity(count);
                for index in 0..count {
                    let command = FlexCommand::store(format!("s{index}"), ScalarType::String, None);
                    let arm = if kind == wide::REGEX {
                        SwitchArm::new_contains_regex(format!("^r{index};"), command)
                    } else {
                        SwitchArm::new(format!("p{index};"), command)
                    };
                    if let Some(arm) = self.built(arm) {
                        arms.push(arm);
                    }
                }
                FlexCommand::Switch {
                    name,
                    mode: switch_mode(flags),
                    arms,
                    default: None,
                }
            }
        }
    }
}

fn switch_mode(flags: u8) -> mapping::SwitchMode {
    if flags & 1 == 0 {
        mapping::SwitchMode::AllPossible
    } else {
        mapping::SwitchMode::FirstMatch
    }
}

fn wrap(kind: u8, name: String, inner: FlexCommand) -> FlexCommand {
    match kind {
        chain::SPLIT_ONCE => FlexCommand::SplitOnce {
            name,
            splitter: OnceSplitter::Delimiter("|".into()),
            first: Box::new(inner),
            second: Box::new(FlexCommand::Ignore),
        },
        chain::SPLIT_MANY => FlexCommand::SplitMany {
            name,
            splitter: ManySplitter::Delimiter("\n".into()),
            child: Box::new(inner),
        },
        chain::SWITCH_DEFAULT => FlexCommand::Switch {
            name,
            mode: mapping::SwitchMode::FirstMatch,
            arms: Vec::new(),
            default: Some(Box::new(inner)),
        },
        _ => match SwitchArm::new("c", inner) {
            Ok(arm) => FlexCommand::Switch {
                name,
                mode: mapping::SwitchMode::AllPossible,
                arms: vec![arm],
                default: None,
            },
            Err(error) => panic!("constant switch prefix must be valid: {error}"),
        },
    }
}

/// Writes programs in the format [`Program::decode`] reads.
#[derive(Debug, Default)]
pub struct Encoder {
    bytes: Vec<u8>,
}

/// A string operand: a literal (pooled when possible, raw otherwise) or a
/// string at or one byte over the operand's size limit.
#[derive(Debug, Clone, Copy)]
pub enum Str<'a> {
    Lit(&'a str),
    AtLimit,
    OverLimit,
}

impl Encoder {
    /// Starts a program with its header and root name.
    pub fn new(crlf: bool, bom: bool, root_name: &str) -> Self {
        let mut encoder = Self::default();
        encoder.byte(u8::from(crlf) | (u8::from(bom) << 1));
        encoder.name(root_name);
        encoder
    }

    pub fn byte(&mut self, byte: u8) -> &mut Self {
        self.bytes.push(byte);
        self
    }

    pub fn op(&mut self, op: u8) -> &mut Self {
        self.byte(op)
    }

    pub fn u16(&mut self, value: u16) -> &mut Self {
        self.bytes.extend(value.to_le_bytes());
        self
    }

    fn pooled(&mut self, pool: &[&str], value: Str<'_>) -> &mut Self {
        match value {
            Str::Lit(text) => match pool.iter().position(|entry| *entry == text) {
                Some(index) => self.byte(selector(index)),
                None => {
                    assert!(
                        text.len() <= MAX_RAW_BYTES,
                        "raw operand too long: {text:?}"
                    );
                    self.byte(selector(pool.len() + RAW));
                    self.byte(selector(text.len()));
                    self.bytes.extend(text.as_bytes());
                    self
                }
            },
            Str::AtLimit => self.byte(selector(pool.len() + AT_LIMIT)),
            Str::OverLimit => self.byte(selector(pool.len() + OVER_LIMIT)),
        }
    }

    pub fn name(&mut self, name: &str) -> &mut Self {
        self.pooled(NAME_POOL, Str::Lit(name))
    }

    pub fn name_str(&mut self, name: Str<'_>) -> &mut Self {
        self.pooled(NAME_POOL, name)
    }

    pub fn text(&mut self, text: &str) -> &mut Self {
        self.pooled(TEXT_POOL, Str::Lit(text))
    }

    pub fn text_str(&mut self, text: Str<'_>) -> &mut Self {
        self.pooled(TEXT_POOL, text)
    }

    pub fn pattern(&mut self, pattern: &str) -> &mut Self {
        self.pooled(REGEX_POOL, Str::Lit(pattern))
    }

    pub fn pattern_str(&mut self, pattern: Str<'_>) -> &mut Self {
        self.pooled(REGEX_POOL, pattern)
    }

    pub fn character(&mut self, character: char) -> &mut Self {
        match CHAR_POOL.iter().position(|entry| *entry == character) {
            Some(index) => self.byte(selector(index)),
            None => {
                self.byte(selector(CHAR_POOL.len()));
                self.bytes.extend(u32::from(character).to_le_bytes());
                self
            }
        }
    }

    pub fn count(&mut self, count: u32) -> &mut Self {
        match COUNT_POOL.iter().position(|entry| *entry == count) {
            Some(index) => self.byte(selector(index)),
            None => {
                self.byte(selector(COUNT_POOL.len()));
                self.bytes.extend(count.to_le_bytes());
                self
            }
        }
    }

    pub fn ty(&mut self, ty: ScalarType) -> &mut Self {
        let index = TYPES.iter().position(|entry| *entry == ty).unwrap_or(0);
        self.byte(selector(index))
    }

    /// Appends the input text and returns the finished program bytes.
    pub fn finish(&mut self, text: &str) -> Vec<u8> {
        let mut bytes = std::mem::take(&mut self.bytes);
        bytes.extend(text.as_bytes());
        bytes
    }
}

fn selector(index: usize) -> u8 {
    match u8::try_from(index) {
        Ok(byte) => byte,
        Err(_) => panic!("selector {index} does not fit in one byte"),
    }
}
