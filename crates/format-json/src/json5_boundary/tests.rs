mod corpus;

use super::{
    Json5SyntaxError, Json5SyntaxKind, Json5SyntaxResource, Limits, MAX_CONTAINER_DEPTH,
    MAX_NORMALIZED_BYTES, MAX_ORIGINAL_BYTES, MAX_SYNTAX_WORK, normalize, normalize_limited,
};

fn success(input: &str, expected: &str) {
    let observed = normalize(input);
    eprintln!("syntax input={input:?}, original result={observed:?}");
    assert_eq!(observed.as_deref(), Ok(expected));
}

fn syntax(input: &str, kind: Json5SyntaxKind, offset: usize) {
    let observed = normalize(input);
    eprintln!("syntax input={input:?}, original result={observed:?}");
    assert_eq!(observed, Err(Json5SyntaxError::Syntax { kind, offset }));
}

#[test]
fn authored_corpus_distinguishes_syntax_from_later_schema_refusals() {
    let mut successes = 0;
    let mut refusals = 0;
    for case in corpus::CASES {
        let observed = normalize(case.input);
        eprintln!(
            "{}: original input={:?}, original result={observed:?}",
            case.id, case.input
        );
        match (case.normalized, case.error) {
            (Some(expected), None) => {
                assert_eq!(observed.as_deref(), Ok(expected), "{}", case.id);
                successes += 1;
            }
            (None, Some(expected)) => {
                assert!(
                    matches!(observed, Err(Json5SyntaxError::Syntax { kind, offset })
                    if kind == expected && offset <= case.input.len()),
                    "{}: {observed:?}",
                    case.id
                );
                refusals += 1;
            }
            _ => panic!("incomplete independent corpus expectation: {}", case.id),
        }
    }
    assert_eq!((corpus::CASES.len(), successes, refusals), (75, 46, 29));
}

#[test]
fn complete_grammar_handles_nested_values_and_preserves_duplicate_members() {
    for (input, expected) in [
        (
            " /*before*/ [1,{true:null, false:0, $x_1:'雪',},[],] //eof",
            "[1,{\"true\":null,\"false\":0,\"$x_1\":\"雪\"},[]]",
        ),
        ("{a:1,/*between*/a:{a:2,},}", "{\"a\":1,\"a\":{\"a\":2}}"),
        ("{a:1,a:false,a:null}", "{\"a\":1,\"a\":false,\"a\":null}"),
        ("[+1,-0,1.,.25,1E+01,]", "[1,0,1.0,0.25,1E+01]"),
        ("null", "null"),
        ("'scalar'", "\"scalar\""),
        ("{}", "{}"),
        ("[]", "[]"),
    ] {
        success(input, expected);
    }
    // A comma after the complete root value is not a trailing container comma.
    syntax(
        " /*before*/ [1,{true:null, false:0, $x_1:'雪',},[],], //eof",
        Json5SyntaxKind::TrailingRootValue,
        53,
    );
    for input in [
        "",
        "/*only*/",
        "{",
        "[",
        "[1,,]",
        "{a:}",
        "{a:1 b:2}",
        "{a:1]",
        "[1}",
        "{,}",
        "[,]",
    ] {
        let observed = normalize(input);
        eprintln!("incomplete input={input:?}, original result={observed:?}");
        assert!(matches!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnexpectedToken,
                ..
            })
        ));
    }
}

