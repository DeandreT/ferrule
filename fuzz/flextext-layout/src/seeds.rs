//! Deterministic, self-authored seed inputs.
//!
//! `cargo run --example write_seeds` writes these to `seeds/`, and the seed
//! test fails if the checked-in files drift from this generator.

use crate::program::{
    Encoder, MAX_DECODE_DEPTH, MAX_PROGRAM_COMMANDS, MAX_WIDE_REGEX_ARMS, Str, chain, op, wide,
};
use crate::{JSON_TEXT_SEPARATOR, MAX_SWITCH_REGEXES};
use ir::ScalarType::{self, Bool, Float, Int, String as Text};

/// A command tree in the program format.
enum Cmd<'a> {
    Store(&'a str, ScalarType, Option<(Side, &'a str)>),
    Ignore,
    SplitOnce(&'a str, Once<'a>, Box<Cmd<'a>>, Box<Cmd<'a>>),
    SplitMany(&'a str, Many<'a>, Box<Cmd<'a>>),
    Fixed {
        name: &'a str,
        fields: Vec<(&'a str, ScalarType, u32)>,
        fill: char,
        delimited: bool,
        empty_absent: bool,
    },
    Delimited {
        name: &'a str,
        field_separator: Str<'a>,
        record_separator: &'a str,
        quote: char,
        escape: char,
        fields: Vec<(&'a str, ScalarType)>,
    },
    Switch {
        name: &'a str,
        first_match: bool,
        arms: Vec<(Cond<'a>, Cmd<'a>)>,
        default: Option<Box<Cmd<'a>>>,
    },
    Chain(usize, u8, Box<Cmd<'a>>),
    Wide {
        name: &'a str,
        count: u16,
        kind: u8,
    },
    /// A command whose name operand is at or over the string limit.
    LongName(Str<'a>),
}

#[derive(Clone, Copy)]
enum Side {
    Left = 1,
    Right = 2,
    Both = 3,
}

enum Once<'a> {
    Lines(u32),
    Columns(u32),
    Delimiter(Str<'a>),
    LineStarting(&'a str),
    LineContaining(&'a str),
}

enum Many<'a> {
    Lines(u32),
    Delimiter(Str<'a>),
    LinesStarting(&'a str),
}

enum Cond<'a> {
    Prefix(&'a str),
    Regex(Str<'a>),
}

fn store(name: &str, ty: ScalarType) -> Cmd<'_> {
    Cmd::Store(name, ty, None)
}

fn split_once<'a>(name: &'a str, splitter: Once<'a>, first: Cmd<'a>, second: Cmd<'a>) -> Cmd<'a> {
    Cmd::SplitOnce(name, splitter, Box::new(first), Box::new(second))
}

fn split_many<'a>(name: &'a str, splitter: Many<'a>, child: Cmd<'a>) -> Cmd<'a> {
    Cmd::SplitMany(name, splitter, Box::new(child))
}

fn switch<'a>(
    name: &'a str,
    first_match: bool,
    arms: Vec<(Cond<'a>, Cmd<'a>)>,
    default: Option<Cmd<'a>>,
) -> Cmd<'a> {
    Cmd::Switch {
        name,
        first_match,
        arms,
        default: default.map(Box::new),
    }
}

fn fixed<'a>(
    name: &'a str,
    fields: Vec<(&'a str, ScalarType, u32)>,
    delimited: bool,
    empty_absent: bool,
) -> Cmd<'a> {
    Cmd::Fixed {
        name,
        fields,
        fill: ' ',
        delimited,
        empty_absent,
    }
}

fn delimited<'a>(
    name: &'a str,
    field_separator: &'a str,
    record_separator: &'a str,
    quote: char,
    escape: char,
    fields: Vec<(&'a str, ScalarType)>,
) -> Cmd<'a> {
    Cmd::Delimited {
        name,
        field_separator: Str::Lit(field_separator),
        record_separator,
        quote,
        escape,
        fields,
    }
}

struct Writer {
    encoder: Encoder,
    commands: usize,
}

impl Writer {
    fn command(&mut self, command: &Cmd<'_>, depth: usize) {
        assert!(
            depth <= MAX_DECODE_DEPTH && self.commands < MAX_PROGRAM_COMMANDS,
            "seed exceeds the program decoder's bounds"
        );
        self.commands += 1;
        let e = &mut self.encoder;
        match command {
            Cmd::Store(name, ty, trim) => {
                e.op(op::STORE).name(name).ty(*ty);
                match trim {
                    None => e.byte(0),
                    Some((side, characters)) => e.byte(*side as u8).text(characters),
                };
            }
            Cmd::Ignore => {
                e.op(op::IGNORE);
            }
            Cmd::LongName(name) => {
                e.op(op::STORE).name_str(*name).ty(Text).byte(0);
            }
            Cmd::SplitOnce(name, splitter, first, second) => {
                e.op(op::SPLIT_ONCE).name(name);
                match splitter {
                    Once::Lines(count) => e.byte(0).count(*count),
                    Once::Columns(count) => e.byte(1).count(*count),
                    Once::Delimiter(value) => e.byte(2).text_str(*value),
                    Once::LineStarting(value) => e.byte(3).text(value),
                    Once::LineContaining(value) => e.byte(4).text(value),
                };
                self.command(first, depth + 1);
                self.command(second, depth + 1);
            }
            Cmd::SplitMany(name, splitter, child) => {
                e.op(op::SPLIT_MANY).name(name);
                match splitter {
                    Many::Lines(count) => e.byte(0).count(*count),
                    Many::Delimiter(value) => e.byte(1).text_str(*value),
                    Many::LinesStarting(value) => e.byte(2).text(value),
                };
                self.command(child, depth + 1);
            }
            Cmd::Fixed {
                name,
                fields,
                fill,
                delimited,
                empty_absent,
            } => {
                e.op(op::FIXED_WIDTH)
                    .name(name)
                    .byte(count_byte(fields.len()));
                for (field, ty, width) in fields {
                    e.name(field).ty(*ty).count(*width);
                }
                e.character(*fill)
                    .byte(u8::from(*delimited) | (u8::from(*empty_absent) << 1));
            }
            Cmd::Delimited {
                name,
                field_separator,
                record_separator,
                quote,
                escape,
                fields,
            } => {
                e.op(op::DELIMITED)
                    .name(name)
                    .text_str(*field_separator)
                    .text(record_separator)
                    .character(*quote)
                    .character(*escape)
                    .byte(count_byte(fields.len()));
                for (field, ty) in fields {
                    e.name(field).ty(*ty);
                }
            }
            Cmd::Switch {
                name,
                first_match,
                arms,
                default,
            } => {
                e.op(op::SWITCH)
                    .name(name)
                    .byte(u8::from(*first_match) | (u8::from(default.is_some()) << 1))
                    .byte(count_byte(arms.len()));
                for (condition, command) in arms {
                    let e = &mut self.encoder;
                    match condition {
                        Cond::Prefix(prefix) => e.byte(0).text(prefix),
                        Cond::Regex(pattern) => e.byte(1).pattern_str(*pattern),
                    };
                    self.command(command, depth + 1);
                }
                if let Some(default) = default {
                    self.command(default, depth + 1);
                }
            }
            Cmd::Chain(count, kind, inner) => {
                e.op(op::CHAIN).byte(count_byte_of(*count)).byte(*kind);
                if depth + count <= MAX_DECODE_DEPTH {
                    self.command(inner, depth + count);
                }
            }
            Cmd::Wide { name, count, kind } => {
                e.op(op::WIDE_SWITCH)
                    .name(name)
                    .byte(0)
                    .u16(*count)
                    .byte(*kind);
            }
        }
    }
}

fn count_byte(count: usize) -> u8 {
    assert!(count < 5, "at most four fields or arms per command");
    count_byte_of(count)
}

fn count_byte_of(count: usize) -> u8 {
    match u8::try_from(count) {
        Ok(byte) => byte,
        Err(_) => panic!("count {count} does not fit in one byte"),
    }
}

fn program(crlf: bool, bom: bool, root: &str, command: &Cmd<'_>, text: &str) -> Vec<u8> {
    let mut writer = Writer {
        encoder: Encoder::new(crlf, bom, root),
        commands: 0,
    };
    writer.command(command, 1);
    writer.encoder.finish(text)
}

fn doc(command: &Cmd<'_>, text: &str) -> Vec<u8> {
    program(false, false, "a", command, text)
}

fn json(layout: &str, text: &str) -> Vec<u8> {
    let mut bytes = layout.as_bytes().to_vec();
    bytes.push(JSON_TEXT_SEPARATOR);
    bytes.extend(text.as_bytes());
    bytes
}

/// A switch tree `levels` deep whose every arm matches input starting with
/// `,,,,`, with `leaf` under each innermost arm: `4^levels` leaves all read
/// the same input.
fn fan_out<'a>(levels: usize, leaf: &dyn Fn() -> Cmd<'a>) -> Cmd<'a> {
    if levels == 0 {
        return leaf();
    }
    let arms = [(",", "a"), (",,", "b"), (",,,", "c"), (",,,,", "d")]
        .into_iter()
        .map(|(prefix, name)| {
            let arm = match fan_out(levels - 1, leaf) {
                Cmd::Switch {
                    arms,
                    first_match,
                    default,
                    ..
                } => Cmd::Switch {
                    name,
                    first_match,
                    arms,
                    default,
                },
                Cmd::SplitMany(_, splitter, child) => Cmd::SplitMany(name, splitter, child),
                other => other,
            };
            (Cond::Prefix(prefix), arm)
        })
        .collect();
    switch("a", false, arms, None)
}

