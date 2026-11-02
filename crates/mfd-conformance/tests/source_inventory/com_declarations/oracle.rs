//! Compact expected tuples from an independent dump-text transcription.
//!
//! The candidate is a bounded binary readback. These tables were NOT derived
//! from that JSON, its binary decoder, or the scout's candidate coverage JSON.
//! See the inventory README for source hashes and the five event exceptions.

use std::sync::OnceLock;

use serde_json::{Value, json};

use super::schema::{Coverage, Declarations};

fn read(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub fn core_expected() -> &'static Vec<Value> {
    static ROWS: OnceLock<Vec<Value>> = OnceLock::new();
    ROWS.get_or_init(|| read(include_str!("../oracles/com-core-declarations.jsonl")))
}

pub fn coverage_expected() -> &'static Vec<Value> {
    static ROWS: OnceLock<Vec<Value>> = OnceLock::new();
    ROWS.get_or_init(|| read(include_str!("../oracles/com-callable-coverage.jsonl")))
}

pub fn core_tuples(value: &Declarations) -> Vec<Value> {
    let mut rows = Vec::new();
    for t in &value.types {
        rows.push(json!([
            "T",
            t.type_info_index,
            t.name,
            t.kind,
            t.name_offset,
            t.header_dump_lines
        ]));
    }
    for d in &value.descriptors {
        rows.push(json!([
            "D",
            d.descriptor_offset,
            d.descriptor_word,
            d.target_word
        ]));
    }
    for o in &value.owners {
        rows.push(json!([
            "O",
            o.name,
            o.type_info_index,
            o.type_flags,
            o.member_block_offset,
            o.member_data_bytes,
            o.function_records,
            o.member_id_table_offset,
            o.member_name_table_offset,
            o.member_record_table_offset
        ]));
    }
    for r in &value.records {
        let params: Vec<_> = r
            .parameters
            .iter()
            .map(|p| {
                json!([
                    p.index,
                    p.name,
                    p.name_offset,
                    p.record_offset,
                    p.parameter_flags,
                    p.datatype.encoded,
                    p.datatype.pointer_depth,
                    p.datatype.leaf.tuple(),
                    p.datatype.descriptor_chain
                ])
            })
            .collect();
        let a = &r.source_anchor;
        rows.push(json!([
            "R",
            r.owner,
            r.type_info_index,
            r.name,
            r.record_index,
            r.dispatch_member_id,
            r.raw_member_id,
            r.name_offset,
            r.record_offset,
            r.record_bytes,
            r.invocation_kind,
            r.invocation_kind_value,
            r.function_flags,
            r.optional_argument_count,
            r.return_type.encoded,
            params,
            [
                a.function_dump_lines[0],
                a.function_dump_lines[1],
                a.member_id_dump_line,
                a.member_name_dump_line,
                a.member_offset_dump_line
            ],
            r.catalog_id
        ]));
    }
    rows
}

pub fn coverage_tuples(value: &Coverage) -> Vec<Value> {
    let mut rows = Vec::new();
    for d in &value.declarations {
        rows.push(json!([
            "T",
            d.type_info_index,
            d.name,
            d.kind,
            d.function_records,
            d.variable_records
        ]));
    }
    for r in &value.records {
        rows.push(json!([
            "R",
            r.type_info_index,
            r.owner,
            r.record_index,
            r.name,
            r.invocation_kind,
            r.dispatch_member_id,
            r.record_offset,
            r.record_bytes,
            r.function_dump_lines,
            r.cohort
        ]));
    }
    rows
}
