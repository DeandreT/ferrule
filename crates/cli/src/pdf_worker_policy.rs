use anyhow::Context;
use pdf_worker::host::{
    BeginFailure, CancellationFlag, ExclusiveReaperAdmission, JobProgress, PdfWorkerManager,
    ScratchPolicy, WorkerProgram,
};
use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

/// Explicit opt-in for filesystem project runs. Acquisition stays lazy for
/// secondary/dynamic sources; the existing primary is always read.
pub struct FilesystemRunPolicy {
    manager: RefCell<Option<PdfWorkerManager>>,
    cancel: CancellationFlag,
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
        }
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
                *slot = Some(PdfWorkerManager::new(
                    program,
                    scratch,
                    ExclusiveReaperAdmission::for_current_host(),
                )?);
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
                    JobProgress::Succeeded { instance, .. } => return Ok(instance),
                    JobProgress::FailedClosed { failure, retained } => {
                        return Err(anyhow::Error::new(failure).context(format!(
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
}

impl crate::FilesystemPdfReader for FilesystemRunPolicy {
    fn read_pdf(&self, path: &Path, layout: &mapping::PdfLayout) -> anyhow::Result<ir::Instance> {
        FilesystemRunPolicy::read_pdf(self, path, layout)
    }
}
