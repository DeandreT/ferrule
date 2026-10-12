//! Private C13-C16 controls from the unchanged frozen reader-route catalog.
//! These synthetic borrowed-reader controls do not qualify a worker or kernel boundary.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::Instance;
use serde_json::{Value, json};

static NEXT_DYNAMIC_COHORT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
enum ScriptedPdfReaderError {
    Denied,
    Exhausted,
}

impl fmt::Display for ScriptedPdfReaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied => formatter.write_str("scripted PDF reader refusal: denied"),
            Self::Exhausted => formatter.write_str("scripted PDF reader reply queue exhausted"),
        }
    }
}

impl Error for ScriptedPdfReaderError {}

enum Reply {
    Ok(Instance),
    Denied,
}

struct BorrowedScriptReader {
    // Rc deliberately makes this ordinary borrowed implementation !Send and !Sync.
    replies: Rc<RefCell<VecDeque<Reply>>>,
    observations: Rc<RefCell<Vec<Value>>>,
    evidence: PathBuf,
}

fn origin_facts(instance: &Instance) -> Value {
    match instance {
        Instance::Scalar(_) => json!({"kind": "Scalar"}),
        Instance::Group(fields) => json!({
            "kind": "Group", "origin": match fields.xml_type_origin() {
                ir::XmlTypeOrigin::Unknown => json!({"kind": "Unknown"}),
                ir::XmlTypeOrigin::Absent => json!({"kind": "Absent"}),
                ir::XmlTypeOrigin::Explicit(identity) => json!({"kind": "Explicit", "identity": identity}),
                ir::XmlTypeOrigin::ExplicitPadded { literal, resolved_identity } =>
                    json!({"kind": "ExplicitPadded", "literal": literal, "resolved_identity": resolved_identity}),
            },
            "fields": fields.iter().map(|(name, child)| {
                json!({"name": name, "child": origin_facts(child)})
            }).collect::<Vec<_>>()
        }),
        Instance::Repeated(children) => json!({"kind": "Repeated", "children":
            children.iter().map(origin_facts).collect::<Vec<_>>() }),
        Instance::MappedSequence(children) => json!({"kind": "MappedSequence", "children":
            children.iter().map(origin_facts).collect::<Vec<_>>() }),
        Instance::DocumentSet(_) => {
            json!({"kind": "DocumentSet", "debug": format!("{instance:?}")})
        }
    }
}

fn write_original(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("original has a parent")).unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn write_json(path: &Path, value: &Value) {
    write_original(path, &serde_json::to_vec_pretty(value).unwrap());
}

impl crate::FilesystemPdfReader for BorrowedScriptReader {
    fn read_pdf(&self, path: &Path, layout: &mapping::PdfLayout) -> anyhow::Result<Instance> {
        let reply = self.replies.borrow_mut().pop_front();
        let (result, reply_record) = match reply {
            Some(Reply::Ok(instance)) => {
                let record = json!({"Ok": serde_json::to_value(&instance).unwrap(),
                    "debug": format!("{instance:?}"), "origin_facts": origin_facts(&instance)});
                (Ok(instance), record)
            }
            Some(Reply::Denied) => {
                let error = ScriptedPdfReaderError::Denied;
                let record = json!({"Err": {"type": "ScriptedPdfReaderError", "variant": "Denied",
                    "display": error.to_string(), "debug": format!("{error:?}"),
                    "source": error.source().map(ToString::to_string), "ordered_source_chain": []}});
                (Err(anyhow::Error::new(error)), record)
            }
            None => {
                let error = ScriptedPdfReaderError::Exhausted;
                let record = json!({"Err": {"type": "ScriptedPdfReaderError", "variant": "Exhausted",
                    "display": error.to_string(), "debug": format!("{error:?}"),
                    "source": error.source().map(ToString::to_string), "ordered_source_chain": []}});
                (Err(anyhow::Error::new(error)), record)
            }
        };
        let mut observations = self.observations.borrow_mut();
        observations.push(json!({"method": "read_pdf", "path": path.to_string_lossy(),
            "path_debug": format!("{path:?}"), "layout": serde_json::to_value(layout).unwrap(),
            "reply": reply_record}));
        // Persist the entire actual request/reply sequence before returning to the loader.
        write_json(
            &self
                .evidence
                .join(format!("reader-call-{:02}.json", observations.len())),
            &json!({"observations": &*observations}),
        );
        result
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/filesystem_pdf_reader")
}

fn frozen_bytes(root: &Path, reference: &str) -> Vec<u8> {
    let relative = reference
        .strip_prefix("fixtures/")
        .expect("frozen fixture prefix");
    assert!(!Path::new(relative).is_absolute());
    assert!(
        !Path::new(relative)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    );
    fs::read(root.join(relative)).unwrap()
}

fn expected_instance(root: &Path, reference: &str) -> Instance {
    serde_json::from_slice(&frozen_bytes(root, reference)).unwrap()
}

fn fresh_evidence_root() -> PathBuf {
    let base = match std::env::var_os("FERRULE_FILESYSTEM_PDF_READER_EVIDENCE_ROOT") {
        Some(root) => {
            let root = PathBuf::from(root);
            assert!(root.is_absolute(), "reader evidence base must be absolute");
            root
        }
        None => std::env::temp_dir(),
    };
    let serial = NEXT_DYNAMIC_COHORT.fetch_add(1, Ordering::Relaxed);
    let root = base
        .join("private-dynamic")
        .join(format!("pid-{}-cohort-{serial}", std::process::id()));
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    fs::create_dir(&root).unwrap();
    root
}

#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    kind: &'static str,
    bytes: Vec<u8>,
}

