//! X12-only schema hints shared by the tree and canvas endpoint rows.

use ir::SchemaNode;
use mapping::{EdiBoundaryKind, FormatOptions};

pub(crate) fn boundary_has_x12(
    schema: &SchemaNode,
    path: Option<&str>,
    options: &FormatOptions,
) -> bool {
    match options.edi_kind {
        Some(EdiBoundaryKind::X12) => true,
        Some(_) => false,
        None => {
            let edi_path = path
                .and_then(|path| std::path::Path::new(path).extension())
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "edi" | "x12" | "edifact" | "hl7"
                    )
                });
            edi_path && matches!(format_edi::dialect_of(schema), Ok(format_edi::Dialect::X12))
        }
    }
}

pub(crate) fn segment_description(enabled: bool, name: &str) -> Option<&'static str> {
    enabled
        .then(|| format_edi::x12::segment_description(name))
        .flatten()
}

pub(crate) fn append_segment_for_path(base: String, enabled: bool, path: &str) -> String {
    let segment =
        path.split('/').rev().skip(1).find_map(|name| {
            segment_description(enabled, name).map(|description| (name, description))
        });
    segment.map_or(base.clone(), |(name, description)| {
        format!("{base}\nX12 {name}: {description}")
    })
}

pub(crate) fn endpoint_header_hint(
    enabled: bool,
    title: &str,
    context: Option<&str>,
) -> Option<String> {
    let title_segment = || {
        title
            .strip_prefix("Source: ")
            .or_else(|| title.strip_prefix("Target: "))
            .and_then(|label| label.split_whitespace().next())
    };
    let segment = context.or_else(title_segment)?;
    segment_description(enabled, segment).map(|description| format!("X12 {segment}: {description}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x12_schema() -> SchemaNode {
        SchemaNode::group(
            "Interchange",
            vec![SchemaNode::group(
                "ISA",
                vec![SchemaNode::scalar("ISA01", ir::ScalarType::String)],
            )],
        )
    }

    #[test]
    fn path_hints_use_the_nearest_segment_ancestor() {
        assert_eq!(
            append_segment_for_path(
                "Source: Interchange/Message/N1/N101".into(),
                true,
                "Interchange/Message/N1/N101",
            ),
            "Source: Interchange/Message/N1/N101\nX12 N1: Party Identification"
        );
    }

    #[test]
    fn disabled_and_unknown_paths_remain_unchanged() {
        let base = "Source: Order/ST/Value".to_string();
        assert_eq!(
            append_segment_for_path(base.clone(), false, "Order/ST/Value"),
            base
        );
        assert_eq!(
            append_segment_for_path("Source: Root/Loop/Value".into(), true, "Root/Loop/Value"),
            "Source: Root/Loop/Value"
        );
    }

    #[test]
    fn endpoint_headers_use_context_or_unchunked_title_segment() {
        assert_eq!(
            endpoint_header_hint(true, "Source: Orders", Some("BEG")),
            Some("X12 BEG: Beginning Segment for Purchase Order".into())
        );
        assert_eq!(
            endpoint_header_hint(true, "Target: NM1 (2/3)", None),
            Some("X12 NM1: Individual or Organizational Name".into())
        );
        assert_eq!(endpoint_header_hint(false, "Source: ISA", None), None);
    }

    #[test]
    fn boundary_detection_prefers_explicit_kind_then_edi_path_inference() {
        let schema = x12_schema();
        let explicit_x12 = FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            ..FormatOptions::default()
        };
        let explicit_edifact = FormatOptions {
            edi_kind: Some(EdiBoundaryKind::Edifact),
            ..FormatOptions::default()
        };

        assert!(boundary_has_x12(&schema, Some("orders.xml"), &explicit_x12));
        assert!(!boundary_has_x12(
            &schema,
            Some("orders.x12"),
            &explicit_edifact
        ));
        assert!(boundary_has_x12(
            &schema,
            Some("ORDERS.X12"),
            &FormatOptions::default()
        ));
        assert!(!boundary_has_x12(
            &schema,
            Some("orders.xml"),
            &FormatOptions::default()
        ));
    }
}
