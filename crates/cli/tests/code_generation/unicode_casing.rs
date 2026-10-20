use super::*;

fn casing_project() -> Project {
    let mut nodes = BTreeMap::new();
    nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Text".into()],
            frame: None,
        },
    );
    for (id, function) in [(2, "upper"), (3, "lower")] {
        nodes.insert(
            id,
            Node::Call {
                function: function.into(),
                args: vec![1],
            },
        );
    }
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Cases", vec![string("Text")]).repeating()],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::group("Cases", vec![string("Upper"), string("Lower")]).repeating()],
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
                        target_field: "Upper".into(),
                        node: 2,
                    },
                    Binding {
                        target_field: "Lower".into(),
                        node: 3,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn generated_unicode_casing_matches_interpreter_in_rust_and_csharp() -> TestResult<()> {
    let project = casing_project();
    assert!(engine::validate(&project).is_empty());
    // Independently authored (input, upper, lower) vectors: length-changing
    // expansions, Final_Sigma context, titlecase digraphs, language-neutral
    // dotted/dotless I and supplementary scalars, including Unicode 17 letters.
    let cases = [
        ("straße", "STRASSE", "straße"),
        ("ẞ", "ẞ", "ß"),
        ("ﬁ ŉ ǰ", "FI \u{2BC}N J\u{30C}", "ﬁ ŉ ǰ"),
        ("ΐ ᾳ", "\u{399}\u{308}\u{301} ΑΙ", "ΐ ᾳ"),
        ("ǅ", "Ǆ", "ǆ"),
        ("İ ı I i", "İ I I I", "i\u{307} ı i i"),
        ("ΟΔΟΣ ΣΑ", "ΟΔΟΣ ΣΑ", "οδος σα"),
        ("AΣ'B A'Σ", "AΣ'B A'Σ", "aσ'b a'ς"),
        ("ʰΣ AʰΣ", "ʰΣ AʰΣ", "ʰσ aʰς"),
        ("𐐀Σ ΑΣ𐐨", "𐐀Σ ΑΣ𐐀", "𐐨ς ασ𐐨"),
        (
            "\u{16EBB}\u{A7CE}",
            "\u{16EA0}\u{A7CE}",
            "\u{16EBB}\u{A7CF}",
        ),
        ("\u{212A}µ🙂", "\u{212A}Μ🙂", "kµ🙂"),
    ];
    let input = serde_json::json!({
        "Cases": cases.iter().map(|&(text, _, _)| serde_json::json!({"Text": text})).collect::<Vec<_>>()
    })
    .to_string();
    let source = format_json::from_str(&input, &project.source)?;
    let output = engine::run(&project, &source)?;
    let expected: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &output)?)?;
    let rows = expected["Cases"].as_array().ok_or("expected output rows")?;
    assert_eq!(rows.len(), cases.len());
    for (row, &(text, upper, lower)) in rows.iter().zip(&cases) {
        assert_eq!(row["Upper"], upper, "interpreter upper({text:?})");
        assert_eq!(row["Lower"], lower, "interpreter lower({text:?})");
    }
    super::json_text_boundaries::run_generated_boundary_cases(
        &project,
        &[serde_json::json!({"input": input, "expected": expected})],
        "unicode_casing",
    )
}
