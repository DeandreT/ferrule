//! Self-authored wire fixtures shared by native and generated-code tests.
pub struct Case {
    pub name: &'static str,
    pub wire: String,
    pub codes: [&'static str; 2],
    pub values: [&'static str; 2],
}

pub fn cases() -> Vec<Case> {
    let default = "UNB+UNOC:3+S+R+261026:1200+A'UNH+MA+ORDERS:D:96A:UN'BGM+AA:123+D??UNA;|.! $?:?+?''UNT+3+MA'UNZ+1+A'";
    let custom = "UNA;|.! $UNB|UNOC;3|S|R|261026;1200|B$UNH|MB|ORDERS;D;96A;UN$BGM|AA;123|V!!UNA:+.?*'!|!$$UNT|3|MB$UNZ|1|B$";
    let other = "UNA^;.# ~UNB;UNOC^3;S;R;261026^1200;C~UNH;MC;ORDERS^D^96A^UN~BGM;AA^123;W##UNA:+.?*'|$#;#~~UNT;3;MC~UNZ;1;C~";
    let d = "D?UNA;|.! $:+'";
    let c = "V!UNA:+.?*'|$";
    let o = "W#UNA:+.?*'|$;~";
    [
        (
            "default-default",
            default,
            default,
            ["AA:123", "AA:123"],
            [d, d],
            "",
        ),
        (
            "same-una",
            custom,
            custom,
            ["AA;123", "AA;123"],
            [c, c],
            "\r\n \t",
        ),
        (
            "different-una",
            custom,
            other,
            ["AA;123", "AA^123"],
            [c, o],
            "\r\n \t",
        ),
        (
            "custom-default",
            custom,
            default,
            ["AA;123", "AA:123"],
            [c, d],
            "\r\n \t",
        ),
        (
            "default-custom",
            default,
            custom,
            ["AA:123", "AA;123"],
            [d, c],
            "",
        ),
    ]
    .into_iter()
    .map(|(name, first, second, codes, values, whitespace)| Case {
        name,
        wire: format!(" \t{first}{whitespace}{second}\r\n"),
        codes,
        values,
    })
    .collect()
}
