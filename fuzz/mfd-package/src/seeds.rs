//! Deterministic, self-authored seed inputs.
//!
//! `cargo run --example write_seeds` writes these to `seeds/`, and the seed
//! test fails if the checked-in files drift from this generator.

use crate::program::{Alias, Chain, Name, Op, encode};

/// Template entry indexes, in template order.
const DESIGN: u8 = 0;
const SOURCE: u8 = 1;
const TARGET: u8 = 2;
/// The included or referenced definitions of `xsd_include` and `json_ref`.
const TYPES: u8 = 2;

/// Filler size classes of [`crate::program::FILLER_SIZES`].
const SMALL: u8 = 2;
const AT_ENTRY_LIMIT: u8 = 3;
const PAST_ENTRY_LIMIT: u8 = 4;

fn pool(name: &str) -> Name<'_> {
    Name::Pool(name)
}

/// Every seed, by name.
pub fn all() -> Vec<(&'static str, Vec<u8>)> {
    let mut seeds = vec![
        ("empty", encode("empty", &[])),
        ("xml_to_json", encode("xml_to_json", &[])),
        ("json_to_xml", encode("json_to_xml", &[])),
        ("dtd_source", encode("dtd_source", &[])),
        ("nested_design", encode("nested_design", &[])),
        ("xsd_include", encode("xsd_include", &[])),
        ("json_ref", encode("json_ref", &[])),
        // Filesystem-only backends refuse in package mode.
        ("edi_idoc_source", encode("edi_idoc_source", &[])),
        (
            "edi_swift_source",
            encode(
                "edi_idoc_source",
                &[Op::Substitute(
                    DESIGN,
                    Name::Raw(b"kind=\"EDIFIXED\""),
                    Name::Raw(b"kind=\"SWIFTMT\""),
                )],
            ),
        ),
        ("db_source", encode("db_source", &[])),
        (
            "raw_minimal_design",
            encode(
                "empty",
                &[Op::Add(pool("design.mfd"), b"<mapping version=\"22\"/>")],
            ),
        ),
        (
            "raw_garbage_design",
            encode(
                "empty",
                &[Op::Add(pool("design.mfd"), b"\xef\xbb\xbf<\x00<!DOCTYPE[")],
            ),
        ),
        // Path and alias admission.
        (
            "design_path_traversal",
            encode("xml_to_json", &[Op::Design(pool("../design-source.xsd"))]),
        ),
        (
            "design_path_case_alias",
            encode("xml_to_json", &[Op::Design(pool("Design.mfd"))]),
        ),
        (
            "entry_path_absolute",
            encode("xml_to_json", &[Op::Add(pool("/etc/passwd"), b"")]),
        ),
        (
            "entry_path_drive",
            encode("xml_to_json", &[Op::Add(pool("C:/design-source.xsd"), b"")]),
        ),
        (
            "entry_path_url",
            encode(
                "xml_to_json",
                &[Op::Add(pool("https://example.invalid/types.xsd"), b"")],
            ),
        ),
        (
            "entry_path_backslash",
            encode("xml_to_json", &[Op::Add(pool("sub\\types.xsd"), b"")]),
        ),
        (
            "entry_path_empty_segment",
            encode("xml_to_json", &[Op::Add(pool("a//b"), b"")]),
        ),
        (
            "entry_path_dot_segment",
            encode("xml_to_json", &[Op::Add(pool("./design-source.xsd"), b"")]),
        ),
        (
            "entry_path_trailing_dot",
            encode("xml_to_json", &[Op::Add(pool("trailing."), b"")]),
        ),
        (
            "entry_path_trailing_space",
            encode("xml_to_json", &[Op::Add(pool("trailing "), b"")]),
        ),
        (
            "entry_path_nul",
            encode("xml_to_json", &[Op::Add(pool("nul\0name"), b"")]),
        ),
        (
            "entry_path_control",
            encode("xml_to_json", &[Op::Add(pool("tab\tname"), b"")]),
        ),
        (
            "entry_path_unicode",
            encode("xml_to_json", &[Op::Add(pool("λ/ü.xsd"), b"")]),
        ),
        (
            "entry_path_invalid_utf8",
            encode(
                "xml_to_json",
                &[Op::Add(Name::Raw(b"bad\xff.xsd"), b"")],
            ),
        ),
        (
            "entry_path_at_limit",
            encode(
                "xml_to_json",
                &[Op::Add(pool(&"p".repeat(mfd::MAX_PACKAGE_PATH_BYTES)), b"")],
            ),
        ),
        (
            "entry_path_past_limit",
            encode(
                "xml_to_json",
                &[Op::Add(
                    pool(&"p".repeat(mfd::MAX_PACKAGE_PATH_BYTES + 1)),
                    b"",
                )],
            ),
        ),
        (
            "entry_path_deep_directories",
            encode(
                "xml_to_json",
                &[Op::Add(pool(&format!("{}/x.xsd", ["d"; 127].join("/"))), b"")],
            ),
        ),
        (
            "duplicate_exact",
            encode("xml_to_json", &[Op::Alias(SOURCE, Alias::Exact)]),
        ),
        (
            "duplicate_upper_case",
            encode("xml_to_json", &[Op::Alias(SOURCE, Alias::Upper)]),
        ),
        (
            "duplicate_first_letter_case",
            encode("xml_to_json", &[Op::Alias(DESIGN, Alias::FirstLetterCase)]),
        ),
        // Entry-count and byte limits.
        (
            "entry_count_at_limit",
            encode("xml_to_json", &[Op::Filler(125, 0)]),
        ),
        (
            "entry_count_past_limit",
            encode("xml_to_json", &[Op::Filler(126, 0)]),
        ),
        (
            "entry_at_byte_limit",
            encode("xml_to_json", &[Op::Filler(1, AT_ENTRY_LIMIT)]),
        ),
        (
            "entry_past_byte_limit",
            encode("xml_to_json", &[Op::Filler(1, PAST_ENTRY_LIMIT)]),
        ),
        (
            "total_at_byte_limit",
            encode(
                "empty",
                &[
                    Op::Filler(4, AT_ENTRY_LIMIT),
                    Op::Design(pool("fill-0.bin")),
                ],
            ),
        ),
        (
            "total_past_byte_limit",
            encode("xml_to_json", &[Op::Filler(4, AT_ENTRY_LIMIT)]),
        ),
        (
            "unreferenced_entries",
            encode(
                "xml_to_json",
                &[
                    Op::Filler(8, SMALL),
                    Op::Add(pool("types.xsd"), b"\xff\xfe<"),
                ],
            ),
        ),
        // Missing direct and transitive resources.
        (
            "missing_source_schema",
            encode("xml_to_json", &[Op::Drop(SOURCE)]),
        ),
        (
            "missing_target_schema",
            encode("xml_to_json", &[Op::Drop(TARGET)]),
        ),
        (
            "missing_dtd",
            encode("dtd_source", &[Op::Drop(SOURCE)]),
        ),
        (
            "missing_nested_schema",
            encode("nested_design", &[Op::Drop(SOURCE)]),
        ),
        (
            "missing_included_xsd",
            encode("xsd_include", &[Op::Drop(TYPES)]),
        ),
        (
            "missing_referenced_json",
            encode("json_ref", &[Op::Drop(TYPES)]),
        ),
        (
            "missing_json_pointer",
            encode(
                "json_ref",
                &[Op::Substitute(
                    SOURCE,
                    pool("types.schema.json"),
                    pool("types.schema.json#/$defs/Missing"),
                )],
            ),
        ),
        (
            "missing_local_json_pointer",
            encode(
                "json_ref",
                &[Op::Substitute(
                    SOURCE,
                    Name::Raw(b"types.schema.json#/$defs/Value"),
                    pool("#/$defs/Missing"),
                )],
            ),
        ),
        (
            "nested_design_root_reference",
            encode(
                "nested_design",
                &[Op::Rename(SOURCE, pool("design-source.xsd"))],
            ),
        ),
        // References that try to leave the package.
        ref_seed("design_ref_parent", "xml_to_json", DESIGN, "../design-source.xsd"),
        ref_seed("design_ref_dot", "xml_to_json", DESIGN, "./design-source.xsd"),
        ref_seed(
            "design_ref_normalized_parent",
            "xml_to_json",
            DESIGN,
            "sub/../design-source.xsd",
        ),
        ref_seed("design_ref_absolute", "xml_to_json", DESIGN, "/etc/passwd"),
        ref_seed("design_ref_drive", "xml_to_json", DESIGN, "C:/design-source.xsd"),
        ref_seed(
            "design_ref_url",
            "xml_to_json",
            DESIGN,
            "https://example.invalid/types.xsd",
        ),
        ref_seed("design_ref_backslash", "xml_to_json", DESIGN, "sub\\types.xsd"),
        ref_seed(
            "design_ref_virtual_root",
            "xml_to_json",
            DESIGN,
            "/__ferrule_package__/design-source.xsd",
        ),
        ref_seed("design_ref_case_alias", "xml_to_json", DESIGN, "DESIGN-SOURCE.XSD"),
        ref_seed("design_ref_empty", "xml_to_json", DESIGN, ""),
        (
            "include_ref_parent",
            encode(
                "xsd_include",
                &[Op::Substitute(
                    SOURCE,
                    pool("types.xsd"),
                    pool("../design-source.xsd"),
                )],
            ),
        ),
        (
            "include_ref_absolute",
            encode(
                "xsd_include",
                &[Op::Substitute(SOURCE, pool("types.xsd"), pool("/etc/passwd"))],
            ),
        ),
        (
            "include_ref_url",
            encode(
                "xsd_include",
                &[Op::Substitute(
                    SOURCE,
                    pool("types.xsd"),
                    pool("https://example.invalid/types.xsd"),
                )],
            ),
        ),
        (
            "json_ref_parent",
            encode(
                "json_ref",
                &[Op::Substitute(
                    SOURCE,
                    pool("types.schema.json"),
                    pool("../design-source.xsd"),
                )],
            ),
        ),
        (
            "json_ref_absolute",
            encode(
                "json_ref",
                &[Op::Substitute(
                    SOURCE,
                    pool("types.schema.json"),
                    pool("/etc/passwd"),
                )],
            ),
        ),
        // Include and reference chains.
        (
            "xsd_chain",
            encode("xml_to_json", &[Op::Chain(SOURCE, 8, Chain::Complete)]),
        ),
        (
            "xsd_chain_long",
            encode("xml_to_json", &[Op::Chain(SOURCE, 64, Chain::Complete)]),
        ),
        (
            "xsd_chain_cycle",
            encode("xml_to_json", &[Op::Chain(SOURCE, 3, Chain::Cycle)]),
        ),
        (
            "xsd_chain_dangling",
            encode("xml_to_json", &[Op::Chain(SOURCE, 3, Chain::Dangling)]),
        ),
        (
            "nested_xsd_chain",
            encode("nested_design", &[Op::Chain(SOURCE, 4, Chain::Complete)]),
        ),
        (
            "json_chain",
            encode("json_to_xml", &[Op::Chain(SOURCE, 8, Chain::Complete)]),
        ),
        (
            "json_chain_long",
            encode("json_to_xml", &[Op::Chain(SOURCE, 64, Chain::Complete)]),
        ),
        (
            "json_chain_cycle",
            encode("json_to_xml", &[Op::Chain(SOURCE, 3, Chain::Cycle)]),
        ),
        (
            "json_chain_dangling",
            encode("json_to_xml", &[Op::Chain(SOURCE, 3, Chain::Dangling)]),
        ),
        // Malformed resources.
        (
            "design_truncated",
            encode("xml_to_json", &[Op::Truncate(DESIGN, 600)]),
        ),
        (
            "design_invalid_utf8",
            encode("xml_to_json", &[Op::Splice(DESIGN, 40, b"\xff")]),
        ),
        (
            "design_not_mapping",
            encode("xml_to_json", &[Op::Replace(DESIGN, b"<schema/>")]),
        ),
        (
            "design_entity_expansion",
            encode(
                "xml_to_json",
                &[Op::Replace(
                    DESIGN,
                    b"<!DOCTYPE m [<!ENTITY a \"aaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\"><!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;\">]><mapping version=\"22\">&c;</mapping>",
                )],
            ),
        ),
        (
            "design_deep_elements",
            encode(
                "empty",
                &[
                    Op::Add(pool("design.mfd"), b""),
                    Op::Wrap(DESIGN, 4096, b"<mapping>", b"</mapping>"),
                ],
            ),
        ),
        (
            "design_deep_resources",
            encode(
                "xml_to_json",
                &[Op::Substitute(
                    DESIGN,
                    Name::Raw(b"<resources/>"),
                    Name::Raw(deep_elements(4096).as_bytes()),
                )],
            ),
        ),
        (
            "design_deep_source_entries",
            deep_entries(SOURCE_LEAF, DEEP_ENTRIES),
        ),
        (
            "design_deep_target_entries",
            deep_entries(TARGET_LEAF, DEEP_ENTRIES),
        ),
        (
            "design_component_elements_below_limit",
            nested_around(SOURCE_LEAF, 1_500, b"<n>", b"</n>"),
        ),
        (
            "design_deep_component_elements",
            nested_around(SOURCE_LEAF, 16_000, b"<n>", b"</n>"),
        ),
        (
            "xsd_deep_nesting",
            encode(
                "xml_to_json",
                &[
                    Op::Replace(SOURCE, b""),
                    Op::Wrap(SOURCE, DEEP_XSD, XSD_OPEN.as_bytes(), XSD_CLOSE.as_bytes()),
                    Op::Wrap(SOURCE, 1, XSD_HEAD.as_bytes(), XSD_TAIL.as_bytes()),
                ],
            ),
        ),
        (
            "xsd_deep_appinfo",
            encode(
                "xml_to_json",
                &[
                    Op::Replace(SOURCE, b""),
                    Op::Wrap(SOURCE, DEEP_INSTANCE, b"<n>", b"</n>"),
                    Op::Wrap(
                        SOURCE,
                        1,
                        APPINFO_HEAD.as_bytes(),
                        APPINFO_TAIL.as_bytes(),
                    ),
                ],
            ),
        ),
        (
            "xsd_named_type_chain",
            encode(
                "xml_to_json",
                &[Op::Replace(SOURCE, named_type_chain(160).as_bytes())],
            ),
        ),
        (
            "instance_deep",
            encode(
                "xml_to_json",
                &[
                    Op::Add(Name::Raw(b"input.xml"), b""),
                    Op::Wrap(3, DEEP_INSTANCE, b"<Input>", b"</Input>"),
                ],
            ),
        ),
        (
            "xsd_truncated",
            encode("xml_to_json", &[Op::Truncate(SOURCE, 120)]),
        ),
        (
            "xsd_invalid_utf8",
            encode("xml_to_json", &[Op::Splice(SOURCE, 60, b"\xc3\x28")]),
        ),
        (
            "xsd_not_schema",
            encode("xml_to_json", &[Op::Replace(SOURCE, b"<Input/>")]),
        ),
        (
            "xsd_deep_types",
            encode("xml_to_json", &[Op::Replace(SOURCE, deep_xsd(150).as_bytes())]),
        ),
        (
            "json_schema_truncated",
            encode("json_to_xml", &[Op::Truncate(SOURCE, 40)]),
        ),
        (
            "json_schema_invalid_utf8",
            encode("json_to_xml", &[Op::Splice(SOURCE, 20, b"\xed\xa0\x80")]),
        ),
        (
            "json_schema_not_object",
            encode("json_to_xml", &[Op::Replace(SOURCE, b"[1,2,3]")]),
        ),
        (
            "json_schema_deep",
            encode(
                "json_to_xml",
                &[Op::Wrap(
                    SOURCE,
                    200,
                    b"{\"type\":\"object\",\"properties\":{\"v\":",
                    b"}}",
                )],
            ),
        ),
        // A one-byte pattern replaced by 256 bytes in a 4 MiB filler entry
        // stops at the entry limit instead of expanding to 1 GiB.
        (
            "entry_substitution_expansion",
            encode(
                "xml_to_json",
                &[
                    Op::Filler(1, AT_ENTRY_LIMIT),
                    Op::Substitute(3, Name::Raw(b"#"), Name::Raw(&[b'x'; 256])),
                ],
            ),
        ),
        (
            "dtd_truncated",
            encode("dtd_source", &[Op::Truncate(SOURCE, 10)]),
        ),
    ];
    seeds.sort_by_key(|(name, _)| *name);
    seeds
}

