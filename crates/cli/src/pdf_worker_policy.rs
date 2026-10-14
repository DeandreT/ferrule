use anyhow::Context;
use pdf_worker::host::{
    BeginFailure, CancellationFlag, ExclusiveReaperAdmission, JobProgress, PdfWorkerManager,
    ScratchPolicy, WorkerProgram,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Explicit opt-in for filesystem project runs. Acquisition stays lazy for
/// secondary/dynamic sources; the existing primary is always read.
pub struct FilesystemRunPolicy {
    manager: RefCell<Option<PdfWorkerManager>>,
    cancel: CancellationFlag,
    retain_pdf_evidence: bool,
    retained_pdf_evidence: RefCell<Vec<PathBuf>>,
}
impl Default for FilesystemRunPolicy {
    fn default() -> Self {
        Self::new()
    }
}
impl FilesystemRunPolicy {
    pub fn new() -> Self {
        Self {
            manager: RefCell::new(None),
            cancel: CancellationFlag::new(),
            retain_pdf_evidence: false,
            retained_pdf_evidence: RefCell::new(Vec::new()),
        }
    }
    /// Opt in before the first lazy PDF acquisition. Ordinary policies clean
    /// successful worker originals; failure evidence is retained in both modes.
    pub fn with_retained_pdf_evidence() -> Self {
        Self {
            retain_pdf_evidence: true,
            ..Self::new()
        }
    }
    /// Closed successful PDF folders, in acquisition order. These paths remain
    /// usable after this policy is dropped; this accessor does not delete them.
    pub fn retained_pdf_evidence(&self) -> Vec<PathBuf> {
        self.retained_pdf_evidence.borrow().clone()
    }
    pub fn cancellation(&self) -> &CancellationFlag {
        &self.cancel
    }
    pub(crate) fn read_pdf(
        &self,
        path: &Path,
        layout: &mapping::PdfLayout,
    ) -> anyhow::Result<ir::Instance> {
        if let Some(dependency) = layout.repair_dependency() {
            return Err(format_pdf::PdfError::RepairDependency(dependency).into());
        }
        let mut guard = match super::pdf_worker_signal::PdfSignalGuard::begin() {
            Ok(guard) => guard,
            Err(mut error) => {
                // A partially installed CLI signal policy is retained until
                // restored, even though no worker has been launched.
                while error.needs_recovery() {
                    if error.retry_after_no_child().is_err() {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
                return Err(error.into());
            }
        };
        let mut retained_success = None;
        let original = (|| -> anyhow::Result<ir::Instance> {
            guard.observe(&self.cancel);
            if self.cancel.is_cancelled() {
                return Err(pdf_worker::host::Failure {
                    kind: pdf_worker::host::FailureKind::Cancelled,
                    operation: "cancelled before CLI worker admission",
                    diagnostics: pdf_worker::Diagnostics::empty(),
                    errno: None,
                }
                .into());
            }
            let mut slot = self
                .manager
                .try_borrow_mut()
                .context("reentrant PDF worker acquisition")?;
            if slot.is_none() {
                let exe = std::env::current_exe().context("resolve packaged CLI executable")?;
                let program = WorkerProgram::admit_packaged_sibling(&exe)?;
                // Zero is this CLI caller's explicit external reserve. This
                // is not a build/global reservation or disk quota.
                let scratch = ScratchPolicy::admit(&std::env::temp_dir(), 0)?;
                let mut manager = PdfWorkerManager::new(
                    program,
                    scratch,
                    ExclusiveReaperAdmission::for_current_host(),
                )?;
                if self.retain_pdf_evidence {
                    manager.retain_successful_originals(true)?;
                }
                *slot = Some(manager);
            }
            let manager = slot.as_mut().expect("admitted manager");
            match manager.begin(path, layout, &self.cancel) {
                Ok(_) => {}
                Err(BeginFailure::NoChild(error)) => return Err(error.into()),
                Err(BeginFailure::OwnedStopping { .. }) => {}
            }
            let mut pending_notice = false;
            loop {
                guard.observe(&self.cancel);
                match manager.poll() {
                    JobProgress::Pending { .. } => {}
                    JobProgress::Succeeded { instance, .. } => {
                        if self.retain_pdf_evidence {
                            retained_success =
                                Some(
                                    manager.evidence_paths().last().cloned().context(
                                        "closed successful PDF evidence path is missing",
                                    )?,
                                );
                        }
                        return Ok(instance);
                    }
                    JobProgress::FailedClosed { failure, retained } => {
                        return Err(anyhow::Error::new(*failure).context(format!(
                            "closed PDF worker evidence retained at {}",
                            retained.display()
                        )));
                    }
                    JobProgress::ClosureIncomplete { snapshot } => {
                        if !pending_notice {
                            eprintln!(
                                "PDF worker is stopping; closure remains unresolved. Evidence: {}",
                                snapshot.evidence.display()
                            );
                            pending_notice = true;
                        }
                    }
                }
                // Acceptance can expire without physical disappearance. Never
                // drop/flatten the still-owned manager into a terminal error.
                std::thread::sleep(Duration::from_millis(250));
            }
        })();
        // No worker ownership remains at this boundary. Restoration failures
        // are kept on the same guard and retried without growing a log.
        guard.observe(&self.cancel);
        let mut restoration_error = None;
        let mut restoration_attempts = 0u64;
        loop {
            match guard.finish_after_closed() {
                Ok(()) => break,
                Err(error) => {
                    restoration_attempts = restoration_attempts.saturating_add(1);
                    if restoration_error.is_none() {
                        restoration_error = Some(error);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
        guard.observe(&self.cancel);
        // This records a closed extraction even when the later CLI boundary
        // refuses mapping publication after cancellation or recovered restoration.
        if let Some(path) = retained_success {
            self.retained_pdf_evidence.borrow_mut().push(path.clone());
        }
        if let Some(error) = restoration_error {
            return Err(anyhow::Error::new(error).context(format!("PDF signal restoration recovered after {restoration_attempts} failed attempts; extraction is not published")));
        }
        if self.cancel.is_cancelled() && original.is_ok() {
            return Err(anyhow::anyhow!(
                "PDF worker cancellation observed at closed CLI return boundary"
            ));
        }
        original
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod tests {
    use super::*;
    use engine::DynamicSourceLoader;
    use mapping::{
        DynamicSourcePath, FormatOptions, NamedSource, PdfCapture, PdfCommand, PdfLayout,
        PdfPageSelection, PdfRegion,
    };
    use std::os::unix::fs::PermissionsExt;
    #[test]
    #[ignore = "root-only real dynamic PDF cache; separately qualified paired package and fresh evidence root"]
    fn b34_cache_keys_and_failed_load_are_not_partial_cache() {
        let root = std::path::PathBuf::from(
            std::env::var_os("FERRULE_PDF_WORKER_DYNAMIC_EVIDENCE_ROOT")
                .expect("fresh root-qualified evidence root"),
        );
        assert!(root.is_absolute() && !root.exists());
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let package = std::path::PathBuf::from(
            std::env::var_os("FERRULE_PDF_WORKER_PACKAGE_DIR")
                .expect("root-qualified immutable paired package directory"),
        );
        let layout = PdfLayout::new(
            "Document",
            PdfPageSelection::First,
            vec![PdfCommand::Capture(PdfCapture {
                name: "Value".into(),
                region: PdfRegion::full(),
                algorithm: Default::default(),
            })],
        )
        .unwrap();
        let source = NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: layout.schema(),
            options: FormatOptions {
                pdf: Some(layout),
                ..FormatOptions::default()
            },
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: Vec::new(),
            }),
        };
        for name in ["one.pdf", "two.pdf"] {
            std::fs::write(
                root.join(name),
                include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
            )
            .unwrap();
        }
        let program = WorkerProgram::admit_packaged_sibling(&package.join("ferrule")).unwrap();
        let mut manager = PdfWorkerManager::new(
            program,
            ScratchPolicy::admit(&root, 0).unwrap(),
            ExclusiveReaperAdmission::for_current_host(),
        )
        .unwrap();
        manager.retain_successful_originals(true).unwrap();
        let policy = FilesystemRunPolicy::new();
        *policy.manager.borrow_mut() = Some(manager);
        let sources = [source];
        let loader = crate::ProjectDynamicSourceLoader::with_policy(&root, &sources, Some(&policy));
        let expected = ir::Instance::Group(
            vec![(
                "Value".into(),
                ir::Instance::Scalar(ir::Value::String("Hello".into())),
            )]
            .into(),
        );
        std::fs::write(root.join("EXPECTED.original.txt"), format!("{expected:#?}")).unwrap();
        for (key, path, expected_jobs) in [
            ("first", "one.pdf", 1),
            ("same", "one.pdf", 1),
            ("different", "two.pdf", 2),
        ] {
            let result = loader.load("document", path);
            std::fs::write(
                root.join(format!("{key}.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            assert_eq!(*result.unwrap(), expected);
            assert_eq!(
                policy
                    .manager
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .evidence_paths()
                    .len(),
                expected_jobs
            );
        }
        let failed = loader.load("document", "absent.pdf");
        std::fs::write(root.join("failed.original.txt"), format!("{failed:#?}")).unwrap();
        assert!(failed.is_err());
        assert!(
            !loader
                .cache
                .borrow()
                .contains_key(&("document".into(), "absent.pdf".into()))
        );
        assert!(
            loader
                .cache
                .borrow()
                .contains_key(&("document".into(), "one.pdf".into()))
        );
        assert!(
            loader
                .cache
                .borrow()
                .contains_key(&("document".into(), "two.pdf".into()))
        );
    }

    #[test]
    #[ignore = "root-only retained-success accessor/cache/drop; immutable paired package and fresh evidence root"]
    fn retained_success_query_survives_policy_drop_without_duplicate_cached_reads() {
        use std::io::Write;
        use std::os::unix::ffi::OsStrExt;
        fn retain(path: &std::path::Path, bytes: &[u8]) {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            file.write_all(bytes).unwrap();
            file.sync_all().unwrap();
            std::fs::File::open(path.parent().unwrap())
                .unwrap()
                .sync_all()
                .unwrap();
        }
        let root = PathBuf::from(
            std::env::var_os("FERRULE_PDF_WORKER_EVIDENCE_API_ROOT")
                .expect("root-qualified fresh absolute evidence directory"),
        );
        assert!(root.is_absolute() && !root.exists());
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let scratch = root.join("scratch");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
        let package = PathBuf::from(
            std::env::var_os("FERRULE_PDF_WORKER_PACKAGE_DIR")
                .expect("root-qualified immutable paired package directory"),
        );
        let layout = PdfLayout::new(
            "Document",
            PdfPageSelection::First,
            vec![PdfCommand::Capture(PdfCapture {
                name: "Value".into(),
                region: PdfRegion::full(),
                algorithm: Default::default(),
            })],
        )
        .unwrap();
        let source = NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: layout.schema(),
            options: FormatOptions {
                pdf: Some(layout),
                ..FormatOptions::default()
            },
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: Vec::new(),
            }),
        };
        retain(
            &root.join("hello.pdf"),
            include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
        );
        let expected = ir::Instance::Group(
            vec![(
                "Value".into(),
                ir::Instance::Scalar(ir::Value::String("Hello".into())),
            )]
            .into(),
        );
        retain(
            &root.join("EXPECTED.original.txt"),
            format!("{expected:#?}").as_bytes(),
        );
        let program = WorkerProgram::admit_packaged_sibling(&package.join("ferrule")).unwrap();
        let mut manager = PdfWorkerManager::new(
            program,
            ScratchPolicy::admit(&scratch, 0).unwrap(),
            ExclusiveReaperAdmission::for_current_host(),
        )
        .unwrap();
        manager.retain_successful_originals(true).unwrap();
        let policy = FilesystemRunPolicy::with_retained_pdf_evidence();
        // Private test injection changes no public API or worker admission.
        *policy.manager.borrow_mut() = Some(manager);
        let sources = [source];
        let loader = crate::ProjectDynamicSourceLoader::with_policy(&root, &sources, Some(&policy));
        let first = loader.load("document", "hello.pdf");
        retain(
            &root.join("first.original.txt"),
            format!("{first:#?}").as_bytes(),
        );
        let same = loader.load("document", "hello.pdf");
        retain(
            &root.join("cached.original.txt"),
            format!("{same:#?}").as_bytes(),
        );
        let paths = policy.retained_pdf_evidence();
        let manager_paths = policy.manager.borrow().as_ref().unwrap().evidence_paths();
        retain(&root.join("query.original.json"), &serde_json::to_vec_pretty(&serde_json::json!({
            "paths_bytes": paths.iter().map(|p| p.as_os_str().as_bytes()).collect::<Vec<_>>(),
            "manager_paths_bytes": manager_paths.iter().map(|p| p.as_os_str().as_bytes()).collect::<Vec<_>>()
        })).unwrap());
        drop(loader);
        drop(policy);
        let mut folders = Vec::new();
        for path in &paths {
            let mut leaves = std::fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect::<Vec<_>>();
            leaves.sort();
            let mut files = std::collections::BTreeMap::new();
            let mut raw_files = Vec::new();
            for leaf in leaves {
                let metadata = std::fs::symlink_metadata(&leaf).unwrap();
                retain(
                    &root.join(format!(
                        "folder-{}-file-{}.metadata.original.txt",
                        folders.len(),
                        raw_files.len()
                    )),
                    format!("{leaf:?} {metadata:?}").as_bytes(),
                );
                assert!(metadata.is_file() && !metadata.file_type().is_symlink());
                let body = std::fs::read(&leaf).unwrap();
                let name = leaf.file_name().unwrap().to_owned();
                raw_files.push(serde_json::json!({"name_bytes": name.as_bytes(), "body": body}));
                files.insert(name, body);
            }
            retain(
                &root.join(format!("folder-{}.full.original.json", folders.len())),
                &serde_json::to_vec_pretty(&raw_files).unwrap(),
            );
            folders.push(files);
        }
        retain(&root.join("after-drop.original.json"), &serde_json::to_vec_pretty(&serde_json::json!({
            "folder_count": folders.len(), "paths_bytes": paths.iter().map(|p| p.as_os_str().as_bytes()).collect::<Vec<_>>()
        })).unwrap());
        assert_eq!(*first.unwrap(), expected);
        assert_eq!(*same.unwrap(), expected);
        assert_eq!(paths.len(), 1);
        assert_eq!(manager_paths, paths);
        assert_eq!(folders.len(), 1);
        let files = &folders[0];
        for name in [
            "input.pdf",
            "request.json",
            "result.json",
            "instance.json",
            "receipt.json",
        ] {
            assert!(
                files.contains_key(std::ffi::OsStr::new(name)),
                "missing {name}"
            );
        }
        assert!(!files.contains_key(std::ffi::OsStr::new("layout.json")));
        assert_eq!(
            files[std::ffi::OsStr::new("input.pdf")],
            include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf").as_slice()
        );
        let request: serde_json::Value =
            serde_json::from_slice(&files[std::ffi::OsStr::new("request.json")]).unwrap();
        assert!(request.get("layout").is_some());
        let receipt: serde_json::Value =
            serde_json::from_slice(&files[std::ffi::OsStr::new("receipt.json")]).unwrap();
        assert_eq!(receipt["own_reap"], true);
        assert_eq!(receipt["successful_originals_retained"], true);
    }
}

impl crate::FilesystemPdfReader for FilesystemRunPolicy {
    fn read_pdf(&self, path: &Path, layout: &mapping::PdfLayout) -> anyhow::Result<ir::Instance> {
        FilesystemRunPolicy::read_pdf(self, path, layout)
    }
}
