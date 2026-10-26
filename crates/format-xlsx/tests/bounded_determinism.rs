use std::{collections::BTreeMap, io::Read, thread, time::Duration};

use format_xlsx::{FlatTableWriteOptions, XlsxLimits, to_bytes_with_options_bounded};
use ir::{Instance, ScalarType, SchemaNode, Value};

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index(index).unwrap();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).unwrap();
            (entry.name().to_string(), content)
        })
        .collect()
}

#[test]
fn bounded_new_workbooks_replay_across_clock_ticks() {
    let schema = SchemaNode::group(
        "rows",
        vec![
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("count", ScalarType::Int),
        ],
    );
    let rows = vec![Instance::Group(
        vec![
            (
                "label".into(),
                Instance::Scalar(Value::String("α <&>".into())),
            ),
            ("count".into(), Instance::Scalar(Value::Int(42))),
        ]
        .into(),
    )];
    let headers = ["Physical label".into(), "Total".into()];
    let options = [
        FlatTableWriteOptions {
            sheet: None,
            start_row: 1,
            columns: &[],
            headers: &[],
            has_header: true,
        },
        FlatTableWriteOptions {
            sheet: Some("Revenue"),
            start_row: 3,
            columns: &[2, 5],
            headers: &headers,
            has_header: true,
        },
        FlatTableWriteOptions {
            sheet: Some("No header"),
            start_row: 4,
            columns: &[3, 7],
            headers: &[],
            has_header: false,
        },
    ];
    let limits = XlsxLimits::default();
    let write = |options| to_bytes_with_options_bounded(&schema, &rows, options, &limits).unwrap();
    let before: Vec<_> = options.iter().copied().map(write).collect();
    assert_eq!(
        before[0],
        format_xlsx::to_bytes_bounded(&schema, &rows, None, 1, &[], true, &limits).unwrap()
    );
    // The upstream document clock has one-second resolution; cross both that
    // boundary and ZIP's two-second timestamp resolution without clock polling.
    thread::sleep(Duration::from_millis(2100));
    for (options, before) in options.into_iter().zip(before) {
        let after = write(options);
        let before_parts = parts(&before);
        let after_parts = parts(&after);
        let changed: Vec<_> = before_parts
            .iter()
            .filter(|(name, content)| after_parts.get(*name) != Some(*content))
            .map(|(name, _)| name.as_str())
            .collect();
        assert!(
            before == after,
            "same-input replay changed parts: {changed:?}"
        );
        let mut bounded_parts = after_parts;
        let bounded_core =
            String::from_utf8(bounded_parts.remove("docProps/core.xml").unwrap()).unwrap();
        assert_eq!(bounded_core.matches("1980-01-01T00:00:00Z").count(), 2);

        let legacy = format_xlsx::to_bytes_with_options(&schema, &rows, options).unwrap();
        let mut legacy_parts = parts(&legacy);
        let legacy_core =
            String::from_utf8(legacy_parts.remove("docProps/core.xml").unwrap()).unwrap();
        assert_eq!(bounded_parts, legacy_parts, "only core metadata may change");
        let legacy_timestamp = legacy_core
            .split_once("<dcterms:created xsi:type=\"dcterms:W3CDTF\">")
            .unwrap()
            .1
            .split_once("</dcterms:created>")
            .unwrap()
            .0;
        assert_ne!(legacy_timestamp, "1980-01-01T00:00:00Z");
        assert_eq!(
            bounded_core.replace("1980-01-01T00:00:00Z", legacy_timestamp),
            legacy_core,
            "legacy properties other than timestamps must be unchanged"
        );
        assert_eq!(
            format_xlsx::from_bytes_bounded(
                &after,
                &schema,
                options.sheet,
                options.start_row,
                options.columns,
                options.has_header,
                &limits,
            )
            .unwrap(),
            rows
        );
    }
}
