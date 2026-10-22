use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, anyhow};
use engine::{
    DynamicSourceCategory, DynamicSourceCodecKind, DynamicSourceFault, DynamicSourceLoader,
    DynamicSourcePhase, DynamicSourceResource, ExecutionBudgets, PolicyRefusal,
    RestrictedDynamicSourceError, RestrictedPolicy, RootAccess,
};
use ir::{ScalarType, SchemaNode};
use mapping::{DynamicSourcePath, NamedSource};

use super::*;
use crate::ProjectDynamicSourceLoader;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_loader_faults_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn json_source() -> NamedSource {
    NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Document",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 0,
            iteration: Vec::new(),
        }),
    }
}

fn tokens(fault: &DynamicSourceFault) -> (&'static str, &'static str) {
    (fault.phase().as_str(), fault.category().as_str())
}

#[test]
fn filesystem_reads_split_acquisition_encoding_and_document_faults() {
    let dir = TestDir::new();
    for (name, bytes) in [
        ("encoding.json", &b"{\"Value\":\"\xff\"}"[..]),
        ("syntax.json", b"{\"Value\":"),
        ("shape.json", b"\"text\""),
    ] {
        std::fs::write(dir.0.join(name), bytes).unwrap();
    }
    let sources = [json_source()];
    let loader = ProjectDynamicSourceLoader::new(&dir.0, &sources);

    for (path, expected) in [
        ("missing.json", ("acquisition", "host")),
        ("encoding.json", ("codec", "encoding")),
        ("syntax.json", ("codec", "document")),
        ("shape.json", ("codec", "document")),
        ("missing.xml", ("acquisition", "host")),
    ] {
        let fault = loader.load_tagged("document", path).unwrap_err();
        assert_eq!(tokens(&fault), expected, "{path}");
        // The legacy string boundary keeps the reader's outermost message.
        let message = loader.load("document", path).unwrap_err();
        assert_eq!(message, fault.to_string(), "{path}");
        let read = fault
            .cause()
            .and_then(|cause| cause.downcast_ref::<DynamicSourceReadError>())
            .expect("retained reader error");
        assert_eq!(message, read.error().to_string(), "{path}");
    }

    let missing = loader.load_tagged("document", "missing.json").unwrap_err();
    assert_eq!(
        missing
            .find_cause::<std::io::Error>()
            .map(std::io::Error::kind),
        Some(std::io::ErrorKind::NotFound)
    );
    let syntax = loader.load_tagged("document", "syntax.json").unwrap_err();
    assert!(syntax.find_cause::<serde_json::Error>().is_some());
    assert!(
        syntax
            .find_cause::<format_json::JsonFormatError>()
            .is_some()
    );
}

#[test]
fn undeclared_sources_are_typed_codec_faults_with_the_legacy_message() {
    let dir = TestDir::new();
    let sources = [json_source()];
    let loader = ProjectDynamicSourceLoader::new(&dir.0, &sources);
    let fault = loader.load_tagged("unknown", "a.json").unwrap_err();
    assert_eq!(tokens(&fault), ("codec", "undeclared_source"));
    assert_eq!(
        fault
            .cause()
            .and_then(|cause| cause.downcast_ref::<UndeclaredDynamicSource>()),
        Some(&UndeclaredDynamicSource {
            source_name: "unknown".into()
        })
    );
    assert_eq!(
        loader.load("unknown", "a.json").unwrap_err(),
        "dynamic source `unknown` is not declared"
    );
}

#[test]
fn restricted_refusals_stay_typed_before_any_read() {
    let dir = TestDir::new();
    let root = dir.0.join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(dir.0.join("outside.json"), "{\"Value\":\"x\"}").unwrap();
    let policy = RestrictedPolicy::new(ExecutionBudgets::new(1_000, 64, 0))
        .allow_root(&root, RootAccess::Read)
        .unwrap();
    let sources = [json_source()];
    let loader = ProjectDynamicSourceLoader::new(&root, &sources);

    let tagged = loader
        .load_restricted_tagged("document", "../outside.json", &policy)
        .unwrap_err();
    let RestrictedDynamicSourceError::Refused(refusal) = tagged else {
        panic!("unexpected {tagged:?}");
    };
    assert!(matches!(refusal, PolicyRefusal::PathOutsideRoots { .. }));
    assert_eq!(
        loader.load_restricted("document", "../outside.json", &policy),
        Err(engine::DynamicSourceLoadError::Refused(refusal))
    );

    // An admitted location still reports its read fault.
    std::fs::write(root.join("syntax.json"), "{").unwrap();
    let RestrictedDynamicSourceError::Fault(fault) = loader
        .load_restricted_tagged("document", "syntax.json", &policy)
        .unwrap_err()
    else {
        panic!("syntax error must be a fault");
    };
    assert_eq!(tokens(&fault), ("codec", "document"));
}

#[test]
fn typed_chain_evidence_decides_the_phase() {
    let invalid = vec![0xff_u8];
    let utf8 = std::str::from_utf8(&invalid).unwrap_err();
    let cases: [(anyhow::Error, Option<DynamicSourceCodecKind>); 6] = [
        (anyhow!("host transport failed"), None),
        (
            anyhow::Error::new(PolicyRefusal::HttpOriginDenied {
                origin: "https://example.test".into(),
            })
            .context("reading input"),
            None,
        ),
        (
            anyhow::Error::new(format_json::JsonFormatError::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )))
            .context("reading JSON input"),
            None,
        ),
        (
            anyhow::Error::new(std::io::Error::new(std::io::ErrorKind::InvalidData, utf8))
                .context("reading input"),
            Some(DynamicSourceCodecKind::Encoding),
        ),
        (
            anyhow::Error::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "corrupt archive",
            )),
            Some(DynamicSourceCodecKind::Document),
        ),
        (
            Err::<(), _>(format_json::JsonFormatError::NoMatchingAlternative {
                name: "Document".into(),
            })
            .context("reading JSON input")
            .unwrap_err(),
            Some(DynamicSourceCodecKind::Document),
        ),
    ];
    for (error, expected) in cases {
        let rendered = error.to_string();
        assert_eq!(codec_kind(&error), expected, "{rendered}");
        assert_eq!(read_fault(error).to_string(), rendered);
    }
}