/// Rewrites the design's source schema reference to `reference`.
fn ref_seed(
    name: &'static str,
    template: &str,
    index: u8,
    reference: &str,
) -> (&'static str, Vec<u8>) {
    (
        name,
        encode(
            template,
            &[Op::Substitute(
                index,
                Name::Raw(b"schema=\"design-source.xsd\""),
                Name::Raw(format!("schema=\"{reference}\"").as_bytes()),
            )],
        ),
    )
}

/// `depth` nested empty `<n>` elements.
fn deep_elements(depth: usize) -> String {
    format!("{}{}", "<n>".repeat(depth), "</n>".repeat(depth))
}

/// Nesting of the deep entry-tree, XSD and instance seeds: far past the
/// depth at which a recursive descent would overflow an 8 MiB stack, and
/// within the per-entry byte limit.
const DEEP_ENTRIES: u16 = 60_000;
const DEEP_XSD: u16 = 30_000;
const DEEP_INSTANCE: u16 = 60_000;

/// The design's source scalar and target string entries.
const SOURCE_LEAF: &str = r#"<entry name="Value" outkey="2" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>"#;
const TARGET_LEAF: &str = r#"<entry name="string" inpkey="4"/>"#;

/// The `xml_to_json` design with `depth` plain `<entry>` elements around
/// `leaf`.
fn deep_entries(leaf: &str, depth: u16) -> Vec<u8> {
    nested_around(leaf, depth, b"<entry name=\"n\">", b"</entry>")
}