fn capture_work(root: &Path, originals: &Path) -> BTreeMap<PathBuf, FileSnapshot> {
    fn visit(
        root: &Path,
        directory: &Path,
        originals: &Path,
        snapshots: &mut BTreeMap<PathBuf, FileSnapshot>,
    ) {
        let mut entries = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let (kind, bytes) = if metadata.is_file() {
                let bytes = fs::read(&path).unwrap();
                write_original(&originals.join("files").join(&relative), &bytes);
                ("regular", bytes)
            } else if metadata.is_dir() {
                ("directory", Vec::new())
            } else if metadata.file_type().is_symlink() {
                (
                    "symlink",
                    format!("{:?}", fs::read_link(&path).unwrap()).into_bytes(),
                )
            } else {
                ("nonregular", format!("{metadata:?}").into_bytes())
            };
            snapshots.insert(relative, FileSnapshot { kind, bytes });
            if metadata.is_dir() {
                visit(root, &path, originals, snapshots);
            }
        }
    }
    fs::create_dir_all(originals).unwrap();
    let mut snapshots = BTreeMap::new();
    visit(root, root, originals, &mut snapshots);
    write_json(
        &originals.join("whole-cohort.json"),
        &json!({"entries": snapshots.iter().map(|(path, snapshot)|
        json!({"path": path.to_string_lossy(), "kind": snapshot.kind, "bytes": snapshot.bytes})
    ).collect::<Vec<_>>() }),
    );
    snapshots
}

fn returned_record(returned: &[Result<Arc<Instance>, String>]) -> Value {
    json!({"complete_debug": format!("{returned:?}"), "returns": returned.iter().map(|result| match result {
        Ok(instance) => json!({"Ok": serde_json::to_value(instance.as_ref()).unwrap(),
            "debug": format!("{instance:?}"), "origin_facts": origin_facts(instance)}),
        Err(error) => json!({"Err": error, "display": error, "debug": format!("{error:?}")}),
    }).collect::<Vec<_>>() })
}

fn assert_unknown_origins(instance: &Instance) {
    match instance {
        Instance::Group(fields) => {
            assert!(matches!(
                fields.xml_type_origin(),
                ir::XmlTypeOrigin::Unknown
            ));
            for (_, child) in fields.iter() {
                assert_unknown_origins(child);
            }
        }
        Instance::Repeated(children) | Instance::MappedSequence(children) => {
            for child in children {
                assert_unknown_origins(child);
            }
        }
        Instance::Scalar(_) | Instance::DocumentSet(_) => {}
    }
}

