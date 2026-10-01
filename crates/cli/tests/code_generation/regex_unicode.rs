#[path = "regex_unicode/binary_cases.rs"]
mod binary_cases;
#[path = "regex_unicode/crlf_cases.rs"]
mod crlf_cases;
#[path = "regex_unicode/grapheme_cases.rs"]
mod grapheme_cases;
#[path = "regex_unicode/script_cases.rs"]
mod script_cases;
#[path = "regex_unicode/ungreedy_cases.rs"]
mod ungreedy_cases;

use super::*;
use mapping::SequenceExpr;

fn regex_project() -> Project {
    let mut nodes = BTreeMap::new();
    for (id, field) in [
        (1, "Text"),
        (2, "Pattern"),
        (3, "Flags"),
        (4, "Replacement"),
    ] {
        nodes.insert(
            id,
            Node::SourceField {
                path: vec![field.into()],
                frame: None,
            },
        );
    }
    nodes.insert(
        5,
        Node::Call {
            function: "matches".into(),
            args: vec![1, 2, 3],
        },
    );
    nodes.insert(
        6,
        Node::Call {
            function: "replace".into(),
            args: vec![1, 2, 4, 3],
        },
    );
    nodes.insert(
        7,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Cases",
                    vec![
                        string("Text"),
                        string("Pattern"),
                        string("Flags"),
                        string("Replacement"),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Cases",
                    vec![
                        bool_("Match"),
                        string("Replaced"),
                        SchemaNode::group("Tokens", vec![string("Value")]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Cases".into(),
                iteration: ScopeIteration::Source(vec!["Cases".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Match".into(),
                        node: 5,
                    },
                    Binding {
                        target_field: "Replaced".into(),
                        node: 6,
                    },
                ],
                children: vec![Scope {
                    target_field: "Tokens".into(),
                    iteration: ScopeIteration::Sequence(SequenceExpr::TokenizeRegex {
                        input: 1,
                        pattern: 2,
                        flags: Some(3),
                        item: 7,
                    }),
                    bindings: vec![Binding {
                        target_field: "Value".into(),
                        node: 7,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn generated_regex_unicode_scalars_match_interpreter_in_rust_and_csharp() -> TestResult<()> {
    let project = regex_project();
    assert!(engine::validate(&project).is_empty());
    // These ordinary scalar atoms must never split a UTF-16 surrogate pair.
    // One saved mapping exercises matching, replacement captures, and private
    // generated-sequence items through both public JSON entry points.
    let cases = [
        ("🙂", "^.$", "", "x"),
        ("🙂", "^[^a]$", "", "x"),
        ("🙂🙂", "^🙂{2}$", "", "x"),
        ("🙂", ".", "", "x"),
        ("🙂", "(.)", "", "[$1]"),
        ("a🙂b", "[🙂]", "", "x"),
        ("🙂", ".", "", "x"),
        ("a🙂é", "(.)", "", "[$1]"),
        ("a🙂b", "[^🙂]", "", "x"),
        ("a🙂b", "[^a-b]+", "", "x"),
        ("😀🙂🙏", "[😀-🙏]+", "", "x"),
        ("🙂z", "[a-🙏]+", "", "x"),
        ("𐐨", "^𐐀$", "i", "x"),
        ("𐐨", "^[𐐀]$", "i", "x"),
        ("𐐨", "^(?i:𐐀)$", "", "x"),
        ("𐐀", r"^\p{L}$", "", "x"),
        ("🙂", r"^\P{L}$", "", "x"),
        ("𐐀é", r"\w+", "", "x"),
        ("🙂\né", "^.$", "m", "x"),
        ("🙂\né", ".", "s", "x"),
        ("🙂", " ^ ( . ) $ # scalar capture", "x", "[$1]"),
        ("🙂🙂a", "(.+?)", "", "[$1]"),
        ("🙂🙂a", "(.)+", "", "[$1]"),
        ("🙂a🙂", "(🙂|a)+", "", "[$1]"),
        ("a🙂b🙂c", "((🙂))", "", "$2:$1"),
        ("🙂\n", "^.$", "", "x"),
        ("é🙂", "(?:é|🙂)+", "", "x"),
        ("ab", "^a\u{a0}b$", "x", "x"),
        ("\u{a0}", "^[a\u{a0}]$", "x", "x"),
        ("m", "^[a # range comment\n - z]$", "x", "x"),
        ("#", "^[a # range comment\n b]$", "x", "x"),
        (" ", r"^[a\ \#]$", "x", "x"),
        ("🙂", "^[ ^ a ]$", "x", "x"),
        ("y ", "x[a ]|(?x:y[a ])", "", "x"),
        ("x ", "(?x:y[a ])|x[a ]", "", "x"),
        ("\u{a7cf}", "^\u{a7ce}$", "i", "x"),
        ("\u{16ebb}", "^\u{16ea0}$", "i", "x"),
        ("Ⓐ", r"^\w$", "", "x"),
        ("🄰", r"^\w$", "", "x"),
        ("🅐", r"^\w$", "", "x"),
        ("🅰", r"^\W$", "", "x"),
        ("ab", "(?<first>a)(b)", "", "$1-$2"),
        ("ab", "(?P<first>a)(b)", "", "$1-$2"),
        ("abc", "^(?<outer>a(?<inner>b))(c)$", "", "$0|$1|$2|$3"),
        ("ab", "^(?<a.b>a)(b)$", "", "$1-$2"),
        ("a🙂", "^(?P<𐐀>a)(.)$", "", "$1|$2"),
        ("🙂", r"^\x{1F642}$", "", "x"),
        ("🙂🙂", r"^\u{1F642}{2}$", "", "x"),
        ("m", r"^[\x{61}-\u{7A}]$", "", "x"),
        ("🙂", r"^[\U0001F600-\U0001F64F]$", "", "x"),
        ("a}", "^a}$", "", "x"),
        ("aa", "^a{ 2 }$", "", "x"),
        ("aaaaaaaaaaaa", "^a{1 2}$", "x", "x"),
        ("a{word}", r"^a\{word\}$", "", "x"),
    ];
    let mut class_cases = vec![
        ("a", "^[a-c&&b-d]$", "", "x", false),
        ("b", "^[a-c&&b-d]$", "", "x", true),
        ("d", "^[a-c&&b-d]$", "", "x", false),
        ("&", "^[a-c&&b-d]$", "", "x", false),
        ("abcd&~🙂🙃🙏", "([a-c&&b-d])", "", "[$1]", true),
        ("a", "^[a-z--[aeiou]]$", "", "x", false),
        ("b", "^[a-z--[aeiou]]$", "", "x", true),
        ("-", "^[a-z--[aeiou]]$", "", "x", false),
        ("a", "^[a-c~~b-d]$", "", "x", true),
        ("b", "^[a-c~~b-d]$", "", "x", false),
        ("d", "^[a-c~~b-d]$", "", "x", true),
        ("acz", "^[[a-c][x-z]]+$", "", "x", true),
        ("b", "^[^a-c&&b-d]$", "", "x", false),
        ("🙂", "^[^a-c&&b-d]$", "", "x", true),
        ("c", "^[ab~~bc&&cd]$", "", "x", true),
        ("a", "^[ab~~bc&&cd]$", "", "x", false),
        ("B", "^[a-c&&B-D]$", "i", "x", true),
        ("a", "^[a--A]$", "i", "x", false),
        ("A", "^[a&&A]$", "i", "x", true),
        ("🙃", "^[🙂-🙏&&🙃]$", "", "x", true),
        ("🙂", "^[🙂🙃--🙂]$", "", "x", false),
        ("a🙂🙃🙏b", "([🙂🙃~~🙃🙏])", "", "[$1]", true),
        ("𐐨", r"^[\p{Lu}&&[𐐀-𐐧]]$", "i", "x", true),
        ("𐐨", r"^[\P{Lu}&&[𐐀-𐐧]]$", "i", "x", false),
        ("é", r"^[\w&&[^a-z]]$", "", "x", true),
        ("ſ", r"^[\w&&[^a-z]]$", "i", "x", false),
        ("b", "(?x)^[a-c # left\n && [b-d]]$", "", "x", true),
        ("&", "(?x)^[a& &b]$", "", "x", true),
        ("a", "[a&&]", "", "x", false),
        ("a", "[a~~]", "", "x", true),
        ("-", "^[--a]$", "", "x", true),
        ("K", "^[[:alpha:]]$", "i", "x", true),
        ("é", "^[[:alpha:]]$", "i", "x", false),
        ("f", "^[[:foobar:]]$", "", "x", true),
        ("z", "^[[:foobar:]]$", "", "x", false),
    ];
    for (pattern, included, excluded) in [
        ("^[[:alnum:]]$", "9", "_"),
        ("^[[:alpha:]]$", "z", "é"),
        ("^[[:ascii:]]$", "\u{7f}", "\u{80}"),
        ("^[[:blank:]]$", "\t", "\n"),
        ("^[[:cntrl:]]$", "\0", " "),
        ("^[[:digit:]]$", "9", "٠"),
        ("^[[:graph:]]$", "~", " "),
        ("^[[:lower:]]$", "a", "A"),
        ("^[[:print:]]$", " ", "\t"),
        ("^[[:punct:]]$", "[", "z"),
        ("^[[:space:]]$", "\r", "\u{85}"),
        ("^[[:upper:]]$", "A", "a"),
        ("^[[:word:]]$", "_", "é"),
        ("^[[:xdigit:]]$", "F", "G"),
    ] {
        class_cases.push((included, pattern, "", "x", true));
        class_cases.push((excluded, pattern, "", "x", false));
    }
    let boundary_cases = [
        ("𐐀", r"^\b𐐀\b$", "", "[$0]", true),
        ("a𐐀", r"^a\b𐐀$", "", "x", false),
        ("a𐐀", r"^a\B𐐀$", "", "x", true),
        ("🙂", r"^\B🙂\B$", "", "x", true),
        ("🙂", r"^\b🙂\b$", "", "x", false),
        ("Ⓐ", r"^\bⒶ\b$", "", "x", true),
        ("🅰", r"^\b🅰\b$", "", "x", true),
        ("\u{301}", r"^\b\w\b$", "", "x", true),
        ("\u{20dd}", r"^\b\w\b$", "", "x", true),
        ("\u{200c}", r"^\b\w\b$", "", "x", true),
        ("١", r"^\b\w\b$", "", "x", true),
        ("𐐀", r"^\b\b(𐐀)\b\b$", "", "[$1]", true),
        ("𐐀", r"^(?:\b)+(𐐀)\b$", "", "[$1]", true),
        ("\n𐐀\n", r"^\b𐐀\b$", "m", "x", true),
        ("𐐨", r"^\b𐐀\b$", "i", "x", true),
        ("b", r"^\b[a-z&&[^aeiou]]\b$", "", "x", true),
        ("a", r"^\b[a-z&&[^aeiou]]\b$", "", "x", false),
        (
            "a",
            " ^ \\b [ [:alpha:] # comment\n ] \\b $ # eof",
            "x",
            "x",
            true,
        ),
        ("a𐐀", r"^a\b𐐀|𐐀$", "", "[$0]", true),
        ("a𐐀 a𐐀", r"(?:(a\b𐐀)|(a\B𐐀))", "", "$1|$2", true),
        ("aaa", r"\b(a+)(a*)", "", "$1|$2", true),
        ("aaa", r"\b(a+?)(a*)", "", "$1|$2", true),
        ("ab a", r"\b(?<first>a)(b)?", "", "$1/$2", true),
        ("abc", r"\b(?<first>a)(b)(?P<third>c)", "", "$1:$2:$3", true),
        ("aba", r"\b(a(b)?)+", "", "$1|$2", true),
        ("a", r"\ba*", "", "[$0]", true),
        ("aaa", r"\b((a*)*)", "", "[$1]", true),
        ("b", r"\b((a?)*)b\b", "", "$1-$2-$0", true),
        ("x🙂𐐀🙂z", r"🙂\b𐐀\b🙂", "", "x", true),
        ("left 𐐀 right", r"\b𐐀\b", "", "x", true),
        (r"\b", r"^\\b$", "", "x", true),
        ("a", "a # \\b", "x", "x", true),
    ];
    let property_cases = [
        ("a", r"^\p{Letter}$", "", "x", true),
        ("🙂", r"^\p{Letter}$", "", "x", false),
        ("A", r"^\p{Uppercase_Letter}$", "", "x", true),
        ("a", r"^\p{Uppercase_Letter}$", "", "x", false),
        ("é", r"^\p{Lowercase_Letter}$", "", "x", true),
        ("ǅ", r"^\p{LC}$", "", "x", true),
        ("漢", r"^\p{LC}$", "", "x", false),
        ("𐐀", r"^\p{gc=Lu}$", "", "x", true),
        (
            "𐐨",
            r"^\p{General_Category:Uppercase_Letter}$",
            "i",
            "x",
            true,
        ),
        ("a", r"^\p{gc!=Lu}$", "", "x", true),
        ("A", r"^\p{gc!=Lu}$", "", "x", false),
        ("A", r"^\P{gc!=Lu}$", "", "x", true),
        ("a", r"^\P{gc!=Lu}$", "i", "x", true),
        ("a", r"^\pL$", "", "x", true),
        ("🙂", r"^\PL$", "", "x", true),
        ("a", r"^\p{ L e t t e r }$", "", "x", true),
        ("a", r"^\p{Is_Letter}$", "", "x", true),
        ("a", r"^\p{Léetter}$", "", "x", true),
        ("a", r"^\p{g c = l u}$", "i", "x", true),
        ("a", "^\\p # query\n L$", "x", "x", true),
        ("a", "^\\p{ g c # query\n = L e t t e r }$", "x", "x", true),
        ("_", r"^\p{gc=Connector_Punctuation}$", "", "x", true),
        ("-", r"^\p{gc:Dash_Punctuation}$", "", "x", true),
        ("0", r"^\p{Decimal_Number}$", "", "x", true),
        ("٠", r"^\p{digit}$", "", "x", true),
        ("\u{e000}", r"^\p{IsPrivateUse}$", "", "x", true),
        ("\u{f0000}", r"^\p{IsPrivateUse}$", "", "x", true),
        ("\u{100000}", r"^\p{Private_Use}$", "", "x", true),
        ("\u{fdd0}", r"^\p{Assigned}$", "", "x", false),
        ("\u{378}", r"^\p{Assigned}$", "", "x", false),
        ("\u{378}", r"^\p{Any}$", "", "x", true),
        ("🙂", r"^\p{ASCII}$", "", "x", false),
        ("\0", r"^\p{ASCII}$", "", "x", true),
        ("\u{378}", r"^\P{Assigned}$", "", "x", true),
        ("\u{f0000}", r"^\p{Assigned}$", "", "x", true),
        ("aBc🅰", r"(\p{gc=Letter})", "i", "[$1]", true),
        ("aBc🅰", r"(\P{gc!=Letter})", "", "[$1]", true),
        ("a🙂𐐀b", r"(\p{LC})", "", "[$1]", true),
        ("🙂a🙃b🙏", r"[[:alpha:]&&\p{gc:Letter}]", "", "x", true),
        ("𐐀", r"^\b\p{General_Category=Letter}\b$", "", "x", true),
        ("\u{f0000}", r"^\B\p{IsPrivateUse}\B$", "", "x", true),
        ("\u{301}", r"^\b\pM\b$", "", "x", true),
        ("F9", r"^[\p{ASCII}&&[A-F0-9]]+$", "", "x", true),
        ("é", r"^[\p{ASCII}&&\p{Letter}]$", "", "x", false),
        ("𐐨", r"^\P{gc!=Lu}$", "i", "x", true),
        ("漢", r"^[\p{L}--\p{LC}]$", "", "x", true),
        ("π", r"^\p{lowercaseletter}$", "", "x", true),
        ("🅰", r"^\p{gc=Other_Symbol}$", "", "x", true),
        ("\0", r"^\p{gc=Control}$", "", "x", true),
        ("\u{a0}", r"^\p{Space_Separator}$", "", "x", true),
        ("\n", r"^\p{gc=Line_Separator}$", "", "x", false),
        ("\u{2028}", r"^\p{Line_Separator}$", "", "x", true),
    ];
    let repetition_cases = [
        ("aaaaaa", "^a{2}{3}$", "", "$0", true),
        ("aaaa", "^a{2}{3}$", "", "$0", false),
        ("aaaaa", "^a{2,3}{2}$", "", "$0", true),
        ("aaaaaaa", "^a{2,3}{2}$", "", "$0", false),
        ("aaab", "^a++b$", "", "$0", true),
        ("aaab", "^a**b$", "", "$0", true),
        ("b", "^a**b$", "", "$0", true),
        ("aab", "^a{2}??b$", "", "$0", true),
        ("b", "^a{2}??b$", "", "$0", true),
        ("aaaab", "^a{2}?+b$", "", "$0", true),
        ("aab", "^a+?+b$", "", "$0", true),
        ("aaab", "^a?+b$", "", "$0", true),
        ("aaaaaab", "^a{2}{3}{1}b$", "", "$0", true),
        ("aaaaaa", "^(a){2}{3}$", "", "$1", true),
        ("abababab", "^(?<first>a)(b){2}{3}$", "", "$1/$2", false),
        ("aaaaaab", "^(?<first>a){2}{3}(b)$", "", "$1/$2", true),
        ("🙂🙂🙂🙂a", "^(🙂){2}{2}(a)$", "", "$1/$2", true),
        ("𐐨𐐨𐐨𐐨b", "^(𐐀){2}{2}(b)$", "i", "$1/$2", true),
        ("aaaaab", "^(?:a|aa)+?+b$", "", "$0", true),
        ("aaab", "^(a|aa)+?+b$", "", "$1", true),
        ("aaaaab", "^(?:a|aa){1,2}{2}b$", "", "$0", true),
        ("aab", "^(?:){2}{3}a{2}b$", "", "$0", true),
        ("aaaaaab", "^[a]{2}{3}b$", "", "$0", true),
        ("aaaaaab", "^\\p{Letter}{2}{3}b$", "", "$0", true),
        ("aaaaaab", "^a{ 2 } { 3 }b$", "x", "$0", true),
        (
            "aaaaaab",
            "^a{2} # between operators\n {3}b$",
            "x",
            "$0",
            true,
        ),
        ("aaaaaab", "^(?i:a){2}{3}b$", "", "$0", true),
        ("aaaaaab", "^((?:a)){2}{3}(b)$", "", "$1/$2", true),
        ("aaaaaa", "^(?P<first>a){2}{3}$", "", "$1", true),
        ("aaab", "^(a*){1,2}{2}b$", "", "$1", true),
        ("b", "^a{2147483647}{0}b$", "", "$0", true),
        ("aaab", "^a{0,2}{1,2}b$", "", "$0", true),
        ("aaaaaa", "^a{2}{3}\\b$", "", "$0", true),
        ("aaaa", "^a{2}{3}\\b$", "", "$0", false),
        ("aaaab", "^a{2}?+b\\b$", "", "$0", true),
        ("b", "^a**b\\b$", "", "$0", true),
        ("aaaaaab", "^(?<first>a){2}{3}(b)\\b$", "", "$1/$2", true),
        ("🙂🙂🙂🙂", "^(🙂){2}{2}\\B$", "", "$1", true),
        ("𐐨𐐨𐐨𐐨b", "^(𐐀){2}{2}(b)\\b$", "i", "$1/$2", true),
        ("aaab", "^(a|aa)+?+b\\b$", "", "$1", true),
        ("aaab", "^(a*){1,2}{2}b\\b$", "", "$1", true),
        ("b", "^a{2147483647}{0}b\\b$", "", "$0", true),
        (
            "aaaaaab",
            "^a{2} # between operators\n {3}b\\b$",
            "x",
            "$0",
            true,
        ),
        ("aaab", "^\\b{1}{2}a++b\\b$", "", "$0", true),
        ("aaaaaab", "^[[:alpha:]]{2}{3}b\\b$", "", "$0", true),
        ("aaaaaab", "^\\p{gc:Letter}{2}{3}b\\b$", "", "$0", true),
        ("aaaaaab", "^(a{1,2}??)+(b)$", "", "[$0:$1:$2]", true),
        ("aaaaaab", "^(a{1,2}??)+(b)\\b$", "", "[$0:$1:$2]", true),
        ("xaaaaaayaaaaaaz", "(a){2}{3}", "", "[$0:$1]", true),
        (
            "x aaaaaa y aaaaaa z",
            "\\b(a){2}{3}\\b",
            "",
            "[$0:$1]",
            true,
        ),
    ];
    let boundary_alias_cases = [
        ("a", "^\\<a\\>$", "", "[$0]", true),
        ("<a>", "^\\<a\\>$", "", "x", false),
        ("<", "\\<", "", "x", false),
        (">", "\\>", "", "x", false),
        ("🙂", "^\\b{start-half}(🙂)\\b{end-half}$", "", "[$1]", true),
        ("🙂", "^\\b{start}(🙂)\\b{end}$", "", "x", false),
        ("𐐨", "(?i)^\\<𐐀\\>$", "", "[$0]", true),
        ("\n𐐀\n", "(?m)^\\b{start}𐐀\\b{end}$", "", "[$0]", true),
        (
            " a🙂𐐀 ",
            "\\b{start}(?<word>\\w+)\\b{end}",
            "",
            "[$0:$1]",
            true,
        ),
        (" a🙂𐐀 ", "\\<(?<word>\\w+)\\>", "", "[$0:$1]", true),
        (
            " a🙂𐐀 ",
            "\\b{start-half}(?<word>\\w+)\\b{end-half}",
            "",
            "[$0:$1]",
            true,
        ),
        (
            "ab",
            "^\\b{start}(?<first>a)(b)\\b{end}$",
            "",
            "$1/$2",
            true,
        ),
        (
            "aaaaaa",
            "^(?<edge>\\b{start}){2}{3}(?<word>a+)\\b{end}$",
            "",
            "$1/$2",
            true,
        ),
        (
            "aaa",
            "^(?<edge>\\b{start-half})+*(?<word>a+)\\b{end}$",
            "",
            "$1/$2",
            true,
        ),
        ("aaa", "\\b{start}(a+?)(a*)\\b{end}", "", "$1/$2", true),
        ("🙂", "^\\b{start}{0}(🙂)\\b{end}{0}$", "", "[$1]", true),
        ("a", "^\\b{ 2 }{3}a\\b{end}$", "", "x", true),
        (
            "aa",
            "\\b{ s t a r t }(a+)\\b{ e n d }# eof",
            "x",
            "[$1]",
            true,
        ),
        (
            "aa",
            "\\b{# open\nsta# middle\nrt# close\n}(a+)\\b{end# terminal\n}",
            "x",
            "[$1]",
            true,
        ),
        (
            "🙂",
            "^\\b{ s t a r t - h a l f }🙂\\b{ e n d - h a l f }$",
            "x",
            "[$0]",
            true,
        ),
        (
            "a",
            "^\\b{start} # repeat\n {2}{3} a \\b{end}$",
            "x",
            "x",
            true,
        ),
        ("a", "^\\<{4294967295}{0}a\\>{0}$", "", "x", true),
        (" a a ", "\\b{start}(a)\\b{end}", "", "[$1]", true),
        (" a a ", "\\<(a)\\>", "", "[$1]", true),
        ("aa", "^(a)\\b{start}(a)$", "", "$1/$2", false),
        ("a!", "^(a)\\b{start}(!)$", "", "$1/$2", false),
        ("!a", "^(!)\\b{start}(a)$", "", "$1/$2", true),
        ("!!", "^(!)\\b{start}(!)$", "", "$1/$2", false),
        ("aa", "^(a)\\b{end}(a)$", "", "$1/$2", false),
        ("a!", "^(a)\\b{end}(!)$", "", "$1/$2", true),
        ("!a", "^(!)\\b{end}(a)$", "", "$1/$2", false),
        ("!!", "^(!)\\b{end}(!)$", "", "$1/$2", false),
        ("aa", "^(a)\\b{start-half}(a)$", "", "$1/$2", false),
        ("a!", "^(a)\\b{start-half}(!)$", "", "$1/$2", false),
        ("!a", "^(!)\\b{start-half}(a)$", "", "$1/$2", true),
        ("!!", "^(!)\\b{start-half}(!)$", "", "$1/$2", true),
        ("aa", "^(a)\\b{end-half}(a)$", "", "$1/$2", false),
        ("a!", "^(a)\\b{end-half}(!)$", "", "$1/$2", true),
        ("!a", "^(!)\\b{end-half}(a)$", "", "$1/$2", false),
        ("!!", "^(!)\\b{end-half}(!)$", "", "$1/$2", true),
        ("𐐀🙂", "^(𐐀)\\b{end}(🙂)$", "", "$1/$2", true),
        ("🙂𐐀", "^(🙂)\\b{start}(𐐀)$", "", "$1/$2", true),
        ("🙂🙂", "^(🙂)\\b{start-half}(🙂)$", "", "$1/$2", true),
        ("🙂🙂", "^(🙂)\\b{end-half}(🙂)$", "", "$1/$2", true),
        ("𐐀𐐨", "^(𐐀)\\b{start-half}(𐐨)$", "", "$1/$2", false),
        ("𐐀𐐨", "^(𐐀)\\b{end-half}(𐐨)$", "", "$1/$2", false),
        ("́!", "^(́)\\b{end}(!)$", "", "$1/$2", true),
        ("!‍", "^(!)\\b{start}(‍)$", "", "$1/$2", true),
    ];
    let mut ascii_mode_cases = vec![
        ("Aé_9🙂", r"(?-u:\w+)", "", "[$0:$1:$2:$3]", true),
        ("1٢３9", r"(?-u:\d+)", "", "[$0:$1:$2:$3]", true),
        (
            "a\t\u{b}\n\ré\u{a0} ",
            r"(?-u:\s+)",
            "",
            "[$0:$1:$2:$3]",
            true,
        ),
        ("KkK", "(?i-u:K)", "", "[$0:$1:$2:$3]", true),
        ("Ssſ", "(?i-u:S)", "", "[$0:$1:$2:$3]", true),
        ("é", "(?i-u:é)", "", "x", true),
        ("É", "(?i-u:é)", "", "x", false),
        ("𐐨", "(?i-u:𐐀)", "", "x", false),
        ("🙂", r"(?-u:\U0001F642)", "", "x", true),
        ("é", r"(?-u:\x{E9})", "", "x", true),
        ("é", r"(?-u:\u00E9)", "", "x", true),
        ("ab-c", "(?-u:[a-z-[b]c])", "", "[$0]", true),
        ("ab-c", "[a-z-[b]c](?u)", "", "[$0]", true),
        ("ab-c", "(?-u:[a-z--[b]])", "", "[$0]", true),
        (
            "aéB",
            r"(?-u:(\w+)(?u:(\w+))(\w+))",
            "",
            "[$0:$1:$2:$3]",
            true,
        ),
        ("éAé", r"(?-u:\b(\w+)\b)", "", "[$0:$1:$2:$3]", true),
        (
            "aaaaaab",
            r"(?-u:^((?:a{1,2}?)?)+(b)$)",
            "",
            "[$0:$1:$2:$3]",
            true,
        ),
        ("kK", "(?i)(?-u:K)(K)", "", "[$0]", true),
        ("é", r"(?-u:a|(?u)\w)", "", "x", true),
        ("a", "(?x-u:a # (?u)\n)", "", "x", true),
        ("a", "(?x-u:\\x{6 # hex\n 1})", "", "x", true),
        ("a", "(?u)^[[:alpha:]]$", "", "x", true),
        ("A", "(?i-u:^[a&&A]$)", "", "x", true),
        ("A", "(?i-u:^[a--A]$)", "", "x", false),
        ("K", "(?i-u:^[[:alpha:]]$)", "", "x", false),
        ("é🙂", r"(?-u:é\B🙂)", "", "$0", true),
        ("éA", r"(?-u:é\b{start}A)", "", "$0", true),
        ("Aé", r"(?-u:A\b{end}é)", "", "$0", true),
        ("🙂A", r"(?-u:🙂\<A)", "", "$0", true),
        ("A🙂", r"(?-u:A\>🙂)", "", "$0", true),
        ("éA", r"(?-u:é\b{start-half}A)", "", "$0", true),
        ("Aé", r"(?-u:A\b{end-half}é)", "", "$0", true),
        ("é", r"(?-u:\B){2}{3}é", "", "$0", true),
        ("éé", r"(?-u:\w+)", "", "x", false),
    ];
    for (pattern, included, excluded) in [
        ("(?-u:^[[:alnum:]]$)", "9", "_"),
        ("(?-u:^[[:alpha:]]$)", "z", "é"),
        ("(?-u:^[[:ascii:]]$)", "\u{7f}", "\u{80}"),
        ("(?-u:^[[:blank:]]$)", "\t", "\n"),
        ("(?-u:^[[:cntrl:]]$)", "\0", " "),
        ("(?-u:^[[:digit:]]$)", "9", "٢"),
        ("(?-u:^[[:graph:]]$)", "~", " "),
        ("(?-u:^[[:lower:]]$)", "a", "A"),
        ("(?-u:^[[:print:]]$)", " ", "\t"),
        ("(?-u:^[[:punct:]]$)", "[", "z"),
        ("(?-u:^[[:space:]]$)", "\u{b}", "\u{a0}"),
        ("(?-u:^[[:upper:]]$)", "A", "a"),
        ("(?-u:^[[:word:]]$)", "_", "é"),
        ("(?-u:^[[:xdigit:]]$)", "F", "G"),
    ] {
        ascii_mode_cases.push((included, pattern, "", "x", true));
        ascii_mode_cases.push((excluded, pattern, "", "x", false));
    }
    let nullable_capture_cases = [
        (
            "aaaaaab",
            "^(a?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "aaaaaab",
            "^(a*)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:aaaaaa:b]",
        ),
        (
            "aaaaaab",
            "^(a?)*(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "aaaaaab",
            "^(a*)*(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:aaaaaa:b]",
        ),
        (
            "aaaaaab",
            "^((?:a{1,2}?)?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "aaaaaab",
            "^((a)?)+(b)$",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[aaaaaab:a:a:b]",
        ),
        (
            "aaaaaab",
            "^(?:(a)?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "aaaaaab",
            "^((?:a|)?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        ("aac", "^(a|b?)+(c)$", "", "[$0:$1:$2]", true, "[aac:a:c]"),
        (
            "aaaaaab",
            "^(?<keep>a?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "aaaaaab",
            "^(?P<keep>a?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "🙂🙂🙂b",
            "^(🙂?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[🙂🙂🙂b:🙂:b]",
        ),
        (
            "AAAAAAb",
            "^(a?)+(b)$",
            "i",
            "[$0:$1:$2]",
            true,
            "[AAAAAAb:A:b]",
        ),
        (
            "aaaaaab",
            " ^ ( a ? ) + ( b ) $ # eof",
            "x",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "aaaaaab",
            "^(\\p{gc=Letter}*)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:aaaaaa:b]",
        ),
        (
            "aaaaaab",
            "^(a?)+?(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:a:b]",
        ),
        (
            "aaaaaab",
            "^(a*){1,2}(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab::b]",
        ),
        (
            "aaaaaab",
            "^(a+)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:aaaaaa:b]",
        ),
        (
            "aaaaaab",
            "^(?:a?)+(b)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab:b:]",
        ),
        (
            "aaaaaab",
            "^((a?)+){0}(aaaaaab)$",
            "",
            "[$0:$1:$2]",
            true,
            "[aaaaaab::]",
        ),
        (
            "xaaaaaabyabz",
            "(a?)+(b)",
            "",
            "[$0:$1:$2]",
            true,
            "x[aaaaaab:a:b]y[ab:a:b]z",
        ),
        ("cccc", "^(a?)+(b)$", "", "[$1]", false, "cccc"),
    ];
    let group_header_cases = [
        ("a", "(?x)( ?-u: a )", "", "[$0:$1:$2:$3]", true, "[a:::]"),
        (
            "a",
            "(?x)( # header comment\n ?-u: a )",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[a:::]",
        ),
        ("a", "(?x)( ?u: a )", "", "[$0:$1:$2:$3]", true, "[a:::]"),
        (
            "ab",
            "(?x)( ?<first> a )(b)(?u)",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[ab:a:b:]",
        ),
        (
            "ab",
            "(?x)( # name comment\n ?P<first> a )(b)(?u)",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[ab:a:b:]",
        ),
        ("a", "(?x)( ?: a )(?u)", "", "[$0:$1:$2:$3]", true, "[a:::]"),
        ("k", "(?ix)( ?-u)K", "", "[$0:$1:$2:$3]", true, "[k:::]"),
        ("K", "(?ix)( ?-u)K", "", "[$0:$1:$2:$3]", false, "K"),
        ("K", "(?ix)( ?u)K", "", "[$0:$1:$2:$3]", true, "[K:::]"),
        (
            "kK",
            "(?ix)( ?-u: K )(K)",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[kK:K::]",
        ),
        (
            "KK",
            "(?ix)( ?-u: ( ?u: K ) )(K)",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[KK:K::]",
        ),
        (
            " a ",
            "(?x)( ?-x: a )(?u)",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[ a :::]",
        ),
        ("a", "(?x)( ?-x: a )(?u)", "", "[$0:$1:$2:$3]", false, "a"),
        (
            "a",
            "(?x)( ?-x:(?-u:a))",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[a:::]",
        ),
        (
            "a",
            "(?x)(\u{a0}?-u: a )",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[a:::]",
        ),
        (
            "a",
            "( # header comment\n ?-u: a )",
            "x",
            "[$0:$1:$2:$3]",
            true,
            "[a:::]",
        ),
        (
            "a",
            "(?x)( # (?u) is ignored\n a )",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[a:a::]",
        ),
        (
            "(?u)",
            r"(?x)\( \?u \)",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[(?u):::]",
        ),
        ("u", "(?x)[ (?u) ]", "", "[$0:$1:$2:$3]", true, "[u:::]"),
        (
            "a",
            "(?x)( ?-x:( ?-u:a))(?u)",
            "",
            "[$0:$1:$2:$3]",
            false,
            "a",
        ),
        (
            "-u:a",
            "(?x)( ?-x:( ?-u:a))(?u)",
            "",
            "[$0:$1:$2:$3]",
            true,
            "[-u:a:-u:a::]",
        ),
    ];
    let ungreedy_cases = ungreedy_cases::CASES;
    let crlf_cases = crlf_cases::CASES;
    let script_cases = script_cases::CASES;
    let binary_cases = binary_cases::CASES;
    let grapheme_cases = grapheme_cases::CASES;
    let input = serde_json::json!({"Cases": cases.into_iter().chain(class_cases.iter().chain(&boundary_cases).chain(&property_cases).chain(&repetition_cases).chain(&boundary_alias_cases).chain(&ascii_mode_cases).map(|&(text, pattern, flags, replacement, _)| (text, pattern, flags, replacement))).chain(nullable_capture_cases.iter().map(|&(text, pattern, flags, replacement, _, _)| (text, pattern, flags, replacement))).chain(group_header_cases.iter().map(|&(text, pattern, flags, replacement, _, _)| (text, pattern, flags, replacement))).chain(ungreedy_cases.iter().map(|&(text, pattern, flags, replacement, _, _)| (text, pattern, flags, replacement))).chain(crlf_cases.iter().map(|&(text, pattern, flags, replacement, _, _)| (text, pattern, flags, replacement))).chain(script_cases.iter().map(|&(text, pattern, flags, replacement, _, _)| (text, pattern, flags, replacement))).chain(binary_cases.iter().map(|&(text, pattern, flags, replacement, _, _)| (text, pattern, flags, replacement))).chain(grapheme_cases.iter().map(|&(text, pattern, flags, replacement, _, _)| (text, pattern, flags, replacement))).map(|(text, pattern, flags, replacement)| {
        serde_json::json!({"Text": text, "Pattern": pattern, "Flags": flags, "Replacement": replacement})
    }).collect::<Vec<_>>()}).to_string();
    let source = format_json::from_str(&input, &project.source)?;
    let output = engine::run(&project, &source)?;
    let expected: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &output)?)?;
    let rows = expected["Cases"].as_array().ok_or("expected output rows")?;
    assert_eq!(rows[0]["Match"], true);
    assert_eq!(rows[1]["Match"], true);
    assert_eq!(rows[2]["Match"], true);
    assert_eq!(rows[3]["Replaced"], "x");
    assert_eq!(rows[4]["Replaced"], "[🙂]");
    assert_eq!(
        rows[5]["Tokens"],
        serde_json::json!([{"Value": "a"}, {"Value": "b"}])
    );
    assert_eq!(
        rows[6]["Tokens"],
        serde_json::json!([{"Value": ""}, {"Value": ""}])
    );
    assert_eq!(rows[12]["Match"], true);
    assert_eq!(rows[25]["Match"], false);
    for index in [27, 29, 31, 32, 34, 37, 38, 39] {
        assert_eq!(rows[index]["Match"], true, "case {index}");
    }
    for index in [28, 30, 33, 35, 36, 40] {
        assert_eq!(rows[index]["Match"], false, "case {index}");
    }
    assert_eq!(rows[41]["Replaced"], "a-b");
    assert_eq!(rows[42]["Replaced"], "a-b");
    assert_eq!(rows[43]["Replaced"], "abc|ab|b|c");
    assert_eq!(rows[44]["Replaced"], "a-b");
    assert_eq!(rows[45]["Replaced"], "a|🙂");
    for (index, row) in rows.iter().enumerate().take(54).skip(46) {
        assert_eq!(row["Match"], true, "case {index}");
    }
    assert_eq!(
        rows.len(),
        cases.len()
            + class_cases.len()
            + boundary_cases.len()
            + property_cases.len()
            + repetition_cases.len()
            + boundary_alias_cases.len()
            + ascii_mode_cases.len()
            + nullable_capture_cases.len()
            + group_header_cases.len()
            + ungreedy_cases.len()
            + crlf_cases.len()
            + script_cases.len()
            + binary_cases.len()
            + grapheme_cases.len()
    );
    for (index, (row, &(_, pattern, _, _, matches))) in
        rows.iter().skip(cases.len()).zip(&class_cases).enumerate()
    {
        assert_eq!(row["Match"], matches, "class case {index}: {pattern}");
    }
    assert_eq!(rows[58]["Replaced"], "a[b][c]d&~🙂🙃🙏");
    assert_eq!(rows[75]["Replaced"], "a[🙂]🙃[🙏]b");
    assert_eq!(
        rows[75]["Tokens"],
        serde_json::json!([{"Value": "a"}, {"Value": "🙃"}, {"Value": "b"}])
    );
    let boundary_rows = &rows[cases.len() + class_cases.len()..];
    for (index, (row, &(_, pattern, _, _, matches))) in
        boundary_rows.iter().zip(&boundary_cases).enumerate()
    {
        assert_eq!(row["Match"], matches, "boundary case {index}: {pattern}");
    }
    assert_eq!(boundary_rows[18]["Replaced"], "a[𐐀]");
    assert_eq!(boundary_rows[19]["Replaced"], "|a𐐀 |a𐐀");
    assert_eq!(boundary_rows[20]["Replaced"], "aaa|");
    assert_eq!(boundary_rows[21]["Replaced"], "a|aa");
    assert_eq!(boundary_rows[22]["Replaced"], "a/b a/");
    assert_eq!(boundary_rows[27]["Replaced"], "--b");
    assert_eq!(
        boundary_rows[28]["Tokens"],
        serde_json::json!([{"Value": "x"}, {"Value": "z"}])
    );
    let property_rows = &boundary_rows[boundary_cases.len()..];
    for (index, (row, &(_, pattern, _, _, matches))) in
        property_rows.iter().zip(&property_cases).enumerate()
    {
        assert_eq!(row["Match"], matches, "property case {index}: {pattern}");
    }
    assert_eq!(property_rows[35]["Replaced"], "[a][B][c]🅰");
    assert_eq!(property_rows[36]["Replaced"], "[a][B][c]🅰");
    assert_eq!(property_rows[37]["Replaced"], "[a]🙂[𐐀][b]");
    assert_eq!(
        property_rows[38]["Tokens"],
        serde_json::json!([{"Value": "🙂"}, {"Value": "🙃"}, {"Value": "🙏"}])
    );
    let repetition_rows = &property_rows[property_cases.len()..];
    for (index, (row, &(_, pattern, _, _, matches))) in
        repetition_rows.iter().zip(&repetition_cases).enumerate()
    {
        assert_eq!(row["Match"], matches, "repetition case {index}: {pattern}");
    }
    assert_eq!(repetition_rows[13]["Replaced"], "a");
    assert_eq!(repetition_rows[15]["Replaced"], "a/b");
    assert_eq!(repetition_rows[16]["Replaced"], "🙂/a");
    assert_eq!(repetition_rows[17]["Replaced"], "𐐨/b");
    assert_eq!(repetition_rows[36]["Replaced"], "a/b");
    assert_eq!(repetition_rows[37]["Replaced"], "🙂");
    assert_eq!(repetition_rows[38]["Replaced"], "𐐨/b");
    assert_eq!(repetition_rows[46]["Replaced"], "[aaaaaab:a:b]");
    assert_eq!(repetition_rows[47]["Replaced"], "[aaaaaab:a:b]");
    assert_eq!(repetition_rows[48]["Replaced"], "x[aaaaaa:a]y[aaaaaa:a]z");
    assert_eq!(
        repetition_rows[48]["Tokens"],
        serde_json::json!([{"Value": "x"}, {"Value": "y"}, {"Value": "z"}])
    );
    assert_eq!(
        repetition_rows[49]["Replaced"],
        "x [aaaaaa:a] y [aaaaaa:a] z"
    );
    assert_eq!(
        repetition_rows[49]["Tokens"],
        serde_json::json!([{"Value": "x "}, {"Value": " y "}, {"Value": " z"}])
    );
    let alias_rows = &repetition_rows[repetition_cases.len()..];
    for (index, (row, &(_, pattern, _, _, matches))) in
        alias_rows.iter().zip(&boundary_alias_cases).enumerate()
    {
        assert_eq!(
            row["Match"], matches,
            "boundary alias case {index}: {pattern}"
        );
    }
    assert_eq!(alias_rows[0]["Replaced"], "[a]");
    assert_eq!(alias_rows[2]["Replaced"], "<");
    assert_eq!(alias_rows[3]["Replaced"], ">");
    assert_eq!(alias_rows[4]["Replaced"], "[🙂]");
    for index in [8, 9, 10] {
        assert_eq!(alias_rows[index]["Replaced"], " [a:a]🙂[𐐀:𐐀] ");
        assert_eq!(
            alias_rows[index]["Tokens"],
            serde_json::json!([{"Value": " "}, {"Value": "🙂"}, {"Value": " "}])
        );
    }
    assert_eq!(alias_rows[11]["Replaced"], "a/b");
    assert_eq!(alias_rows[12]["Replaced"], "/aaaaaa");
    assert_eq!(alias_rows[13]["Replaced"], "/aaa");
    assert_eq!(alias_rows[14]["Replaced"], "a/aa");
    for index in [22, 23] {
        assert_eq!(alias_rows[index]["Replaced"], " [a] [a] ");
        assert_eq!(
            alias_rows[index]["Tokens"],
            serde_json::json!([{"Value": " "}, {"Value": " "}, {"Value": " "}])
        );
    }
    let ascii_rows = &alias_rows[boundary_alias_cases.len()..];
    for (index, (row, &(_, pattern, _, _, matches))) in
        ascii_rows.iter().zip(&ascii_mode_cases).enumerate()
    {
        assert_eq!(row["Match"], matches, "ASCII mode case {index}: {pattern}");
    }
    assert_eq!(ascii_rows[0]["Replaced"], "[A:::]é[_9:::]🙂");
    assert_eq!(
        ascii_rows[0]["Tokens"],
        serde_json::json!([{"Value":""},{"Value":"é"},{"Value":"🙂"}])
    );
    assert_eq!(ascii_rows[1]["Replaced"], "[1:::]٢３[9:::]");
    assert_eq!(ascii_rows[3]["Replaced"], "[K:::][k:::]K");
    assert_eq!(ascii_rows[4]["Replaced"], "[S:::][s:::]ſ");
    assert_eq!(ascii_rows[11]["Replaced"], "[a][b][-][c]");
    assert_eq!(ascii_rows[12]["Replaced"], "[a][b][-][c]");
    assert_eq!(ascii_rows[13]["Replaced"], "[a]b-[c]");
    assert_eq!(ascii_rows[14]["Replaced"], "[aéB:a:é:B]");
    assert_eq!(ascii_rows[15]["Replaced"], "é[A:A::]é");
    assert_eq!(ascii_rows[16]["Replaced"], "[aaaaaab:a:b:]");
    assert_eq!(ascii_rows[17]["Replaced"], "[kK]");
    let nullable_begin = ascii_mode_cases.len();
    let nullable_rows = &ascii_rows[nullable_begin..nullable_begin + nullable_capture_cases.len()];
    for (index, (row, &(_, pattern, _, _, matched, replaced))) in nullable_rows
        .iter()
        .zip(&nullable_capture_cases)
        .enumerate()
    {
        assert_eq!(
            row["Match"], matched,
            "nullable capture case {index}: {pattern}"
        );
        assert_eq!(
            row["Replaced"], replaced,
            "nullable capture case {index}: {pattern}"
        );
    }
    assert_eq!(
        nullable_rows[20]["Tokens"],
        serde_json::json!([{"Value": "x"}, {"Value": "y"}, {"Value": "z"}])
    );
    assert_eq!(
        nullable_rows[21]["Tokens"],
        serde_json::json!([{"Value": "cccc"}])
    );
    let group_header_begin = nullable_begin + nullable_capture_cases.len();
    let group_header_rows =
        &ascii_rows[group_header_begin..group_header_begin + group_header_cases.len()];
    for (index, (row, &(text, pattern, _, _, matched, replaced))) in group_header_rows
        .iter()
        .zip(&group_header_cases)
        .enumerate()
    {
        assert_eq!(
            row["Match"], matched,
            "group header case {index}: {pattern}"
        );
        assert_eq!(
            row["Replaced"], replaced,
            "group header case {index}: {pattern}"
        );
        let tokens = if matched {
            serde_json::json!([{"Value": ""}, {"Value": ""}])
        } else {
            serde_json::json!([{"Value": text}])
        };
        assert_eq!(
            row["Tokens"], tokens,
            "group header case {index}: {pattern}"
        );
    }
    let ungreedy_begin = group_header_begin + group_header_cases.len();
    let ungreedy_rows = &ascii_rows[ungreedy_begin..ungreedy_begin + ungreedy_cases.len()];
    for (index, (row, &(_, pattern, _, _, matched, replaced))) in
        ungreedy_rows.iter().zip(ungreedy_cases).enumerate()
    {
        assert_eq!(row["Match"], matched, "ungreedy case {index}: {pattern}");
        assert_eq!(
            row["Replaced"], replaced,
            "ungreedy case {index}: {pattern}"
        );
    }
    assert_eq!(
        ungreedy_rows[25]["Tokens"],
        serde_json::json!([{"Value": "x"}, {"Value": ""}, {"Value": ""},
            {"Value": "y"}, {"Value": ""}, {"Value": ""}, {"Value": "Z"}])
    );
    assert_eq!(
        ungreedy_rows[26]["Tokens"],
        serde_json::json!([{"Value": "x"}, {"Value": "y"}, {"Value": "Z"}])
    );
    let crlf_begin = ungreedy_begin + ungreedy_cases.len();
    let crlf_rows = &ascii_rows[crlf_begin..crlf_begin + crlf_cases.len()];
    for (index, (row, &(_, pattern, _, _, matched, replaced))) in
        crlf_rows.iter().zip(crlf_cases).enumerate()
    {
        assert_eq!(row["Match"], matched, "CRLF case {index}: {pattern}");
        assert_eq!(row["Replaced"], replaced, "CRLF case {index}: {pattern}");
    }
    assert_eq!(
        crlf_rows[33]["Tokens"],
        serde_json::json!([{"Value": ""}, {"Value": "\r\n"}, {"Value": "\r"}, {"Value": "\n"}, {"Value": ""}])
    );
    assert_eq!(
        crlf_rows[34]["Tokens"],
        serde_json::json!([{"Value": "left"}, {"Value": "middle"}, {"Value": "right"}, {"Value": "end"}])
    );
    let script_begin = crlf_begin + crlf_cases.len();
    script_cases::assert_rows(&ascii_rows[script_begin..script_begin + script_cases.len()]);
    let binary_begin = script_begin + script_cases.len();
    binary_cases::assert_rows(&ascii_rows[binary_begin..binary_begin + binary_cases.len()]);
    let grapheme_begin = binary_begin + binary_cases.len();
    grapheme_cases::assert_rows(&ascii_rows[grapheme_begin..grapheme_begin + grapheme_cases.len()]);
    super::json_text_boundaries::run_generated_boundary_cases(
        &project,
        &[serde_json::json!({"input": input, "expected": expected})],
        "regex_unicode_scalars",
    )
}