#[test]
fn integer_edges_decimal_spelling_and_every_nonfinite_token_are_checked() {
    for (input, expected) in [
        ("18446744073709551615", "18446744073709551615"),
        ("0xffffffffffffffff", "18446744073709551615"),
        ("-9223372036854775808", "-9223372036854775808"),
        ("-0x8000000000000000", "-9223372036854775808"),
        ("-0e0", "-0e0"),
        ("+0x0", "0"),
        ("-0X0", "0"),
        ("+1.2300E-02", "1.2300E-02"),
        ("-.5e+01", "-0.5e+01"),
        ("5.e0", "5.0e0"),
        ("1e-400", "1e-400"),
        ("1.7976931348623157e308", "1.7976931348623157e308"),
    ] {
        success(input, expected);
    }
    for input in [
        "18446744073709551616",
        "-9223372036854775809",
        "0x10000000000000000",
        "-0x8000000000000001",
    ] {
        syntax(input, Json5SyntaxKind::IntegerOutOfRange, 0);
    }
    for input in [
        "00", "00.1", "0xG", "1e", "1e-", ".", "+", "--1", "1.2.3", "1_0",
    ] {
        syntax(input, Json5SyntaxKind::InvalidNumber, 0);
    }
    for input in [
        "Infinity",
        "+Infinity",
        "-Infinity",
        "NaN",
        "+NaN",
        "-NaN",
        "1e400",
        "1.7976931348623159e308",
    ] {
        syntax(input, Json5SyntaxKind::NonFiniteNumber, 0);
    }
    syntax(
        "{ignored:Infinity,n:7}",
        Json5SyntaxKind::NonFiniteNumber,
        9,
    );
    syntax("{n:NaN,n:null}", Json5SyntaxKind::NonFiniteNumber, 3);
    syntax("[0,1e400]", Json5SyntaxKind::NonFiniteNumber, 3);
}

#[test]
fn whitespace_strings_and_offsets_use_original_utf8_bytes() {
    const SPACES: &[char] = &[
        '\t', '\n', '\u{000b}', '\u{000c}', '\r', ' ', '\u{00a0}', '\u{1680}', '\u{2000}',
        '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}',
        '\u{2008}', '\u{2009}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}',
        '\u{3000}', '\u{feff}',
    ];
    for space in SPACES {
        success(&format!("{space}{{n:7}}{space}"), "{\"n\":7}");
    }
    success("{\\u006e0:7}", "{\"n0\":7}");
    success("{'雪':'\\uD83D\\uDE00'}", "{\"雪\":\"😀\"}");
    success("{n:'a\\\rb\\\u{2029}c'}", "{\"n\":\"abc\"}");
    success("{n:'\u{0000}\u{001f}'}", "{\"n\":\"\\u0000\\u001f\"}");
    success(
        "{n:'\\\"\\\\\\/\\b\\f\\r'}",
        "{\"n\":\"\\\"\\\\/\\b\\f\\r\"}",
    );
    syntax("\u{0085}{}", Json5SyntaxKind::UnexpectedToken, 0);
    syntax(
        "\u{feff}/*雪*/{n:NaN}",
        Json5SyntaxKind::NonFiniteNumber,
        13,
    );
    syntax("{n:7}\u{00a0}[]", Json5SyntaxKind::TrailingRootValue, 7);
    syntax("{n:7}/*雪", Json5SyntaxKind::UnterminatedComment, 5);
    syntax(" {n:'a\\1'}", Json5SyntaxKind::InvalidEscape, 6);
    syntax("{雪:7}", Json5SyntaxKind::UnsupportedIdentifier, 1);
    syntax("{'雪':'\\uD800'}", Json5SyntaxKind::InvalidEscape, 8);
    syntax("{n 7}", Json5SyntaxKind::UnexpectedToken, 3);
    syntax("'unfinished", Json5SyntaxKind::UnterminatedString, 0);
    syntax("/", Json5SyntaxKind::UnexpectedToken, 0);
    syntax("null/", Json5SyntaxKind::UnexpectedToken, 4);
    syntax("{\\:1}", Json5SyntaxKind::InvalidEscape, 1);
    syntax("{\\uD800:1}", Json5SyntaxKind::InvalidEscape, 1);
}