/// Every engine token equals the generated Rust runtime's token for the same
/// phase and category; the exhaustive matches fail to compile when either
/// side adds a variant without the other.
#[test]
fn projection_tokens_match_the_generated_runtime() {
    use codegen_runtime::{
        DynamicSourceCategory as RuntimeCategory, DynamicSourcePhase as RuntimePhase,
    };
    let phase = |phase| match phase {
        DynamicSourcePhase::Request => RuntimePhase::Request,
        DynamicSourcePhase::Acquisition => RuntimePhase::Acquisition,
        DynamicSourcePhase::Codec => RuntimePhase::Codec,
        DynamicSourcePhase::Resource => RuntimePhase::Resource,
    };
    let category = |category| match category {
        DynamicSourceCategory::MissingLoader => RuntimeCategory::MissingLoader,
        DynamicSourceCategory::PathType => RuntimeCategory::PathType,
        DynamicSourceCategory::PathTooLong => RuntimeCategory::PathTooLong,
        DynamicSourceCategory::TooManyRequests => RuntimeCategory::TooManyRequests,
        DynamicSourceCategory::Host => RuntimeCategory::Host,
        DynamicSourceCategory::Encoding => RuntimeCategory::Encoding,
        DynamicSourceCategory::Document => RuntimeCategory::Document,
        DynamicSourceCategory::UndeclaredSource => RuntimeCategory::UndeclaredSource,
        DynamicSourceCategory::DocumentBytes => RuntimeCategory::DocumentBytes,
        DynamicSourceCategory::TotalBytes => RuntimeCategory::TotalBytes,
        DynamicSourceCategory::RequestCount => RuntimeCategory::RequestCount,
    };
    let _exhaustive = |runtime| match runtime {
        RuntimeCategory::MissingLoader
        | RuntimeCategory::PathType
        | RuntimeCategory::PathTooLong
        | RuntimeCategory::TooManyRequests
        | RuntimeCategory::Host
        | RuntimeCategory::Encoding
        | RuntimeCategory::Document
        | RuntimeCategory::UndeclaredSource
        | RuntimeCategory::DocumentBytes
        | RuntimeCategory::TotalBytes
        | RuntimeCategory::RequestCount => (),
    };
    for value in [
        DynamicSourcePhase::Request,
        DynamicSourcePhase::Acquisition,
        DynamicSourcePhase::Codec,
        DynamicSourcePhase::Resource,
    ] {
        assert_eq!(value.as_str(), phase(value).as_str());
    }
    for value in [
        DynamicSourceCategory::MissingLoader,
        DynamicSourceCategory::PathType,
        DynamicSourceCategory::PathTooLong,
        DynamicSourceCategory::TooManyRequests,
        DynamicSourceCategory::Host,
        DynamicSourceCategory::Encoding,
        DynamicSourceCategory::Document,
        DynamicSourceCategory::UndeclaredSource,
        DynamicSourceCategory::DocumentBytes,
        DynamicSourceCategory::TotalBytes,
        DynamicSourceCategory::RequestCount,
    ] {
        assert_eq!(value.as_str(), category(value).as_str());
    }

    // Faults with the same kind project to the same tokens in both runtimes.
    let engine = [
        DynamicSourceFault::legacy("host".into()),
        DynamicSourceFault::codec(DynamicSourceCodecKind::Encoding, "bytes"),
        DynamicSourceFault::codec(DynamicSourceCodecKind::Document, "shape"),
        DynamicSourceFault::codec(DynamicSourceCodecKind::UndeclaredSource, "endpoint"),
        DynamicSourceFault::resource(DynamicSourceResource::DocumentBytes, 2, 1),
        DynamicSourceFault::resource(DynamicSourceResource::TotalBytes, 2, 1),
        DynamicSourceFault::resource(DynamicSourceResource::RequestCount, 2, 1),
    ];
    let runtime = [
        codegen_runtime::DynamicSourceFault::legacy("host".into()),
        codegen_runtime::DynamicSourceFault::codec(
            codegen_runtime::DynamicSourceCodecKind::Encoding,
            "bytes",
        ),
        codegen_runtime::DynamicSourceFault::codec(
            codegen_runtime::DynamicSourceCodecKind::Document,
            "shape",
        ),
        codegen_runtime::DynamicSourceFault::undeclared_source("endpoint"),
        codegen_runtime::DynamicSourceFault::resource(
            codegen_runtime::DynamicSourceResource::DocumentBytes,
            2,
            1,
        ),
        codegen_runtime::DynamicSourceFault::resource(
            codegen_runtime::DynamicSourceResource::TotalBytes,
            2,
            1,
        ),
        codegen_runtime::DynamicSourceFault::resource(
            codegen_runtime::DynamicSourceResource::RequestCount,
            2,
            1,
        ),
    ];
    for (engine, runtime) in engine.iter().zip(&runtime) {
        assert_eq!(
            (engine.phase().as_str(), engine.category().as_str()),
            (runtime.phase().as_str(), runtime.category().as_str())
        );
        assert_eq!(
            engine
                .resource_limit()
                .map(|limit| (limit.observed, limit.limit)),
            runtime
                .resource_limit()
                .map(|limit| (limit.observed, limit.limit))
        );
    }
}
