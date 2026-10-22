//! One extraction in a fresh trial process, so lifetime high-water values
//! belong to that trial alone.
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use pdf_worker::host::{
    BeginFailure, CancellationFlag, ClosedReceipt, ExclusiveReaperAdmission, Failure, FailureKind,
    JobProgress, PdfWorkerManager, ScratchPolicy, WorkerProgram,
};

use crate::fixtures::{Expected, Fixture};
use crate::record::{
    ChildUsage, Memory, Mode, Outcome, Trial, Usage, WorkerObservation, descriptors,
    exit_and_signal, memory, micros, self_usage,
};

const BALLAST_BYTES: usize = 256 * 1024 * 1024;
const AS_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
/// Generous outer bound for one owned job; the owner's own cutoffs are 30+5 s.
const CLOSURE_BOUND: Duration = Duration::from_secs(120);
const STDERR_EXCERPT_BYTES: u64 = 512;
const OBSERVED_WIRE_BYTES: usize = 1024;

pub struct Request<'a> {
    pub fixture: &'static Fixture,
    pub mode: Mode,
    pub repeat: u32,
    pub input: &'a Path,
    pub package: &'a Path,
}

/// The typed instance and its complete expectation check.
fn instance_outcome(
    instance: &ir::Instance,
    expected: &Expected,
) -> Result<(Outcome, Option<bool>)> {
    let mut wire = Vec::new();
    let encoded = pdf_worker::encode_instance(instance, &mut wire)?;
    let matches = match expected {
        Expected::Instance(expected) => Some(expected == instance),
        Expected::PdfError(_) => Some(false),
        Expected::Observed => None,
    };
    let observed_wire = (matches.is_none() && wire.len() <= OBSERVED_WIRE_BYTES)
        .then(|| String::from_utf8_lossy(&wire).into_owned());
    let outcome = Outcome::Instance {
        byte_len: encoded.byte_len,
        sha256: encoded.sha256.to_hex(),
        observed_wire,
    };
    Ok((outcome, matches))
}

fn error_outcome(variant: String, display: String, expected: &Expected) -> (Outcome, Option<bool>) {
    let matches = match expected {
        Expected::PdfError(name) => Some(*name == variant),
        Expected::Instance(_) => Some(false),
        Expected::Observed => None,
    };
    (Outcome::PdfError { variant, display }, matches)
}

fn library_variant(error: &format_pdf::PdfError) -> String {
    let debug = format!("{error:?}");
    debug.split('(').next().unwrap_or_default().to_owned()
}

pub fn run(request: Request<'_>) -> Result<Trial> {
    let layout = request.fixture.layout.layout()?;
    let expected = (request.fixture.expected)();
    let memory_before = memory()?;
    let mut trial = match request.mode {
        Mode::InProcess | Mode::InProcessAs512 => {
            if request.mode == Mode::InProcessAs512 {
                let limit = libc::rlimit {
                    rlim_cur: AS_LIMIT_BYTES,
                    rlim_max: AS_LIMIT_BYTES,
                };
                // Lowers only this trial process's own address-space limit.
                if unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } != 0 {
                    bail!("setrlimit: {}", std::io::Error::last_os_error());
                }
            }
            in_process(&request, &layout, &expected)?
        }
        Mode::Worker | Mode::WorkerBallast => {
            let ballast = (request.mode == Mode::WorkerBallast).then(|| vec![1u8; BALLAST_BYTES]);
            let trial = worker(&request, &layout, &expected)?;
            drop(std::hint::black_box(ballast));
            trial
        }
    };
    trial.memory_before = memory_before;
    trial.memory_after = memory()?;
    trial.self_usage = self_usage();
    Ok(trial)
}

fn blank(request: &Request<'_>, outcome: Outcome, matches_expected: Option<bool>) -> Trial {
    Trial {
        fixture: request.fixture.name.into(),
        mode: request.mode,
        repeat: request.repeat,
        outcome,
        matches_expected,
        extraction_wall_us: 0,
        extraction_self_cpu_us: 0,
        self_usage: Usage::default(),
        memory_before: Memory::default(),
        memory_after: Memory::default(),
        worker: None,
        process_wall_us: 0,
    }
}

fn in_process(
    request: &Request<'_>,
    layout: &mapping::PdfLayout,
    expected: &Expected,
) -> Result<Trial> {
    let cpu = self_usage().cpu_us();
    let start = Instant::now();
    let result = format_pdf::read(request.input, layout);
    let wall = start.elapsed();
    let cpu = self_usage().cpu_us() - cpu;
    let (outcome, matches) = match &result {
        Ok(instance) => instance_outcome(instance, expected)?,
        Err(error) => error_outcome(library_variant(error), error.to_string(), expected),
    };
    let mut trial = blank(request, outcome, matches);
    trial.extraction_wall_us = wall.as_micros() as u64;
    trial.extraction_self_cpu_us = cpu;
    Ok(trial)
}