#[test]
fn actual_container_depth_is_bounded_without_recursive_parser_calls() {
    let exact = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    success(&exact, &exact);
    let excess = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    let observed = normalize(&excess);
    eprintln!("depth128 original result={observed:?}");
    assert_eq!(
        observed,
        Err(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::ContainerDepth,
            offset: 127,
            requested: 128,
            max: 127,
        })
    );
    let limits = Limits {
        depth: 1,
        ..Limits::default()
    };
    let observed = normalize_limited("/*[[*/['[[',[]]", limits).map(|result| result.output);
    eprintln!("real second container original result={observed:?}");
    assert_eq!(
        observed,
        Err(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::ContainerDepth,
            offset: 12,
            requested: 2,
            max: 1,
        })
    );
}

#[test]
fn scaled_byte_work_and_checked_overflow_controls_are_not_public_cap_claims() {
    assert_eq!(
        (
            MAX_ORIGINAL_BYTES,
            MAX_NORMALIZED_BYTES,
            MAX_CONTAINER_DEPTH,
            MAX_SYNTAX_WORK
        ),
        (67_108_864, 67_108_864, 127, 536_870_912)
    );
    let limits = Limits {
        original: 7,
        ..Limits::default()
    };
    let exact = normalize_limited("\u{feff}null", limits);
    eprintln!(
        "original7 raw result={:?}",
        exact.as_ref().map(|result| (&result.output, result.work))
    );
    assert_eq!(exact.map(|result| result.output), Ok("null".into()));
    let observed = normalize_limited("\u{feff}null ", limits).map(|result| result.output);
    eprintln!("original8 raw result={observed:?}");
    assert_eq!(
        observed,
        Err(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::OriginalDocumentBytes,
            offset: 0,
            requested: 8,
            max: 7,
        })
    );
    let source = "{n:'\\0'}";
    let exact = normalize_limited(
        source,
        Limits {
            normalized: 14,
            ..Limits::default()
        },
    );
    eprintln!(
        "normalized14 raw result={:?}",
        exact.as_ref().map(|result| (&result.output, result.work))
    );
    assert_eq!(
        exact.map(|result| result.output),
        Ok("{\"n\":\"\\u0000\"}".into())
    );
    let observed = normalize_limited(
        source,
        Limits {
            normalized: 13,
            ..Limits::default()
        },
    )
    .map(|result| result.output);
    eprintln!("normalized14 refusal raw result={observed:?}");
    assert_eq!(
        observed,
        Err(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::NormalizedDocumentBytes,
            offset: source.len(),
            requested: 14,
            max: 13,
        })
    );
    // Independent census for null: dispatch2 + lookahead11 + consumed4 +
    // reserved-token comparisons12 + append4 + state transition1 + EOF2=36.
    let exact = normalize_limited(
        "null",
        Limits {
            work: 36,
            ..Limits::default()
        },
    );
    eprintln!(
        "work36 raw result={:?}",
        exact.as_ref().map(|result| (&result.output, result.work))
    );
    assert_eq!(
        exact.map(|result| (result.output, result.work)),
        Ok(("null".into(), 36))
    );
    let observed = normalize_limited(
        "null",
        Limits {
            work: 35,
            ..Limits::default()
        },
    )
    .map(|result| result.output);
    eprintln!("work35 raw result={observed:?}");
    assert_eq!(
        observed,
        Err(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::SyntaxWork,
            offset: 4,
            requested: 36,
            max: 35,
        })
    );
    let mut parser = super::Parser {
        source: "",
        index: 0,
        output: String::new(),
        states: vec![super::State::RootValue],
        work: usize::MAX,
        limits: Limits {
            work: usize::MAX,
            ..Limits::default()
        },
    };
    let observed = parser.charge(1);
    eprintln!("internal arithmetic overflow raw result={observed:?}");
    assert_eq!(
        observed,
        Err(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::SyntaxWork,
            offset: 0,
            requested: usize::MAX,
            max: usize::MAX,
        })
    );
    // This last direct counter control is only integer-arithmetic coverage.
    // It is not represented as a real-input public work-limit witness.
}