/// Every seed as `(name, bytes)`.
pub fn all() -> Vec<(&'static str, Vec<u8>)> {
    let mut seeds = program_seeds();
    seeds.extend(json_seeds());
    seeds
}

fn program_seeds() -> Vec<(&'static str, Vec<u8>)> {
    let record_line = || {
        split_once(
            "c",
            Once::Delimiter(Str::Lit(",")),
            store("a", Text),
            Cmd::Store("b", Int, Some((Side::Right, "\r\n"))),
        )
    };
    let commas = ",".repeat(4_096);
    vec![
        ("empty", Vec::new()),
        (
            "store_trim_unicode",
            doc(
                &Cmd::Store("名前", Text, Some((Side::Both, "_ "))),
                " _ünï_ ",
            ),
        ),
        (
            "store_trim_left",
            doc(&Cmd::Store("b", Text, Some((Side::Left, "#"))), "##x#"),
        ),
        ("store_int", doc(&store("a", Int), "-42")),
        ("store_int_invalid", doc(&store("a", Int), "4 2")),
        ("store_float_nan", doc(&store("a", Float), "NaN")),
        ("store_float", doc(&store("a", Float), "1e300")),
        ("store_bool", doc(&store("a", Bool), "true")),
        (
            "store_bom_crlf",
            program(true, true, "名前", &store("a", Text), "\u{feff}x\r\ny"),
        ),
        (
            "store_carriage_return_before_line_feed",
            doc(&store("a", Text), "x\r\r\n"),
        ),
        (
            "split_once_carriage_return_delimiter",
            doc(
                &split_once(
                    "a",
                    Once::Delimiter(Str::Lit("\r")),
                    store("b", Text),
                    store("c", Text),
                ),
                "a\r\r\nb",
            ),
        ),
        (
            "split_once_delimiter_unicode",
            doc(
                &split_once(
                    "a",
                    Once::Delimiter(Str::Lit("é")),
                    store("b", Text),
                    store("c", Text),
                ),
                "leftérightémore",
            ),
        ),
        (
            "split_once_columns_unicode",
            doc(
                &split_once("a", Once::Columns(3), store("b", Text), store("c", Text)),
                "日本語😀text",
            ),
        ),
        (
            "split_once_columns_short",
            doc(
                &split_once("a", Once::Columns(8), store("b", Text), store("c", Text)),
                "ab",
            ),
        ),
        (
            "split_once_lines_crlf",
            program(
                true,
                false,
                "a",
                &split_once("a", Once::Lines(2), store("b", Text), store("c", Text)),
                "l1\r\nl2\r\nrest",
            ),
        ),
        (
            "split_once_line_starting",
            doc(
                &split_once("a", Once::LineStarting("#"), Cmd::Ignore, store("c", Text)),
                "head\n#tail\n#more",
            ),
        ),
        (
            "split_once_line_containing",
            doc(
                &split_once(
                    "a",
                    Once::LineContaining("日本"),
                    store("b", Text),
                    store("c", Text),
                ),
                "x\ny日本z\nw",
            ),
        ),
        (
            "split_many_delimiter_trailing",
            doc(
                &split_many("a", Many::Delimiter(Str::Lit(",")), store("b", Text)),
                "x,y,,z,",
            ),
        ),
        (
            "split_many_lines",
            doc(
                &split_many("a", Many::Lines(2), store("b", Text)),
                "1\n2\n3\n4\n5",
            ),
        ),
        (
            "split_many_lines_starting",
            doc(
                &split_many("a", Many::LinesStarting("H"), store("b", Text)),
                "pre\nH1\nd\nH2\nd",
            ),
        ),
        (
            "split_many_bom_delimiter",
            doc(
                &split_many("a", Many::Delimiter(Str::Lit("\u{feff}")), store("b", Text)),
                "\u{feff}\u{feff}x\u{feff}y",
            ),
        ),
        (
            "split_many_empty_input",
            doc(&split_many("a", Many::Lines(1), store("b", Text)), ""),
        ),
        (
            "fixed_width_delimited",
            doc(
                &fixed("a", vec![("b", Text, 3), ("c", Int, 2)], true, true),
                "ab 12\r\nxyz  \n",
            ),
        ),
        (
            "fixed_width_undelimited",
            doc(
                &fixed("a", vec![("b", Text, 2), ("c", Float, 3)], false, true),
                "日本1.5é  0.",
            ),
        ),
        (
            "fixed_width_not_multiple",
            doc(
                &fixed("a", vec![("b", Text, 2), ("c", Int, 2)], false, true),
                "abc",
            ),
        ),
        (
            "fixed_width_short_final_string",
            doc(
                &fixed("a", vec![("b", Int, 2), ("c", Text, 5)], true, false),
                "12x\n34",
            ),
        ),
        (
            "fixed_width_empty_int_present",
            doc(
                &fixed("a", vec![("b", Int, 2), ("c", Text, 1)], true, false),
                "  x",
            ),
        ),
        (
            "fixed_width_unicode_fill",
            doc(
                &Cmd::Fixed {
                    name: "a",
                    fields: vec![("b", Text, 3), ("c", Bool, 5)],
                    fill: '😀',
                    delimited: true,
                    empty_absent: true,
                },
                "x😀😀true😀",
            ),
        ),
        (
            "fixed_width_huge_field",
            doc(
                &fixed("a", vec![("b", Text, u32::MAX)], true, true),
                "abc\ndef",
            ),
        ),
        (
            "fixed_columns_huge",
            doc(
                &split_once(
                    "a",
                    Once::Columns(u32::MAX),
                    store("b", Text),
                    store("c", Text),
                ),
                "abc",
            ),
        ),
        (
            "delimited_quoted",
            doc(
                &delimited("a", ",", "\n", '"', '"', vec![("b", Text), ("c", Text)]),
                "x,\"y,\"\"z\"\"\"\n1,\n",
            ),
        ),
        (
            "delimited_backslash_escape",
            doc(
                &delimited("a", ";", "\r\n", '\'', '\\', vec![("b", Text), ("c", Int)]),
                "'a\\'b\\\\';7\r\n'é';\n",
            ),
        ),
        (
            "delimited_trailing_escape",
            doc(
                &delimited("a", ",", "\n", '"', '\\', vec![("b", Text)]),
                "\"x\\",
            ),
        ),
        (
            "delimited_unterminated",
            doc(
                &delimited("a", ",", "\n", '"', '"', vec![("b", Text)]),
                "\"open",
            ),
        ),
        (
            "delimited_field_count",
            doc(
                &delimited("a", ",", "\n", '"', '"', vec![("b", Text), ("c", Text)]),
                "1,2\n3",
            ),
        ),
        (
            "delimited_multichar_separators",
            doc(
                &delimited(
                    "a",
                    "||",
                    "日本",
                    '"',
                    '"',
                    vec![("b", Int), ("c", Float), ("d", Bool)],
                ),
                "1||2.5||true日本||||日本",
            ),
        ),
        (
            "switch_first_match",
            doc(
                &switch(
                    "a",
                    true,
                    vec![
                        (Cond::Prefix("AB"), store("b", Text)),
                        (Cond::Regex(Str::Lit("^H")), store("c", Text)),
                    ],
                    Some(store("d", Text)),
                ),
                "ABH",
            ),
        ),
        (
            "switch_all_possible",
            doc(
                &switch(
                    "a",
                    false,
                    vec![
                        (Cond::Prefix("ab"), store("b", Text)),
                        (Cond::Regex(Str::Lit("a|b")), store("c", Text)),
                        (Cond::Regex(Str::Lit(r"\d+")), store("d", Int)),
                    ],
                    None,
                ),
                "ab",
            ),
        ),
        (
            "switch_default",
            doc(
                &switch(
                    "a",
                    false,
                    vec![(Cond::Prefix("#"), store("b", Text))],
                    Some(store("c", Text)),
                ),
                "plain",
            ),
        ),
        (
            "switch_matched_ignore",
            doc(
                &switch(
                    "a",
                    true,
                    vec![(Cond::Prefix("#"), Cmd::Ignore)],
                    Some(store("c", Text)),
                ),
                "#comment",
            ),
        ),
        (
            "switch_regex_unicode",
            doc(
                &switch(
                    "a",
                    true,
                    vec![
                        (Cond::Regex(Str::Lit("(?i)é")), store("b", Text)),
                        (Cond::Regex(Str::Lit(r"\p{Greek}")), store("c", Text)),
                    ],
                    None,
                ),
                "xÉ",
            ),
        ),
        (
            "nested_records",
            program(
                true,
                false,
                "a",
                &split_many(
                    "a",
                    Many::Lines(1),
                    switch(
                        "b",
                        true,
                        vec![
                            (Cond::Prefix("H"), record_line()),
                            (
                                Cond::Regex(Str::Lit("(?m)^X$")),
                                fixed("d", vec![("e", Text, 1)], true, true),
                            ),
                        ],
                        Some(Cmd::Store("f", Text, Some((Side::Right, "\n")))),
                    ),
                ),
                "Hname,7\r\nX\nother\nHnext,8\n",
            ),
        ),
        (
            "chain_split_once_depth_limit",
            doc(
                &Cmd::Chain(63, chain::SPLIT_ONCE, Box::new(store("a", Text))),
                "x|y",
            ),
        ),
        (
            "chain_split_once_too_deep",
            doc(
                &Cmd::Chain(64, chain::SPLIT_ONCE, Box::new(store("a", Text))),
                "x",
            ),
        ),
        (
            "chain_split_many_deep",
            doc(
                &Cmd::Chain(40, chain::SPLIT_MANY, Box::new(store("a", Text))),
                "x",
            ),
        ),
        (
            "chain_switch_default_depth_limit",
            doc(
                &Cmd::Chain(63, chain::SWITCH_DEFAULT, Box::new(store("a", Text))),
                "x",
            ),
        ),
        (
            "chain_switch_arm_json_limit",
            doc(
                &Cmd::Chain(31, chain::SWITCH_ARM, Box::new(store("a", Text))),
                "cx",
            ),
        ),
        (
            // The trim object adds the 96th JSON level exactly.
            "chain_switch_arm_at_json_limit",
            doc(
                &Cmd::Chain(
                    31,
                    chain::SWITCH_ARM,
                    Box::new(Cmd::Store("a", Text, Some((Side::Both, " ")))),
                ),
                "cx",
            ),
        ),
        (
            "chain_switch_arm_past_json_limit",
            doc(
                &Cmd::Chain(32, chain::SWITCH_ARM, Box::new(store("a", Text))),
                "cx",
            ),
        ),
        (
            "chain_switch_arm_depth_limit",
            doc(
                &Cmd::Chain(63, chain::SWITCH_ARM, Box::new(store("a", Text))),
                "cx",
            ),
        ),
        (
            "chain_switch_arm_too_deep",
            doc(
                &Cmd::Chain(64, chain::SWITCH_ARM, Box::new(store("a", Text))),
                "cx",
            ),
        ),
        (
            "wide_switch_node_limit",
            doc(
                &Cmd::Wide {
                    name: "a",
                    count: 4_095,
                    kind: wide::LITERAL,
                },
                "p4094;",
            ),
        ),
        (
            "wide_switch_too_many_nodes",
            doc(
                &Cmd::Wide {
                    name: "a",
                    count: 4_096,
                    kind: wide::LITERAL,
                },
                "p1;",
            ),
        ),
        (
            "wide_regex_limit",
            doc(
                &Cmd::Wide {
                    name: "a",
                    count: count_u16(MAX_SWITCH_REGEXES),
                    kind: wide::REGEX,
                },
                "r63;",
            ),
        ),
        (
            "wide_regex_too_many",
            doc(
                &Cmd::Wide {
                    name: "a",
                    count: count_u16(MAX_WIDE_REGEX_ARMS),
                    kind: wide::REGEX,
                },
                "r1;",
            ),
        ),
        (
            "too_many_nodes",
            doc(
                &fan_out(4, &|| {
                    split_many("a", Many::Delimiter(Str::Lit(",")), store("b", Text))
                }),
                &commas,
            ),
        ),
        (
            "too_many_records",
            doc(
                &fan_out(4, &|| {
                    split_many(
                        "a",
                        Many::Delimiter(Str::Lit(",")),
                        switch("b", true, vec![(Cond::Prefix("#"), store("c", Text))], None),
                    )
                }),
                &commas,
            ),
        ),
        (
            "empty_root_name",
            program(false, false, "", &store("a", Text), ""),
        ),
        ("empty_command_name", doc(&store("", Text), "")),
        (
            "empty_once_delimiter",
            doc(
                &split_once(
                    "a",
                    Once::Delimiter(Str::Lit("")),
                    store("b", Text),
                    store("c", Text),
                ),
                "x",
            ),
        ),
        (
            "empty_many_delimiter",
            doc(
                &split_many("a", Many::Delimiter(Str::Lit("")), store("b", Text)),
                "x",
            ),
        ),
        (
            "duplicate_split_names",
            doc(
                &split_once("a", Once::Columns(1), store("b", Text), store("b", Text)),
                "xy",
            ),
        ),
        (
            "duplicate_switch_prefix",
            doc(
                &switch(
                    "a",
                    false,
                    vec![
                        (Cond::Prefix("#"), store("b", Text)),
                        (Cond::Prefix("#"), store("c", Text)),
                    ],
                    None,
                ),
                "#",
            ),
        ),
        (
            "empty_switch",
            doc(&switch("a", false, Vec::new(), None), ""),
        ),
        (
            "switch_without_output",
            doc(
                &switch(
                    "a",
                    false,
                    vec![(Cond::Prefix("#"), Cmd::Ignore)],
                    Some(Cmd::Ignore),
                ),
                "",
            ),
        ),
        (
            "split_once_without_output",
            doc(
                &split_once("a", Once::Lines(1), Cmd::Ignore, Cmd::Ignore),
                "",
            ),
        ),
        (
            "split_many_without_output",
            doc(&split_many("a", Many::Lines(1), Cmd::Ignore), ""),
        ),
        ("empty_record", doc(&fixed("a", Vec::new(), true, true), "")),
        (
            "duplicate_record_fields",
            doc(
                &delimited("a", ",", "\n", '"', '"', vec![("b", Text), ("b", Int)]),
                "",
            ),
        ),
        (
            "dialect_line_break_quote",
            doc(&delimited("a", ",", "\n", '\n', '"', vec![("b", Text)]), ""),
        ),
        (
            "dialect_quote_in_separator",
            doc(&delimited("a", "\"", "\n", '"', '"', vec![("b", Text)]), ""),
        ),
        (
            "dialect_empty_record_separator",
            doc(&delimited("a", ",", "", '"', '"', vec![("b", Text)]), ""),
        ),
        (
            "fill_line_break",
            doc(
                &Cmd::Fixed {
                    name: "a",
                    fields: vec![("b", Text, 1)],
                    fill: '\n',
                    delimited: true,
                    empty_absent: true,
                },
                "",
            ),
        ),
        (
            "duplicate_trim_character",
            doc(&Cmd::Store("a", Text, Some((Side::Both, "aa"))), ""),
        ),
        (
            "empty_trim_characters",
            doc(&Cmd::Store("a", Text, Some((Side::Right, ""))), ""),
        ),
        (
            "invalid_regex",
            doc(
                &switch(
                    "a",
                    false,
                    vec![(Cond::Regex(Str::Lit("(")), store("b", Text))],
                    None,
                ),
                "",
            ),
        ),
        (
            "regex_too_large",
            doc(
                &switch(
                    "a",
                    false,
                    vec![(Cond::Regex(Str::Lit("a{1000}{1000}")), store("b", Text))],
                    None,
                ),
                "",
            ),
        ),
        (
            "regex_empty",
            doc(
                &switch(
                    "a",
                    false,
                    vec![(Cond::Regex(Str::Lit("")), store("b", Text))],
                    None,
                ),
                "",
            ),
        ),
        (
            "regex_pattern_over_limit",
            doc(
                &switch(
                    "a",
                    false,
                    vec![(Cond::Regex(Str::OverLimit), store("b", Text))],
                    None,
                ),
                "",
            ),
        ),
        ("name_over_limit", doc(&Cmd::LongName(Str::OverLimit), "")),
        ("name_at_limit", doc(&Cmd::LongName(Str::AtLimit), "")),
        (
            "string_budget_fallback",
            doc(
                &split_many(
                    "a",
                    Many::Delimiter(Str::AtLimit),
                    Cmd::LongName(Str::OverLimit),
                ),
                "x",
            ),
        ),
        (
            "delimiter_at_limit",
            doc(
                &split_many("a", Many::Delimiter(Str::AtLimit), store("b", Text)),
                "x",
            ),
        ),
    ]
}

