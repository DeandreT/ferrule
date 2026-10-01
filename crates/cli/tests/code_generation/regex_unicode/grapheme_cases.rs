pub(super) const CASES: &[(&str, &str, &str, &str, bool, &str)] = &[
    ("ͅ", "(?u)(\\p{GCB=Extend})", "", "[$1]", true, "[ͅ]"),
    (
        "ͅ",
        "(?u)(\\p{Grapheme_Cluster_Break:ex})",
        "",
        "[$1]",
        true,
        "[ͅ]",
    ),
    ("ः", "(?U)(\\p{GCB=SpacingMark})", "", "[$1]", true, "[ः]"),
    ("ः", "(?u)(\\p{GCB=Extend})", "", "[$1]", false, "ः"),
    ("‍", "(?R)(\\p{GCB=ZWJ})", "", "[$1]", true, "[‍]"),
    ("‍", "(?u)(\\p{GCB=Control})", "", "[$1]", false, "‍"),
    ("\r", "(?u)(\\p{GCB=CR})", "", "[$1]", true, "[\r]"),
    ("\r", "(?u)(\\p{GCB=Control})", "", "[$1]", false, "\r"),
    ("\r", "(?u)(\\p{Control})", "", "[$1]", true, "[\r]"),
    ("\n", "(?u)(\\p{GCB=LF})", "", "[$1]", true, "[\n]"),
    (
        "\u{0}",
        "(?u)(\\p{GCB=Control})",
        "",
        "[$1]",
        true,
        "[\u{0}]",
    ),
    ("؀", "(?u)(\\p{GCB=Prepend})", "", "[$1]", true, "[؀]"),
    ("ᄀ", "(?u)(\\p{GCB=L})", "", "[$1]", true, "[ᄀ]"),
    ("ᅡ", "(?u)(\\p{GCB=V})", "", "[$1]", true, "[ᅡ]"),
    ("ᆨ", "(?u)(\\p{GCB=T})", "", "[$1]", true, "[ᆨ]"),
    ("가", "(?u)(\\p{GCB=LV})", "", "[$1]", true, "[가]"),
    ("각", "(?u)(\\p{GCB=LVT})", "", "[$1]", true, "[각]"),
    ("🇺", "(?u)(\\p{GCB=RI})", "", "[$1]", true, "[🇺]"),
    ("🙂", "(?u)(\\p{GCB=RI})", "", "[$1]", false, "🙂"),
    ("A", "(?u)(\\p{GCB!=Extend})", "", "[$1]", true, "[A]"),
    ("ͅ", "(?u)(\\P{GCB!=Extend})", "", "[$1]", true, "[ͅ]"),
    ("Ι", "(?iu)(\\p{GCB=Extend})", "", "[$1]", true, "[Ι]"),
    ("Ι", "(?iu)(\\P{GCB=Extend})", "", "[$1]", false, "Ι"),
    ("Ι", "(?iu)(\\p{GCB!=Extend})", "", "[$1]", false, "Ι"),
    (
        "ͅ",
        "(?u)([\\p{GCB=Extend}&&\\p{Alphabetic}])",
        "",
        "[$1]",
        true,
        "[ͅ]",
    ),
    (
        "̀",
        "(?u)([\\p{GCB=Extend}--\\p{Alphabetic}])",
        "",
        "[$1]",
        true,
        "[̀]",
    ),
    (
        "ः",
        "(?u)([\\p{M}~~\\p{GCB=Extend}])",
        "",
        "[$1]",
        true,
        "[ः]",
    ),
    (
        "àbͅ!",
        "(?u)(?<marks>\\p{GCB=Extend}+)",
        "",
        "[$1]",
        true,
        "a[̀]b[ͅ]!",
    ),
    (
        "a\r\nb",
        "(?R)([\\p{GCB=CR}\\p{GCB=LF}]+)",
        "",
        "[$1]",
        true,
        "a[\r\n]b",
    ),
    ("̀ͅ", "(?U)(\\p{GCB=Extend}+)", "", "[$1]", true, "[̀][ͅ]"),
    ("̀ͅ", "(?U)(\\p{GCB=Extend}+?)", "", "[$1]", true, "[̀ͅ]"),
    (
        "Aͅ",
        "(?u)(?-u:(A))|(\\p{GCB=Extend})",
        "",
        "[$1/$2]",
        true,
        "[A/][/ͅ]",
    ),
    ("ͅ", "(?R)(?-R:(\\p{GCB=Extend}))", "", "[$1]", true, "[ͅ]"),
    (
        "ः",
        "(?ux)(\\p{Gra# comment\npheme_Cluster_Break : Spacing Mark})",
        "",
        "[$1]",
        true,
        "[ः]",
    ),
    (
        "ͅ",
        "(?u)(\\p{Is_Grapheme_Cluster_Break=Is_Ex_tend})",
        "",
        "[$1]",
        true,
        "[ͅ]",
    ),
    ("ͅ", "(?u)([^\\p{GCB=Extend}])", "", "[$1]", false, "ͅ"),
    ("A", "(?u)(\\p{GCB=L})", "", "[$1]", false, "A"),
    ("A", "(?u)(\\p{L})", "", "[$1]", true, "[A]"),
    ("Ι", "(?iu)(\\P{GCB!=Extend})", "", "[$1]", true, "[Ι]"),
];

pub(super) fn assert_rows(rows: &[serde_json::Value]) {
    assert_eq!(rows.len(), CASES.len());
    for (index, (row, &(_, pattern, _, _, matched, replaced))) in rows.iter().zip(CASES).enumerate()
    {
        assert_eq!(row["Match"], matched, "GCB case {index}: {pattern}");
        assert_eq!(row["Replaced"], replaced, "GCB case {index}: {pattern}");
    }
    assert_eq!(
        rows[27]["Tokens"],
        serde_json::json!([{ "Value": "a" }, { "Value": "b" }, { "Value": "!" }])
    );
    assert_eq!(
        rows[28]["Tokens"],
        serde_json::json!([{ "Value": "a" }, { "Value": "b" }])
    );
}
