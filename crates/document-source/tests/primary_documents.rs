use std::cell::RefCell;
use std::convert::Infallible;

use document_source::{
    PrimaryAcquisitionError, PrimaryAssemblyError, PrimaryDocument, PrimarySourceHost, SourceData,
    acquire_primary_documents, assemble_primary_documents,
};
use ir::Instance;
use mapping::{EmptyFileSelection, PrimaryDocumentShape, PrimaryFileSelection};

const WILDCARD: PrimaryFileSelection = PrimaryFileSelection::FileNameWildcard {
    empty: EmptyFileSelection::MapNoDocuments,
};

struct Host<'a> {
    requests: Vec<String>,
    fail: Option<&'static str>,
    events: &'a RefCell<Vec<String>>,
}

impl PrimarySourceHost for Host<'_> {
    type Error = &'static str;

    fn select(&self, filename: &str, _: PrimaryFileSelection) -> Result<Vec<String>, Self::Error> {
        self.events.borrow_mut().push(format!("select:{filename}"));
        Ok(self.requests.clone())
    }

    fn load(&self, request: &str) -> Result<SourceData, Self::Error> {
        self.events.borrow_mut().push(format!("load:{request}"));
        if self.fail == Some(request) {
            return Err("load failed");
        }
        Ok(SourceData {
            bytes: request.as_bytes().to_vec(),
            resolved_path: Some(format!("/resolved/{request}")),
        })
    }
}

#[test]
fn selection_and_parsing_keep_host_order_and_both_identities() {
    let events = RefCell::new(Vec::new());
    let host = Host {
        requests: vec!["b.xml".into(), "a.json".into()],
        fail: None,
        events: &events,
    };
    let documents = acquire_primary_documents(
        || {
            events.borrow_mut().push("filename".into());
            Ok::<_, Infallible>("*".into())
        },
        WILDCARD,
        &host,
        2,
        |request, data| {
            events.borrow_mut().push(format!("parse:{request}"));
            assert_eq!(data.bytes, request.as_bytes());
            Ok::<_, Infallible>(Instance::Group(Vec::new().into()))
        },
    )
    .unwrap();
    assert_eq!(
        *events.borrow(),
        [
            "filename",
            "select:*",
            "load:b.xml",
            "parse:b.xml",
            "load:a.json",
            "parse:a.json"
        ]
    );
    assert_eq!(documents[0].request, "b.xml");
    assert_eq!(
        documents[0].resolved_path.as_deref(),
        Some("/resolved/b.xml")
    );
    let Instance::DocumentSet(members) =
        assemble_primary_documents(PrimaryDocumentShape::DocumentSet, documents).unwrap()
    else {
        panic!("document set")
    };
    assert_eq!(members[0].path(), "b.xml");
    assert_eq!(members[0].source_path(), "/resolved/b.xml");
    assert_eq!(members[1].path(), "a.json");
}

#[test]
fn load_and_parse_failures_do_not_reach_later_requests() {
    for fail_load in [true, false] {
        let events = RefCell::new(Vec::new());
        let host = Host {
            requests: vec!["first".into(), "second".into()],
            fail: fail_load.then_some("first"),
            events: &events,
        };
        let error = acquire_primary_documents(
            || Ok::<_, Infallible>("*".into()),
            WILDCARD,
            &host,
            2,
            |request, _| {
                events.borrow_mut().push(format!("parse:{request}"));
                Err::<(), _>("parse failed")
            },
        )
        .unwrap_err();
        if fail_load {
            assert!(
                matches!(error, PrimaryAcquisitionError::Load { path, source: "load failed" } if path == "first")
            );
            assert_eq!(*events.borrow(), ["select:*", "load:first"]);
        } else {
            assert!(
                matches!(error, PrimaryAcquisitionError::Input { path, source: "parse failed" } if path == "first")
            );
            assert_eq!(*events.borrow(), ["select:*", "load:first", "parse:first"]);
        }
    }
}

#[test]
fn admission_and_count_limits_precede_their_respective_effects() {
    let events = RefCell::new(Vec::new());
    let host = Host {
        requests: vec!["first".into(), String::new()],
        fail: None,
        events: &events,
    };
    let error = acquire_primary_documents(
        || {
            events.borrow_mut().push("filename".into());
            Ok::<_, Infallible>("*".into())
        },
        WILDCARD,
        &host,
        document_source::MAX_SOURCE_DOCUMENTS + 1,
        |_, _| Ok::<_, Infallible>(()),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PrimaryAcquisitionError::InvalidLimit { .. }
    ));
    assert!(events.borrow().is_empty());
    let error = acquire_primary_documents(
        || Ok::<_, Infallible>("*".into()),
        WILDCARD,
        &host,
        1,
        |_, _| Ok::<_, Infallible>(()),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PrimaryAcquisitionError::Documents { maximum: 1 }
    ));
    assert_eq!(*events.borrow(), ["select:*", "load:first"]);
    events.borrow_mut().clear();
    let error = acquire_primary_documents(
        || Ok::<_, Infallible>("*".into()),
        PrimaryFileSelection::Direct,
        &host,
        2,
        |_, _| Ok::<_, Infallible>(()),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PrimaryAcquisitionError::DirectSelection { found: 2 }
    ));
    assert_eq!(*events.borrow(), ["select:*"]);
}

#[test]
fn invalid_request_paths_refuse_before_loading() {
    for path in [
        String::new(),
        "a\0b".into(),
        "x".repeat(document_source::MAX_SOURCE_PATH_BYTES + 1),
    ] {
        let events = RefCell::new(Vec::new());
        let host = Host {
            requests: vec![path.clone()],
            fail: None,
            events: &events,
        };
        let error = acquire_primary_documents(
            || Ok::<_, Infallible>("*".into()),
            WILDCARD,
            &host,
            1,
            |_, _| Ok::<_, Infallible>(()),
        )
        .unwrap_err();
        assert!(matches!(error, PrimaryAcquisitionError::Request { path: found } if found == path));
        assert_eq!(*events.borrow(), ["select:*"]);
    }
}

#[test]
fn assembly_preserves_shape_refusals() {
    let document = || PrimaryDocument {
        request: "input".into(),
        resolved_path: None,
        value: Instance::Group(Vec::new().into()),
    };
    assert_eq!(
        assemble_primary_documents(PrimaryDocumentShape::Rows, vec![document()]),
        Err(PrimaryAssemblyError::NotRows {
            path: "input".into()
        })
    );
    assert_eq!(
        assemble_primary_documents(PrimaryDocumentShape::Document, vec![]),
        Err(PrimaryAssemblyError::DocumentCount { found: 0 })
    );
    assert_eq!(
        assemble_primary_documents(PrimaryDocumentShape::Document, vec![document(), document()]),
        Err(PrimaryAssemblyError::DocumentCount { found: 2 })
    );
    assert_eq!(
        assemble_primary_documents(PrimaryDocumentShape::Document, vec![document()]).unwrap(),
        document().value
    );
}
