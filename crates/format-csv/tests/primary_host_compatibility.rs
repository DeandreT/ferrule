//! Old CSV-qualified host paths remain aliases, not parallel implementations.
use std::convert::Infallible;

use format_csv::{CsvSourceData, PrimarySourceHost};
use mapping::PrimaryFileSelection;

struct LegacyHost;

impl PrimarySourceHost for LegacyHost {
    type Error = Infallible;

    fn select(&self, _: &str, _: PrimaryFileSelection) -> Result<Vec<String>, Self::Error> {
        Ok(vec!["input.xml".into()])
    }

    fn load(&self, _: &str) -> Result<CsvSourceData, Self::Error> {
        Ok(CsvSourceData {
            bytes: b"<root/>".to_vec(),
            resolved_path: Some("/input.xml".into()),
        })
    }
}

#[test]
fn csv_qualified_hosts_and_data_work_with_the_neutral_entry_point() {
    let documents = document_source::acquire_primary_documents(
        || Ok::<_, Infallible>("input.xml".into()),
        PrimaryFileSelection::Direct,
        &LegacyHost,
        1,
        |_, data: document_source::SourceData| Ok::<_, Infallible>(data.bytes),
    )
    .unwrap();
    assert_eq!(documents[0].request, "input.xml");
    assert_eq!(documents[0].resolved_path.as_deref(), Some("/input.xml"));
    assert_eq!(documents[0].value, b"<root/>");
    assert_eq!(
        format_csv::MAX_CSV_SOURCE_DOCUMENTS,
        document_source::MAX_SOURCE_DOCUMENTS
    );
    assert_eq!(
        format_csv::MAX_CSV_SOURCE_PATH_BYTES,
        document_source::MAX_SOURCE_PATH_BYTES
    );
    assert_eq!(
        format_csv::DEFAULT_LOCAL_DOCUMENT_BYTES,
        document_source::DEFAULT_LOCAL_DOCUMENT_BYTES
    );
    assert_eq!(
        document_source::DEFAULT_LOCAL_DOCUMENT_BYTES,
        format_csv::MAX_CSV_INPUT_BYTES as u64
    );
}