/// The `xml_to_json` design with `depth` `open`/`close` elements around
/// `leaf`: the leaf is wrapped deeply, then once in the rest of the design.
fn nested_around(leaf: &str, depth: u16, open: &[u8], close: &[u8]) -> Vec<u8> {
    let template = crate::templates::all()
        .into_iter()
        .find(|template| template.name == "xml_to_json")
        .and_then(|template| template.entries.into_iter().next())
        .map(|(_, design)| String::from_utf8_lossy(&design).into_owned())
        .unwrap_or_default();
    let (head, tail) = template
        .split_once(leaf)
        .unwrap_or_else(|| panic!("the xml_to_json design has no {leaf}"));
    encode(
        "xml_to_json",
        &[
            Op::Replace(DESIGN, leaf.as_bytes()),
            Op::Wrap(DESIGN, depth, open, close),
            Op::Wrap(DESIGN, 1, head.as_bytes(), tail.as_bytes()),
        ],
    )
}

/// An XSD whose `Input` element nests anonymous complex types, built by
/// wrapping `XSD_OPEN`/`XSD_CLOSE` and then `XSD_HEAD`/`XSD_TAIL` once.
const XSD_HEAD: &str =
    r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input">"#;
const XSD_TAIL: &str = "</xs:element></xs:schema>";
const XSD_OPEN: &str = r#"<xs:complexType><xs:sequence><xs:element name="Value">"#;
const XSD_CLOSE: &str = "</xs:element></xs:sequence></xs:complexType>";

/// An XSD whose annotation holds the wrapped foreign elements, ahead of the
/// template's `Input` declaration.
const APPINFO_HEAD: &str =
    r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:annotation><xs:appinfo>"#;
const APPINFO_TAIL: &str = r#"</xs:appinfo></xs:annotation><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;

/// An XSD whose `Input` type reaches `xs:string` through `length` named
/// complex types, each holding one element of the next type.
fn named_type_chain(length: usize) -> String {
    let mut xsd = String::from(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input" type="t0"/>"#,
    );
    for index in 0..length {
        xsd.push_str(&format!(
            r#"<xs:complexType name="t{index}"><xs:sequence><xs:element name="Value" type="t{}"/></xs:sequence></xs:complexType>"#,
            index + 1
        ));
    }
    xsd.push_str(&format!(
        r#"<xs:complexType name="t{length}"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:schema>"#
    ));
    xsd
}

/// An XSD whose `Input` element nests `depth` anonymous complex types.
fn deep_xsd(depth: usize) -> String {
    format!(
        "{XSD_HEAD}{}{}{XSD_TAIL}",
        XSD_OPEN.repeat(depth),
        XSD_CLOSE.repeat(depth)
    )
}
