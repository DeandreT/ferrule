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
    let input = serde_json::json!({"Cases": cases.into_iter().chain(class_cases.iter().chain(&boundary_cases).chain(&property_cases).chain(&repetition_cases).map(|&(text, pattern, flags, replacement, _)| (text, pattern, flags, replacement))).map(|(text, pattern, flags, replacement)| {
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
    super::json_text_boundaries::run_generated_boundary_cases(
        &project,
        &[serde_json::json!({"input": input, "expected": expected})],
        "regex_unicode_scalars",
    )
}