fn private_dir(path: &Path) -> Result<PathBuf> {
    std::fs::create_dir(path).with_context(|| path.display().to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(path.to_path_buf())
}

fn failure_outcome(
    failure: &Failure,
    child: Option<&pdf_worker::Outcome>,
    expected: &Expected,
) -> (Outcome, Option<bool>) {
    let text = failure.diagnostics.display().text().to_owned();
    match (failure.kind, child) {
        (FailureKind::Pdf, Some(pdf_worker::Outcome::PdfError { error, diagnostics })) => {
            error_outcome(
                format!("{:?}", error.variant()),
                diagnostics.display().text().to_owned(),
                expected,
            )
        }
        // The owner's typed prelaunch refusal of the library's input limit.
        (FailureKind::InputTooLarge, None) => error_outcome("InputTooLarge".into(), text, expected),
        _ => {
            let outcome = Outcome::WorkerFailure {
                failure: format!("{:?}", failure.kind),
                operation: failure.operation.into(),
                display: text,
                errno: failure.errno,
                child_outcome: child.map(|outcome| format!("{outcome:?}")),
            };
            let matches = (!matches!(expected, Expected::Observed)).then_some(false);
            (outcome, matches)
        }
    }
}

fn stderr_excerpt(job: &Path) -> Result<String> {
    let path = job.join("stderr.bin");
    if !path.exists() {
        return Ok(String::new());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(STDERR_EXCERPT_BYTES)
        .read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn observation(receipt: &ClosedReceipt) -> WorkerObservation {
    let child = receipt
        .raw_wait_status
        .zip(receipt.reap_usage.as_ref())
        .map(|(status, usage)| {
            let (exit, signal) = exit_and_signal(status);
            ChildUsage {
                usage: Usage {
                    maxrss_kib: usage.maxrss_kib,
                    user_us: micros(usage.user_seconds, usage.user_microseconds),
                    system_us: micros(usage.system_seconds, usage.system_microseconds),
                },
                minor_faults: usage.raw_counters[3],
                major_faults: usage.raw_counters[4],
                raw_wait_status: status,
                exit,
                signal,
            }
        });
    WorkerObservation {
        owner_before_begin: Memory::default(),
        events: receipt
            .events
            .iter()
            .map(|event| (event.operation.to_owned(), event.at_microseconds))
            .collect(),
        child_spawned: receipt.child_pid.is_some(),
        ready: receipt.ready,
        start_sent: receipt.start_sent,
        child,
        effective_limits: receipt.effective_limits,
        receipt_elapsed_us: receipt.elapsed_microseconds,
        polls: receipt.polls,
        closure_incomplete_polls: 0,
        job_files: receipt
            .files
            .iter()
            .map(|file| (file.name.clone(), file.byte_len))
            .collect(),
        stderr_bytes: receipt.stderr_observed_bytes,
        stderr_excerpt: String::new(),
        cleanup_removed: receipt.cleanup_removed.clone(),
        job_directory_removed: false,
        scratch_entries_after: Vec::new(),
        descriptors_before: 0,
        descriptors_after: 0,
    }
}

fn worker(
    request: &Request<'_>,
    layout: &mapping::PdfLayout,
    expected: &Expected,
) -> Result<Trial> {
    let root = private_dir(
        &std::env::temp_dir().join(format!("ferrule-pdf-profile-{}", std::process::id())),
    )?;
    let scratch = private_dir(&root.join("scratch"))?;
    let descriptors_before = descriptors()?;
    let mut manager = PdfWorkerManager::new(
        WorkerProgram::admit_packaged_sibling(&request.package.join("ferrule"))?,
        ScratchPolicy::admit(&scratch, 0)?,
        ExclusiveReaperAdmission::for_current_host(),
    )?;
    let cancel = CancellationFlag::new();
    let owner_before_begin = memory()?;
    let cpu = self_usage().cpu_us();
    let start = Instant::now();
    let begun = manager.begin(request.input, layout, &cancel);
    let mut closure_incomplete_polls = 0;
    let progress = match begun {
        Err(BeginFailure::NoChild(failure)) => {
            bail!("refused without a job: {failure:#?}")
        }
        Ok(_) | Err(BeginFailure::OwnedStopping { .. }) => loop {
            match manager.poll() {
                JobProgress::Pending { .. } => {}
                JobProgress::ClosureIncomplete { .. } => closure_incomplete_polls += 1,
                terminal => break terminal,
            }
            if start.elapsed() > CLOSURE_BOUND {
                bail!("owned job unresolved after {CLOSURE_BOUND:?}");
            }
            std::thread::sleep(Duration::from_millis(1));
        },
    };
    let wall = start.elapsed();
    let cpu = self_usage().cpu_us() - cpu;
    let mut trial;
    match progress {
        JobProgress::Succeeded { instance, receipt } => {
            let (outcome, matches) = instance_outcome(&instance, expected)?;
            trial = blank(request, outcome, matches);
            let mut seen = observation(&receipt);
            seen.job_directory_removed = !receipt.successful_originals_retained;
            trial.worker = Some(seen);
        }
        JobProgress::FailedClosed { failure, retained } => {
            let (outcome, matches) =
                failure_outcome(&failure.failure, failure.child_outcome.as_ref(), expected);
            trial = blank(request, outcome, matches);
            let mut seen = observation(&failure.receipt);
            seen.stderr_excerpt = stderr_excerpt(&retained)?;
            let cleanup = manager
                .cleanup_closed_failure()
                .map_err(|failure| anyhow::anyhow!("cleanup refused: {failure:#?}"))?;
            seen.cleanup_removed = cleanup.removed;
            seen.job_directory_removed = cleanup.directory_removed;
            trial.worker = Some(seen);
        }
        other => bail!("unexpected terminal progress {other:#?}"),
    }
    drop(manager);
    if let Some(seen) = &mut trial.worker {
        seen.owner_before_begin = owner_before_begin;
        seen.closure_incomplete_polls = closure_incomplete_polls;
        seen.descriptors_before = descriptors_before;
        seen.descriptors_after = descriptors()?;
        seen.scratch_entries_after = std::fs::read_dir(&scratch)?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_>>()?;
    }
    trial.extraction_wall_us = wall.as_micros() as u64;
    trial.extraction_self_cpu_us = cpu;
    // Leftover scratch entries are recorded above and kept for inspection.
    if trial
        .worker
        .as_ref()
        .is_some_and(|seen| seen.scratch_entries_after.is_empty())
    {
        std::fs::remove_dir(&scratch)?;
        std::fs::remove_dir(&root)?;
    }
    Ok(trial)
}