fn count_u16(count: usize) -> u16 {
    match u16::try_from(count) {
        Ok(count) => count,
        Err(_) => panic!("count {count} does not fit in two bytes"),
    }
}

fn json_seeds() -> Vec<(&'static str, Vec<u8>)> {
    // 130 nested `first` commands pass serde_json's 128-level recursion
    // limit before the layout validator sees them.
    let deep = format!(
        r#"{{"root_name":"a","command":{}{{"kind":"store","name":"v","ty":"string"}}{},"output_line_ending":"lf"}}"#,
        r#"{"kind":"split_once","name":"s","splitter":{"kind":"fixed_lines","value":1},"second":{"kind":"ignore"},"first":"#
            .repeat(130),
        "}".repeat(130)
    );
    vec![
        (
            "json_store",
            json(
                r#"{"root_name":"doc","command":{"kind":"store","name":"v","ty":"string"},"output_line_ending":"lf"}"#,
                "text",
            ),
        ),
        (
            "json_records",
            json(
                r#"{"root_name":"doc","command":{"kind":"split_many","name":"rows","splitter":{"kind":"fixed_lines","value":1},"child":{"kind":"switch","name":"row","mode":"first_match","arms":[{"prefix":"H","command":{"kind":"split_once","name":"head","splitter":{"kind":"delimiter","value":","},"first":{"kind":"store","name":"id","ty":"int","trim":{"side":"both","characters":"H "}},"second":{"kind":"ignore"}}},{"prefix":"^D","contains_regex":true,"command":{"kind":"delimited_records","name":"detail","dialect":{"field_separator":";","record_separator":"\n","quote":"\"","escape":"\\"},"fields":[{"name":"x","ty":"string"},{"name":"y","ty":"float"}]}}],"default":{"kind":"fixed_width_records","name":"other","fields":[{"name":"z","ty":"bool","width":5}],"fill_char":"_","treat_empty_as_absent":false}}},"output_line_ending":"crlf","write_bom":true}"#,
                "H 12,rest\r\nDa;1.5\nfalse\n",
            ),
        ),
        (
            "json_empty_delimiter",
            json(
                r#"{"root_name":"doc","command":{"kind":"split_once","name":"s","splitter":{"kind":"delimiter","value":""},"first":{"kind":"store","name":"a","ty":"string"},"second":{"kind":"ignore"}},"output_line_ending":"lf"}"#,
                "x",
            ),
        ),
        (
            "json_zero_width",
            json(
                r#"{"root_name":"doc","command":{"kind":"fixed_width_records","name":"r","fields":[{"name":"a","ty":"string","width":0}]},"output_line_ending":"lf"}"#,
                "",
            ),
        ),
        (
            "json_unknown_kind",
            json(
                r#"{"root_name":"doc","command":{"kind":"loop","name":"x"},"output_line_ending":"lf"}"#,
                "",
            ),
        ),
        (
            "json_truncated",
            json(r#"{"root_name":"doc","command":{"kind":"st"#, ""),
        ),
        ("json_deep_nesting", json(&deep, "")),
    ]
}
