use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{NumericRange, SchemaNode};

use super::super::{import_str, import_with_root};
use super::{export, import_str_result};
use crate::JsonFormatError;

fn both_boundaries(text: &str) -> Result<SchemaNode, JsonFormatError> {
    let inline = import_str(text)?;
    assert_eq!(inline, import_str_result(text)?);
    Ok(inline)
}

fn integer_bounds(schema: &SchemaNode) -> (Option<i64>, Option<i64>) {
    let Some(NumericRange::Integer(range)) = schema.numeric_range else {
        panic!("the integer range should be retained");
    };
    (range.minimum(), range.maximum())
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-native-bound-lexemes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exact_decimal_and_exponent_bounds_import_at_inline_and_file_roots() -> Result<(), JsonFormatError>
{
    for (bounds, expected) in [
        (
            r#""minimum":1.0,"exclusiveMaximum":5e0"#,
            (Some(1), Some(4)),
        ),
        (
            r#""exclusiveMinimum":1.0,"maximum":5e0"#,
            (Some(2), Some(5)),
        ),
        (r#""minimum":12e-1,"maximum":3.000"#, (Some(2), Some(3))),
        (r#""minimum":1200e-2,"maximum":1.3e1"#, (Some(12), Some(13))),
        (
            r#""minimum":-1.20e1,"maximum":-10.00"#,
            (Some(-12), Some(-10)),
        ),
        (r#""minimum":-0.0,"maximum":0e10"#, (Some(0), Some(0))),
        (r#""minimum":-0,"exclusiveMaximum":1e0"#, (Some(0), Some(0))),
    ] {
        let text = format!(r#"{{"title":"Bound","type":"integer",{bounds}}}"#);
        let schema = both_boundaries(&text)?;
        assert_eq!(integer_bounds(&schema), expected, "{text}");
        assert_eq!(import_str(&export(&schema))?, schema);
        if let Some(minimum) = expected.0 {
            assert!(
                crate::from_str(&minimum.to_string(), &schema).is_ok(),
                "{text}"
            );
            if let Some(below) = minimum.checked_sub(1) {
                assert!(
                    crate::from_str(&below.to_string(), &schema).is_err(),
                    "{text}"
                );
            }
        }
        if let Some(maximum) = expected.1 {
            assert!(
                crate::from_str(&maximum.to_string(), &schema).is_ok(),
                "{text}"
            );
            if let Some(above) = maximum.checked_add(1) {
                assert!(
                    crate::from_str(&above.to_string(), &schema).is_err(),
                    "{text}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn exact_bound_spellings_preserve_signed_and_unsigned_token_domains() -> Result<(), JsonFormatError>
{
    let schema = both_boundaries(
        r#"{"type":"integer","minimum":-9223372036854775808.0,"maximum":9223372036854775807e0}"#,
    )?;
    assert_eq!(integer_bounds(&schema), (Some(i64::MIN), Some(i64::MAX)));
    assert_eq!(import_str(&export(&schema))?, schema);
    let unsigned_upper =
        both_boundaries(r#"{"type":"integer","minimum":-0.0,"maximum":18446744073709551615.0}"#)?;
    assert_eq!(integer_bounds(&unsigned_upper), (Some(0), None));
    for bounds in [
        r#""exclusiveMinimum":9223372036854775807.0"#,
        r#""exclusiveMaximum":-9223372036854775808e0"#,
        r#""minimum":9223372036854775808.0"#,
        r#""minimum":18446744073709551616.0"#,
        r#""maximum":-9223372036854775809.0"#,
    ] {
        let text = format!(r#"{{"type":"integer",{bounds}}}"#);
        for actual in [import_str(&text), import_str_result(&text)] {
            assert!(
                matches!(actual, Err(JsonFormatError::UnsupportedSchemaUnion { .. })),
                "{text}"
            );
        }
    }
    Ok(())
}

#[test]
fn integral_number_bounds_match_existing_integer_token_inequalities() -> Result<(), JsonFormatError>
{
    for keyword in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] {
        let decimal = format!(r#"{{"type":"number","{keyword}":9007199254740993.0}}"#);
        let integer = format!(r#"{{"type":"number","{keyword}":9007199254740993}}"#);
        let schema = both_boundaries(&decimal)?;
        assert_eq!(schema, import_str(&integer)?, "{keyword}");
        assert_eq!(import_str(&export(&schema))?, schema, "{keyword}");
    }
    let exact_unsigned = both_boundaries(r#"{"type":"number","minimum":9223372036854775808.0}"#)?;
    let Some(NumericRange::Number(range)) = exact_unsigned.numeric_range else {
        panic!("the number range should be retained");
    };
    assert_eq!(
        range.minimum().map(|b| b.value().get()),
        Some(9_223_372_036_854_775_808.0)
    );
    assert_eq!(import_str(&export(&exact_unsigned))?, exact_unsigned);
    assert!(matches!(
        import_str(r#"{"type":"number","minimum":18446744073709551615.0}"#),
        Err(JsonFormatError::UnsupportedSchemaUnion { .. })
    ));
    Ok(())
}

#[test]
fn fractional_near_integer_and_precision_cliff_refusals_remain() {
    for (keyword, token) in [
        ("minimum", "9007199254740992.5"),
        ("minimum", "4503599627370496.5"),
        ("minimum", "1.00000000000000001"),
        ("maximum", "0.99999999999999999"),
    ] {
        let text = format!(r#"{{"type":"integer","{keyword}":{token}}}"#);
        for actual in [import_str(&text), import_str_result(&text)] {
            assert!(
                matches!(
                    actual,
                    Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                        if reason.contains("exactly normalizable")
                ),
                "{text}"
            );
        }
    }
    for text in [
        r#"{"allOf":[{"type":"number","minimum":1.00000000000000001},{"type":"integer"}]}"#,
        r#"{"allOf":[{"type":"number","maximum":9007199254740991.5},{"type":"integer"}]}"#,
    ] {
        assert!(matches!(
            import_str(text),
            Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                if reason.contains("ambiguous")
        ));
    }
}

#[test]
fn escaped_bound_names_and_duplicate_schema_entries_keep_last_value_semantics()
-> Result<(), JsonFormatError> {
    let schema = both_boundaries(
        r#"{"type":"integer","minimum":1.0,"min\u0069mum":3e0,"max\u0069mum":4.0}"#,
    )?;
    assert_eq!(integer_bounds(&schema), (Some(3), Some(4)));
    assert!(matches!(
        import_str(r#"{"type":"integer","minimum":2.0,"min\u0069mum":1.00000000000000001}"#),
        Err(JsonFormatError::UnsupportedSchemaUnion { .. })
    ));
    let object = both_boundaries(
        r#"{"type":"object","properties":{
            "minimum":{"type":"integer","minimum":1.00000000000000001},
            "last":{"type":"string"},
            "minimum":{"type":"integer","minimum":2.0,"maximum":3e0}
        }}"#,
    )?;
    let child = object
        .child("minimum")
        .expect("the final minimum property should exist");
    assert_eq!(integer_bounds(child), (Some(2), Some(3)));
    let ir::SchemaKind::Group { children, .. } = &object.kind else {
        panic!("the object schema should remain a group");
    };
    assert_eq!(
        children.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["minimum", "last"]
    );
    Ok(())
}

#[test]
fn document_local_refs_preserve_exact_decimal_bounds() -> Result<(), JsonFormatError> {
    let text = r##"{"$ref":"#/$defs/Bound","$defs":{
        "Bound":{"title":"Bound","type":"integer","minimum":2.0,"maximum":5e0}
    }}"##;
    let schema = both_boundaries(text)?;
    assert_eq!(integer_bounds(&schema), (Some(2), Some(5)));
    assert_eq!(import_str(&export(&schema))?, schema);
    Ok(())
}

#[test]
fn every_confined_resource_keeps_its_own_dialect_and_bound_lexemes()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    std::fs::create_dir(directory.0.join("package"))?;
    let package = directory.0.join("package");
    std::fs::write(
        package.join("middle.json"),
        r#"{"$schema":"http://json-schema.org/draft-04/schema#","$ref":"leaf.json","minimum":99.0}"#,
    )?;
    std::fs::write(
        package.join("leaf.json"),
        r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"integer","minimum":2.0,"maximum":10e0}"#,
    )?;
    for (dialect, expected) in [
        (Some("http://json-schema.org/draft-04/schema#"), 2),
        (Some("http://json-schema.org/draft-06/schema#"), 2),
        (Some("http://json-schema.org/draft-07/schema#"), 2),
        (Some("https://json-schema.org/draft/2019-09/schema"), 3),
        (Some("https://json-schema.org/draft/2020-12/schema"), 3),
        (None, 3),
    ] {
        let declaration = dialect
            .map(|d| format!(r#""$schema":"{d}","#))
            .unwrap_or_default();
        let text = format!(r#"{{{declaration}"$ref":"middle.json","minimum":3e0}}"#);
        std::fs::write(package.join("root.json"), &text)?;
        let schema = import_with_root(&package.join("root.json"), &package)?;
        assert_eq!(
            integer_bounds(&schema),
            (Some(expected), Some(10)),
            "{text}"
        );
        assert_eq!(import_str(&export(&schema))?, schema);
        assert!(matches!(
            import_str(&text),
            Err(JsonFormatError::SchemaResource { .. })
        ));
    }
    std::fs::write(
        directory.0.join("outside.json"),
        r#"{"type":"integer","minimum":2.0}"#,
    )?;
    std::fs::write(package.join("root.json"), r#"{"$ref":"../outside.json"}"#)?;
    assert!(matches!(
        import_with_root(&package.join("root.json"), &package),
        Err(JsonFormatError::SchemaResource { .. })
    ));
    Ok(())
}

#[test]
fn legacy_boolean_exclusives_and_all_of_keep_exact_intersections() -> Result<(), JsonFormatError> {
    let legacy = both_boundaries(
        r#"{"$schema":"http://json-schema.org/draft-04/schema#","type":"integer",
        "minimum":1.0,"exclusiveMinimum":true,"maximum":5e0,"exclusiveMaximum":false}"#,
    )?;
    assert_eq!(integer_bounds(&legacy), (Some(2), Some(5)));
    let composed = both_boundaries(
        r#"{"allOf":[{"type":"integer","minimum":1.0,"maximum":9e0},
        {"type":"integer","exclusiveMinimum":2e0,"maximum":7.000}]}"#,
    )?;
    assert_eq!(integer_bounds(&composed), (Some(3), Some(7)));
    assert_eq!(import_str(&export(&legacy))?, legacy);
    assert_eq!(import_str(&export(&composed))?, composed);
    Ok(())
}

#[test]
fn escaped_opaque_pointer_targets_normalize_only_when_reached() -> Result<(), JsonFormatError> {
    let target = both_boundaries(
        r##"{"$ref":"#/default/A~1B~0C","default":{
        "A/B~C":{"type":"integer","minimum":2.0,"maximum":5e0}}}"##,
    )?;
    assert_eq!(integer_bounds(&target), (Some(2), Some(5)));
    assert_eq!(import_str(&export(&target))?, target);
    let unreferenced = both_boundaries(
        r#"{"type":"integer","default":{
        "A/B~C":{"type":"integer","minimum":2.0,"maximum":5e0}}}"#,
    )?;
    assert!(unreferenced.numeric_range.is_none());
    assert!(crate::from_str("1", &unreferenced).is_ok());
    Ok(())
}

#[test]
fn existing_local_ref_chains_reach_opaque_escaped_targets() -> Result<(), JsonFormatError> {
    let text = r##"{"$ref":"#/default/chain","default":{
        "chain":{"$ref":"#/default/A~1B~0C"},
        "A/B~C":{"type":"integer","minimum":2.0,"maximum":5e0}}}"##;
    let schema = both_boundaries(text)?;
    assert_eq!(integer_bounds(&schema), (Some(2), Some(5)));
    assert_eq!(import_str(&export(&schema))?, schema);
    let cycle = r##"{"$ref":"#/default/chain","default":{
        "chain":{"$ref":"#/default/chain"}}}"##;
    for result in [import_str(cycle), import_str_result(cycle)] {
        assert!(matches!(
            result,
            Err(JsonFormatError::ContentlessSchema {
                reason: crate::ContentlessSchemaReason::ReferenceCycle,
                ..
            })
        ));
    }
    Ok(())
}

#[test]
fn confined_external_opaque_fragments_keep_existing_resource_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    for root_dialect in [
        Some("http://json-schema.org/draft-04/schema#"),
        Some("http://json-schema.org/draft-06/schema#"),
        Some("http://json-schema.org/draft-07/schema#"),
        Some("https://json-schema.org/draft/2019-09/schema"),
        Some("https://json-schema.org/draft/2020-12/schema"),
        None,
    ] {
        for leaf_dialect in [
            Some("http://json-schema.org/draft-04/schema#"),
            Some("https://json-schema.org/draft/2020-12/schema"),
            None,
        ] {
            let root_decl = root_dialect
                .map(|d| format!(r#""$schema":"{d}","#))
                .unwrap_or_default();
            let leaf_decl = leaf_dialect
                .map(|d| format!(r#""$schema":"{d}","#))
                .unwrap_or_default();
            let root =
                format!(r#"{{{root_decl}"$ref":"external.json#/default/A~1B~0C","minimum":3e0}}"#);
            let leaf = format!(
                r#"{{{leaf_decl}"default":{{"A/B~C":{{"type":"integer","minimum":2.0,"maximum":5e0}}}}}}"#
            );
            std::fs::write(directory.0.join("root.json"), &root)?;
            std::fs::write(directory.0.join("external.json"), &leaf)?;
            let schema = import_with_root(&directory.0.join("root.json"), &directory.0)?;
            let expected_minimum = if root_dialect.is_some_and(|d| d.contains("draft-0")) {
                2
            } else {
                3
            };
            assert_eq!(integer_bounds(&schema), (Some(expected_minimum), Some(5)));
            assert_eq!(import_str(&export(&schema))?, schema);
            // The independently spelled integer-token documents retain the
            // same existing resource flags; no policy is added at dereference.
            let token_root = root.replace("3e0", "3");
            let token_leaf = leaf.replace("2.0", "2").replace("5e0", "5");
            std::fs::write(directory.0.join("root.json"), token_root)?;
            std::fs::write(directory.0.join("external.json"), token_leaf)?;
            assert_eq!(
                import_with_root(&directory.0.join("root.json"), &directory.0)?,
                schema
            );
        }
    }
    Ok(())
}

#[test]
fn opaque_target_all_of_clones_keep_exact_numeric_constraints() -> Result<(), JsonFormatError> {
    let text = r##"{"$ref":"#/unknown/Bound","unknown":{"Bound":{
        "allOf":[{"minimum":2.0},{"type":"integer","maximum":5e0}]}}}"##;
    let schema = both_boundaries(text)?;
    assert_eq!(integer_bounds(&schema), (Some(2), Some(5)));
    assert_eq!(import_str(&export(&schema))?, schema);
    Ok(())
}
