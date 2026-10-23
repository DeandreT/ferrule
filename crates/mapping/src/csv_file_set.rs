//! The explicit execution view for physical CSV files. The original row schema
//! is retained separately by its importer/host; this wrapper supplies only file
//! membership, source paths and repeated row occurrences.

use ir::{ScalarType, SchemaNode};

pub const CSV_FILE_SET_FILES_FIELD: &str = "Files";
pub const CSV_FILE_SET_PATH_FIELD: &str = "Path";

pub fn csv_file_set_source_schema(row: &SchemaNode) -> SchemaNode {
    SchemaNode::group(
        "CsvFileSet",
        vec![
            SchemaNode::group(
                CSV_FILE_SET_FILES_FIELD,
                vec![
                    SchemaNode::scalar(CSV_FILE_SET_PATH_FIELD, ScalarType::String),
                    row.clone().repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