#[test]
fn frozen_c13_c16_private_borrowed_dynamic_loader_controls() {
    let fixtures = fixture_root();
    let catalog_bytes =
        fs::read(fixtures.join("FROZEN_26_COMPLETE_READER_ROUTE_CONTROLS.json")).unwrap();
    let catalog: Value = serde_json::from_slice(&catalog_bytes).unwrap();
    let evidence = fresh_evidence_root();
    write_original(
        &evidence.join("frozen-catalog.original.json"),
        &catalog_bytes,
    );
    for id in ["C13", "C14", "C15", "C16"] {
        let case = catalog["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"].as_str() == Some(id))
            .unwrap();
        let case_evidence = evidence.join(id);
        fs::create_dir(&case_evidence).unwrap();
        let work = case_evidence.join("work");
        fs::create_dir(&work).unwrap();
        let model_bytes = frozen_bytes(&fixtures, case["model"].as_str().unwrap());
        write_original(&work.join("project.json"), &model_bytes);
        let project: mapping::Project = serde_json::from_slice(&model_bytes).unwrap();
        for (name, reference) in case["initial_files"].as_object().unwrap() {
            write_original(
                &work.join(name),
                &frozen_bytes(&fixtures, reference.as_str().unwrap()),
            );
        }
        let before = capture_work(&work, &case_evidence.join("before"));
        let script = case["scripted_reader_requests_in_order"]
            .as_array()
            .unwrap();
        let replies = script
            .iter()
            .map(|request| {
                if let Some(reference) = request["reply"]["Ok"].as_str() {
                    Reply::Ok(expected_instance(&fixtures, reference))
                } else {
                    Reply::Denied
                }
            })
            .collect::<VecDeque<_>>();
        let observations = Rc::new(RefCell::new(Vec::new()));
        let reader = BorrowedScriptReader {
            replies: Rc::new(RefCell::new(replies)),
            observations: Rc::clone(&observations),
            evidence: case_evidence.clone(),
        };
        let loader = crate::ProjectDynamicSourceLoader::with_pdf_reader(
            &work,
            &project.extra_sources,
            Some(&reader),
        );
        let mut returned = Vec::new();
        let mut after_calls = Vec::new();
        for (index, call) in case["invocation"]["load_calls"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            returned.push(engine::DynamicSourceLoader::load(
                &loader,
                call[0].as_str().unwrap(),
                call[1].as_str().unwrap(),
            ));
            // Preserve all raw returns and the whole model/input/publication cohort first.
            write_json(
                &case_evidence.join(format!("returned-after-call-{:02}.json", index + 1)),
                &returned_record(&returned),
            );
            after_calls.push(capture_work(
                &work,
                &case_evidence.join(format!("after-call-{:02}", index + 1)),
            ));
        }
        write_json(
            &case_evidence.join("all-observations-before-comparison.json"),
            &json!({"reader": &*observations.borrow(), "returned": returned_record(&returned),
                "remaining_script_replies": reader.replies.borrow().len()}),
        );

        // Oracles below come from independent unchanged literal fixtures and catalog rows.
        let actual_requests = observations.borrow();
        assert_eq!(
            actual_requests.len(),
            script.len(),
            "{id} complete call count"
        );
        for (actual, expected) in actual_requests.iter().zip(script) {
            assert_eq!(actual["method"], expected["method"], "{id} method");
            let path = expected["path"]
                .as_str()
                .unwrap()
                .replace("@ROOT@", work.to_str().unwrap());
            assert_eq!(actual["path"], path, "{id} exact request path");
            let layout: Value = serde_json::from_slice(&frozen_bytes(
                &fixtures,
                expected["layout"].as_str().unwrap(),
            ))
            .unwrap();
            assert_eq!(actual["layout"], layout, "{id} full layout");
            if let Some(reference) = expected["reply"]["Ok"].as_str() {
                let reply: Value =
                    serde_json::from_slice(&frozen_bytes(&fixtures, reference)).unwrap();
                assert_eq!(actual["reply"]["Ok"], reply, "{id} full injected reply");
            } else {
                for field in ["type", "variant", "display", "source"] {
                    assert_eq!(
                        actual["reply"]["Err"][field], expected["reply"]["Err"][field],
                        "{id} full typed cause {field}"
                    );
                }
                assert_eq!(
                    actual["reply"]["Err"]["ordered_source_chain"],
                    json!([]),
                    "{id} cause chain"
                );
            }
        }
        assert!(
            reader.replies.borrow().is_empty(),
            "{id} all declared replies used"
        );
        let expected_returns = case["expected"]["returns"].as_array().unwrap();
        assert_eq!(
            returned.len(),
            expected_returns.len(),
            "{id} whole return count"
        );
        for (actual, expected) in returned.iter().zip(expected_returns) {
            if let Some(reference) = expected["Ok"].as_str() {
                let expected = expected_instance(&fixtures, reference);
                let actual = actual.as_ref().unwrap();
                assert_eq!(actual.as_ref(), &expected, "{id} entire returned Instance");
                assert_unknown_origins(actual);
            } else {
                let error = expected["Err"]
                    .as_str()
                    .unwrap()
                    .replace("@ROOT@", work.to_str().unwrap());
                assert_eq!(
                    actual.as_ref().unwrap_err(),
                    &error,
                    "{id} complete returned String"
                );
            }
        }
        if case["expected"]["same_arc_identity"] == json!(true) {
            assert!(
                Arc::ptr_eq(returned[0].as_ref().unwrap(), returned[1].as_ref().unwrap()),
                "{id} same cached allocation"
            );
        }
        if id == "C14" || id == "C15" {
            assert!(
                !Arc::ptr_eq(returned[0].as_ref().unwrap(), returned[1].as_ref().unwrap()),
                "{id} distinct key allocations"
            );
        }
        for after in after_calls {
            assert_eq!(
                after, before,
                "{id} complete original bytes and no publication at every return"
            );
        }
        assert_eq!(case["expected"]["filesystem_publication"], json!([]));
        assert_eq!(case["unexpected_files_expected"], json!([]));
        assert_eq!(case["expected"]["all_initial_files_unchanged"], json!(true));
        assert_eq!(case["model_bytes_preserved"], json!(true));
    }
    // Keep the complete before/after evidence; no automatic cleanup is permitted here.
}
