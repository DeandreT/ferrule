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
    ];
    let input = serde_json::json!({"Cases": cases.into_iter().map(|(text, pattern, flags, replacement)| {
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
    super::json_text_boundaries::run_generated_boundary_cases(
        &project,
        &[serde_json::json!({"input": input, "expected": expected})],
        "regex_unicode_scalars",
    )
}
