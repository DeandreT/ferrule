//! Per-call cost attribution for the generated JSON endpoint boundary.
//!
//! A counting global allocator records allocations on the measuring thread.
//! Each row reports the mean allocation count, allocated bytes, and peak live
//! bytes above the starting point for one call. Run
//! `cargo test -p codegen-runtime --release --test json_endpoint_preparation -- --nocapture`
//! to see the table; `docs/performance/json-endpoint-preparation.md` records
//! the observations and how to read them.

mod support;

use codegen_runtime::{
    MAX_EMBEDDED_JSON_SCHEMA_BYTES, PreparedJsonSchema, parse_json, serialize_json,
    serialize_json_bytes,
};
use ir::{IntegerRange, JsonPatternConstraints, NumericRange, ScalarType, SchemaNode};
use support::allocation::{header, measure, retained, row};

fn patterned(name: &str, pattern: &str) -> SchemaNode {
    let Ok(patterns) = JsonPatternConstraints::new([[pattern]]) else {
        panic!("fixture pattern is valid");
    };
    let Some(schema) = SchemaNode::scalar(name, ScalarType::String).with_json_patterns(patterns)
    else {
        panic!("fixture pattern matches its string field");
    };
    schema
}

fn ranged(name: &str, minimum: i64, maximum: i64) -> SchemaNode {
    let Some(range) = IntegerRange::new(Some(minimum), Some(maximum)) else {
        panic!("fixture range is ordered");
    };
    let Some(schema) =
        SchemaNode::scalar(name, ScalarType::Int).with_numeric_range(NumericRange::Integer(range))
    else {
        panic!("fixture range matches its integer field");
    };
    schema
}

/// A self-authored order schema with patterns, ranges, and a repeated group,
/// padded with optional attributes to a realistic descriptor size.
fn order_schema() -> SchemaNode {
    let line = SchemaNode::group(
        "Line",
        vec![
            patterned("Sku", "^[A-Z]{2}-[0-9]{4}$"),
            ranged("Quantity", 1, 1_000),
            SchemaNode::scalar("Price", ScalarType::Float),
            SchemaNode::scalar("Note", ScalarType::String),
        ],
    )
    .repeating();
    let mut children = vec![
        patterned("Id", "^ORD-[0-9]{6}$"),
        SchemaNode::scalar("Customer", ScalarType::String),
        patterned("Currency", "^[A-Z]{3}$"),
        SchemaNode::scalar("Total", ScalarType::Float),
        line,
    ];
    for index in 0..48 {
        children.push(if index % 4 == 0 {
            patterned(
                &format!("Attribute{index:02}"),
                &format!("^[a-z]{{1,{}}}$", index + 8),
            )
        } else {
            SchemaNode::scalar(format!("Attribute{index:02}"), ScalarType::String)
        });
    }
    SchemaNode::group("Order", children)
}

fn order_document(lines: usize) -> String {
    let lines = (0..lines)
        .map(|index| {
            serde_json::json!({
                "Sku": format!("AB-{:04}", index % 10_000),
                "Quantity": index % 999 + 1,
                "Price": 1.25 + (index % 100) as f64,
                "Note": format!("line {index} of the order"),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "Id": "ORD-000042",
        "Customer": "Example Customer",
        "Currency": "EUR",
        "Total": 1234.5,
        "Line": lines,
        "Attribute00": "alpha",
        "Attribute01": "beta",
    })
    .to_string()
}

#[test]
fn json_endpoint_per_call_cost_attribution() {
    let schema = order_schema();
    let Ok(descriptor) = codegen_schema::encode(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES) else {
        panic!("fixture schema encodes within the embedded schema limit");
    };
    let small = order_document(3);
    let large = order_document(20_000);
    let Ok(small_instance) = parse_json(&descriptor, &small) else {
        panic!("small fixture parses");
    };
    let Ok(large_instance) = parse_json(&descriptor, &large) else {
        panic!("large fixture parses");
    };
    let Ok(rendered) = serialize_json(&descriptor, &large_instance) else {
        panic!("large fixture serializes");
    };
    assert_eq!(
        parse_json(&descriptor, &rendered),
        Ok(large_instance.clone())
    );

    println!(
        "descriptor {} bytes; small document {} bytes; large document {} bytes; large output {} bytes",
        descriptor.len(),
        small.len(),
        large.len(),
        rendered.len()
    );
    header();
    let decode = measure(200, || {
        codegen_schema::decode(&descriptor, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
    });
    row("embedded schema decode (`codegen_schema::decode`)", decode);
    row(
        "`PreparedJsonSchema::prepare`",
        measure(200, || PreparedJsonSchema::prepare(&descriptor)),
    );
    let (prepared, live) = retained(|| PreparedJsonSchema::prepare(&descriptor));
    let Ok(prepared) = prepared else {
        panic!("fixture descriptor prepares");
    };
    println!("| live bytes retained by one `PreparedJsonSchema` | | | {live} | |");
    row(
        "decoded schema clone (`SchemaNode::clone`)",
        measure(200, || schema.clone()),
    );
    row(
        "`format_json::from_str`, small",
        measure(200, || format_json::from_str(&small, &schema)),
    );
    row(
        "`format_json::to_value`, small",
        measure(200, || format_json::to_value(&schema, &small_instance)),
    );
    row(
        "owned `serde_json::Value` tree of the small input",
        measure(200, || serde_json::from_str::<serde_json::Value>(&small)),
    );
    let parse_small = measure(200, || parse_json(&descriptor, &small));
    row("`parse_json`, small", parse_small);
    row(
        "`PreparedJsonSchema::parse_json`, small",
        measure(200, || prepared.parse_json(&small)),
    );
    let serialize_small = measure(200, || serialize_json(&descriptor, &small_instance));
    row("`serialize_json`, small", serialize_small);
    row(
        "`PreparedJsonSchema::serialize_json`, small",
        measure(200, || prepared.serialize_json(&small_instance)),
    );
    row(
        "`parse_json`, large",
        measure(3, || parse_json(&descriptor, &large)),
    );
    row(
        "owned `serde_json::Value` tree of the large input",
        measure(3, || serde_json::from_str::<serde_json::Value>(&large)),
    );
    row(
        "`serialize_json`, large",
        measure(3, || serialize_json(&descriptor, &large_instance)),
    );
    row(
        "`serialize_json_bytes`, large",
        measure(3, || serialize_json_bytes(&descriptor, &large_instance)),
    );
    let Ok(value) = format_json::to_value(&schema, &large_instance) else {
        panic!("large fixture normalizes");
    };
    row(
        "owned normalized `serde_json::Value` of the large output",
        measure(3, || format_json::to_value(&schema, &large_instance)),
    );
    row(
        "pretty text from that value",
        measure(3, || serde_json::to_string_pretty(&value)),
    );
    row(
        "measuring pass over that value (`io::sink`)",
        measure(3, || serde_json::to_writer_pretty(std::io::sink(), &value)),
    );

    // Repeated calls share the prepared descriptor: a small document costs
    // less than decoding the descriptor once. Counts are deterministic for one
    // toolchain; timings are informational only.
    assert!(
        parse_small.allocations < decode.allocations,
        "{parse_small:?}"
    );
    assert!(
        serialize_small.allocations < decode.allocations,
        "{serialize_small:?}"
    );
}
