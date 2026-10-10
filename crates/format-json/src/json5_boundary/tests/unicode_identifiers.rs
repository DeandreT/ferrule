use std::error::Error;

use super::super::{identifiers, normalize};
use super::{Json5SyntaxError, Json5SyntaxKind, Json5SyntaxResource, Limits, normalize_limited};

fn capture(id: &str, input: &str, observed: &Result<String, Json5SyntaxError>) {
    eprintln!(
        "{id}: original input UTF8={:?}; result={observed:?}",
        input.as_bytes()
    );
    if let Ok(output) = observed {
        eprintln!("complete output UTF8={:?}", output.as_bytes());
    }
    if let Err(error) = observed {
        eprintln!("complete Display={error}; source={:?}", error.source());
    }
}

#[test]
fn frozen_unicode_identifier_documents_and_original_offsets() {
    // Literal expectations are independent of this parser and generated table.
    {
        let input = "{$x_0:0,true:1,null:2,default:3}";
        let observed = normalize(input);
        capture("ascii-keyword-conservation", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"$x_0\":0,\"true\":1,\"null\":2,\"default\":3}".as_bytes())
        );
    }
    {
        let input = "{\u{c0}:0}";
        let observed = normalize(input);
        capture("start-lu", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{c0}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{3b1}:0}";
        let observed = normalize(input);
        capture("start-ll", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{3b1}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{1c5}:0}";
        let observed = normalize(input);
        capture("start-lt", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{1c5}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{2b0}:0}";
        let observed = normalize(input);
        capture("start-lm", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{2b0}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{96ea}:0}";
        let observed = normalize(input);
        capture("start-lo", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{96ea}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{2160}:0}";
        let observed = normalize(input);
        capture("start-nl", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{2160}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\u{301}:0}";
        let observed = normalize(input);
        capture("continue-mn", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{301}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\u{903}:0}";
        let observed = normalize(input);
        capture("continue-mc", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{903}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\u{661}:0}";
        let observed = normalize(input);
        capture("continue-nd", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{661}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\u{203f}:0}";
        let observed = normalize(input);
        capture("continue-pc", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{203f}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\u{200c}\u{200d}:0}";
        let observed = normalize(input);
        capture("continue-join-controls", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{200c}\u{200d}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{10400}:0}";
        let observed = normalize(input);
        capture("raw-supplementary-lu", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{10400}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{1e6c0}:0}";
        let observed = normalize(input);
        capture("unicode17-tai-yo-lo", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{1e6c0}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{1c89}:0}";
        let observed = normalize(input);
        capture("unicode16-cyrillic-lu", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{1c89}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\u{11de0}:0}";
        let observed = normalize(input);
        capture("unicode17-supplementary-nd-continue", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{11de0}\":0}".as_bytes())
        );
    }
    {
        let input = "{\\u00C0:0}";
        let observed = normalize(input);
        capture("escaped-lu", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{c0}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\\u0301:0}";
        let observed = normalize(input);
        capture("escaped-mn-continue", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{301}\":0}".as_bytes())
        );
    }
    {
        let input = "{\\u2160:0}";
        let observed = normalize(input);
        capture("escaped-nl", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{2160}\":0}".as_bytes())
        );
    }
    {
        let input = "{\\uD801\\uDC00:0}";
        let observed = normalize(input);
        capture("paired-escape-supplementary-lu", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{10400}\":0}".as_bytes())
        );
    }
    {
        let input = "{\\uD839\\uDEC0:0}";
        let observed = normalize(input);
        capture("paired-escape-unicode17-lo", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{1e6c0}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\\u200C\\u200D:0}";
        let observed = normalize(input);
        capture("escaped-join-controls", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{200c}\u{200d}\":0}".as_bytes())
        );
    }
    {
        let input = "{\\u005F0:0}";
        let observed = normalize(input);
        capture("escaped-underscore", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"_0\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{e9}:0,e\u{301}:1}";
        let observed = normalize(input);
        capture("composed-decomposed-distinct", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{e9}\":0,\"e\u{301}\":1}".as_bytes())
        );
    }
    {
        let input = "{\u{96ea}:0,\\u96EA:1}";
        let observed = normalize(input);
        capture("duplicate-decoded-name", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{96ea}\":0,\"\u{96ea}\":1}".as_bytes())
        );
    }
    {
        let input = "{\\u96eA:0}";
        let observed = normalize(input);
        capture("mixed-case-hex", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{96ea}\":0}".as_bytes())
        );
    }
    {
        let input = "{\"\u{e9}\":0,\u{10400}:1}";
        let observed = normalize(input);
        capture("multibyte-prefix-success", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{e9}\":0,\"\u{10400}\":1}".as_bytes())
        );
    }
    {
        let input = "{'\u{1f600}':0,'\u{2118}':1,'\u{b7}':2}";
        let observed = normalize(input);
        capture("quoted-excluded-scalars", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{1f600}\":0,\"\u{2118}\":1,\"\u{b7}\":2}".as_bytes())
        );
    }
    {
        let input = "{'\\uD801\\uDC00':0}";
        let observed = normalize(input);
        capture("quoted-supplementary-pair", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"\u{10400}\":0}".as_bytes())
        );
    }
    {
        let input = "{n:'\\uD83D\\uDE00'}";
        let observed = normalize(input);
        capture("quoted-value-supplementary-pair", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"n\":\"\u{1f600}\"}".as_bytes())
        );
    }
    {
        let input = "{a\u{fe0f}:0}";
        let observed = normalize(input);
        capture("continue-variation-selector", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{fe0f}\":0}".as_bytes())
        );
    }
    {
        let input = "{a\u{e0100}:0}";
        let observed = normalize(input);
        capture("continue-supplementary-mn", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"a\u{e0100}\":0}".as_bytes())
        );
    }
    {
        let input = "{\u{301}a:0}";
        let observed = normalize(input);
        capture("start-mn", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{903}a:0}";
        let observed = normalize(input);
        capture("start-mc", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{661}a:0}";
        let observed = normalize(input);
        capture("start-arabic-nd", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{203f}a:0}";
        let observed = normalize(input);
        capture("start-pc", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{200c}a:0}";
        let observed = normalize(input);
        capture("start-zwnj", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{200d}a:0}";
        let observed = normalize(input);
        capture("start-zwj", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{a\u{20dd}:0}";
        let observed = normalize(input);
        capture("excluded-enclosing-mark", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 2
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 2 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{2118}:0}";
        let observed = normalize(input);
        capture("excluded-other-id-start-sm", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{309b}:0}";
        let observed = normalize(input);
        capture("excluded-other-id-start-sk", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{a\u{b7}:0}";
        let observed = normalize(input);
        capture("excluded-other-id-continue", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 2
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 2 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{1f600}:0}";
        let observed = normalize(input);
        capture("raw-emoji-start", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{a\u{61c}:0}";
        let observed = normalize(input);
        capture("excluded-format-continue", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 2
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 2 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{e000}:0}";
        let observed = normalize(input);
        capture("excluded-private-use", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{378}:0}";
        let observed = normalize(input);
        capture("excluded-unassigned", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{10ffff}:0}";
        let observed = normalize(input);
        capture("excluded-last-noncharacter", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{9a:0}";
        let observed = normalize(input);
        capture("ascii-leading-digit", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnexpectedToken,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnexpectedToken, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnexpectedToken at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\u0039a:0}";
        let observed = normalize(input);
        capture("escaped-leading-digit", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\u0020:0}";
        let observed = normalize(input);
        capture("escaped-space", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\u003A:0}";
        let observed = normalize(input);
        capture("escaped-colon", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uFFFF:0}";
        let observed = normalize(input);
        capture("escaped-bmp-noncharacter", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\u{10400}:0}";
        let observed = normalize(input);
        capture("braced-escape", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\xC0:0}";
        let observed = normalize(input);
        capture("x-escape", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\U00010400:0}";
        let observed = normalize(input);
        capture("uppercase-u-marker", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\u00G0:0}";
        let observed = normalize(input);
        capture("nonhex-digit", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\u00\u{ff26}0:0}";
        let observed = normalize(input);
        capture("nonascii-hex-digit", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\u12";
        let observed = normalize(input);
        capture("truncated-hex", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uD800:0}";
        let observed = normalize(input);
        capture("lone-high", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uDC00:0}";
        let observed = normalize(input);
        capture("lone-low", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uD800\\uD800:0}";
        let observed = normalize(input);
        capture("two-highs", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uDC00\\uD800:0}";
        let observed = normalize(input);
        capture("reversed-pair", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uD801\\u0041:0}";
        let observed = normalize(input);
        capture("high-followed-bmp", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uD801\\uDC0Z:0}";
        let observed = normalize(input);
        capture("pair-bad-second-hex", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uD801\\xDC00:0}";
        let observed = normalize(input);
        capture("pair-wrong-second-marker", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\uD83D\\uDE00:0}";
        let observed = normalize(input);
        capture("wellformed-pair-excluded-emoji", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\"\u{e9}\":0,a\u{1f600}:1}";
        let observed = normalize(input);
        capture("multibyte-prefix-excluded", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 9
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 9 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 9"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\"\u{e9}\":0,a\\u00G0:1}";
        let observed = normalize(input);
        capture("multibyte-prefix-invalid-escape", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 9
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 9 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 9"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{a\\uDC00:0}";
        let observed = normalize(input);
        capture("low-at-continuation", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 2
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 2 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 2"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "\u{feff}/*\u{96ea}*/{\u{2118}:0}";
        let observed = normalize(input);
        capture("bom-comment-offset", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 11
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 11 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 11"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{11de0}:0}";
        let observed = normalize(input);
        capture("supplementary-nd-start", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\u{e0100}:0}";
        let observed = normalize(input);
        capture("supplementary-mn-start", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{a\u{200b}:0}";
        let observed = normalize(input);
        capture("excluded-zero-width-space", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnsupportedIdentifier,
                offset: 2
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnsupportedIdentifier, offset: 2 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{\\:0}";
        let observed = normalize(input);
        capture("bare-backslash", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::InvalidEscape,
                offset: 1
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: InvalidEscape, offset: 1 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax InvalidEscape at original UTF-8 byte 1"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{a-b:0}";
        let observed = normalize(input);
        capture("ascii-punctuation-after-key", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::UnexpectedToken,
                offset: 2
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "Syntax { kind: UnexpectedToken, offset: 2 }"
        );
        assert_eq!(
            error.to_string(),
            "JSON5 syntax UnexpectedToken at original UTF-8 byte 2"
        );
        assert!(error.source().is_none());
    }
}

#[test]
fn frozen_identifier_scalar_membership_and_char_domain() {
    for (point, start, continuation, valid) in [
        (0x24, true, true, true),
        (0x5F, true, true, true),
        (0x41, true, true, true),
        (0x61, true, true, true),
        (0x30, false, true, true),
        (0xC0, true, true, true),
        (0x3B1, true, true, true),
        (0x1C5, true, true, true),
        (0x2B0, true, true, true),
        (0x96EA, true, true, true),
        (0x2160, true, true, true),
        (0x301, false, true, true),
        (0x903, false, true, true),
        (0x661, false, true, true),
        (0x203F, false, true, true),
        (0x200C, false, true, true),
        (0x200D, false, true, true),
        (0x10400, true, true, true),
        (0x1E6C0, true, true, true),
        (0x1C89, true, true, true),
        (0x11DE0, false, true, true),
        (0xFE0F, false, true, true),
        (0xE0100, false, true, true),
        (0x2118, false, false, true),
        (0x309B, false, false, true),
        (0xB7, false, false, true),
        (0x20DD, false, false, true),
        (0x1F600, false, false, true),
        (0x61C, false, false, true),
        (0x200B, false, false, true),
        (0xE000, false, false, true),
        (0x378, false, false, true),
        (0xFFFF, false, false, true),
        (0x10FFFF, false, false, true),
        (0x20, false, false, true),
        (0x3A, false, false, true),
        (0x5C, false, false, true),
        (0xD800, false, false, false),
        (0xDC00, false, false, false),
        (0x110000, false, false, false),
    ] {
        let value = char::from_u32(point);
        eprintln!("U+{point:06X}: original char-domain result={value:?}");
        assert_eq!(value.is_some(), valid);
        if let Some(value) = value {
            let observed = (
                identifiers::is_identifier_start(value),
                identifiers::is_identifier_continue(value),
            );
            eprintln!("complete membership={observed:?}");
            assert_eq!(observed, (start, continuation));
        } else {
            // No Rust lookup call is possible for the three non-scalar integers.
            assert_eq!((start, continuation), (false, false));
        }
    }
}

#[test]
fn frozen_scaled_budget_conservation() {
    {
        let input = "null";
        let raw = normalize_limited(
            input,
            Limits {
                original: 67108864,
                normalized: 67108864,
                depth: 127,
                work: 36,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("ascii-null-work-exact", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("null".as_bytes())
        );
    }
    {
        let input = "null";
        let raw = normalize_limited(
            input,
            Limits {
                original: 67108864,
                normalized: 67108864,
                depth: 127,
                work: 35,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("ascii-null-work-one-below", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Limit {
                resource: Json5SyntaxResource::SyntaxWork,
                offset: 4,
                requested: 36,
                max: 35
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            error.to_string(),
            "JSON5 SyntaxWork limit at original UTF-8 byte 4: requested 36, max 35"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "\u{feff}null";
        let raw = normalize_limited(
            input,
            Limits {
                original: 7,
                normalized: 67108864,
                depth: 127,
                work: 536870912,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("original-bom-exact", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("null".as_bytes())
        );
    }
    {
        let input = "\u{feff}null ";
        let raw = normalize_limited(
            input,
            Limits {
                original: 7,
                normalized: 67108864,
                depth: 127,
                work: 536870912,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("original-one-over", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Limit {
                resource: Json5SyntaxResource::OriginalDocumentBytes,
                offset: 0,
                requested: 8,
                max: 7
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            error.to_string(),
            "JSON5 OriginalDocumentBytes limit at original UTF-8 byte 0: requested 8, max 7"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "{n:'\\0'}";
        let raw = normalize_limited(
            input,
            Limits {
                original: 67108864,
                normalized: 14,
                depth: 127,
                work: 536870912,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("normalized-escape-exact", input, &observed);
        assert_eq!(
            observed.as_deref().map(str::as_bytes),
            Ok("{\"n\":\"\\u0000\"}".as_bytes())
        );
    }
    {
        let input = "{n:'\\0'}";
        let raw = normalize_limited(
            input,
            Limits {
                original: 67108864,
                normalized: 13,
                depth: 127,
                work: 536870912,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("normalized-escape-one-over", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Limit {
                resource: Json5SyntaxResource::NormalizedDocumentBytes,
                offset: 8,
                requested: 14,
                max: 13
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            error.to_string(),
            "JSON5 NormalizedDocumentBytes limit at original UTF-8 byte 8: requested 14, max 13"
        );
        assert!(error.source().is_none());
    }
    {
        let input = "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[0]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]";
        let raw = normalize_limited(
            input,
            Limits {
                original: 67108864,
                normalized: 67108864,
                depth: 127,
                work: 536870912,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("depth-exact", input, &observed);
        assert_eq!(observed.as_deref().map(str::as_bytes), Ok("[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[0]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]".as_bytes()));
    }
    {
        let input = "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[0]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]";
        let raw = normalize_limited(
            input,
            Limits {
                original: 67108864,
                normalized: 67108864,
                depth: 127,
                work: 536870912,
            },
        );
        eprintln!(
            "complete limited result={:?}",
            raw.as_ref().map(|value| (&value.output, value.work))
        );
        let observed = raw.map(|value| value.output);
        capture("depth-one-over", input, &observed);
        assert_eq!(
            observed,
            Err(Json5SyntaxError::Limit {
                resource: Json5SyntaxResource::ContainerDepth,
                offset: 127,
                requested: 128,
                max: 127
            })
        );
        let error = observed.as_ref().unwrap_err();
        assert_eq!(
            error.to_string(),
            "JSON5 ContainerDepth limit at original UTF-8 byte 127: requested 128, max 127"
        );
        assert!(error.source().is_none());
    }
}
