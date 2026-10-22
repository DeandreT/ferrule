//! One trial's typed record and the process observations it is built from.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// `format_pdf::read` in the trial process, without added limits.
    InProcess,
    /// The same call after the trial process lowers its own `RLIMIT_AS` to the
    /// worker's 512 MiB, without any other worker policy.
    InProcessAs512,
    /// The packaged worker under `PdfWorkerManager`.
    Worker,
    /// The packaged worker while the owner holds 256 MiB of touched memory.
    WorkerBallast,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProcess => "in-process",
            Self::InProcessAs512 => "in-process-as512",
            Self::Worker => "worker",
            Self::WorkerBallast => "worker-ballast",
        }
    }
    pub fn parse(text: &str) -> Result<Self> {
        [
            Self::InProcess,
            Self::InProcessAs512,
            Self::Worker,
            Self::WorkerBallast,
        ]
        .into_iter()
        .find(|mode| mode.as_str() == text)
        .with_context(|| format!("unknown mode {text}"))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// A typed tree, identified by its complete worker-wire encoding. Small
    /// trees without an independent expectation also keep their wire text.
    Instance {
        byte_len: u64,
        sha256: String,
        observed_wire: Option<String>,
    },
    /// A complete typed `PdfError`, from the library or the worker's result.
    PdfError { variant: String, display: String },
    /// Any other closed worker failure, retained as the owner reported it.
    WorkerFailure {
        failure: String,
        operation: String,
        display: String,
        errno: Option<i32>,
        child_outcome: Option<String>,
    },
    /// The trial process itself did not complete; recorded by the driver.
    TrialTerminated {
        exit: Option<i32>,
        signal: Option<i32>,
        stderr_excerpt: String,
    },
}

impl Outcome {
    /// Outcome class used to compare repeats and execution modes.
    pub fn class(&self) -> String {
        match self {
            Self::Instance {
                byte_len, sha256, ..
            } => format!("instance {byte_len} B {}", &sha256[..12]),
            Self::PdfError { variant, .. } => format!("PdfError::{variant}"),
            Self::WorkerFailure {
                failure, operation, ..
            } => format!("{failure}: {operation}"),
            Self::TrialTerminated { exit, signal, .. } => match (exit, signal) {
                (_, Some(signal)) => format!("trial process killed by signal {signal}"),
                (Some(code), _) => format!("trial process exit {code}"),
                _ => "trial process status unknown".into(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub maxrss_kib: i64,
    pub user_us: u64,
    pub system_us: u64,
}

impl Usage {
    pub fn cpu_us(&self) -> u64 {
        self.user_us + self.system_us
    }
}

/// Selected `/proc/self/status` values in KiB.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Memory {
    pub vm_peak_kib: u64,
    pub vm_hwm_kib: u64,
    pub vm_rss_kib: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChildUsage {
    /// `wait4` own-reap usage of the worker; a lifetime high-water value.
    pub usage: Usage,
    pub minor_faults: i64,
    pub major_faults: i64,
    pub raw_wait_status: i32,
    pub exit: Option<i32>,
    pub signal: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerObservation {
    /// The owner's memory immediately before `begin`.
    pub owner_before_begin: Memory,
    /// The owner's microsecond event timeline from its receipt.
    pub events: Vec<(String, u64)>,
    pub child_spawned: bool,
    pub ready: bool,
    pub start_sent: bool,
    pub child: Option<ChildUsage>,
    pub effective_limits: Option<[u64; 5]>,
    pub receipt_elapsed_us: u64,
    pub polls: u64,
    pub closure_incomplete_polls: u64,
    /// Every job leaf the owner's census recorded, with its byte length.
    pub job_files: Vec<(String, u64)>,
    pub stderr_bytes: u64,
    pub stderr_excerpt: String,
    pub cleanup_removed: Vec<String>,
    pub job_directory_removed: bool,
    pub scratch_entries_after: Vec<String>,
    pub descriptors_before: usize,
    pub descriptors_after: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trial {
    pub fixture: String,
    pub mode: Mode,
    pub repeat: u32,
    pub outcome: Outcome,
    /// `None` when the fixture records its outcome as observed only.
    pub matches_expected: Option<bool>,
    /// Monotonic wall time of the extraction or owned job, begin to terminal.
    pub extraction_wall_us: u64,
    /// The trial process's own CPU over the same bracket.
    pub extraction_self_cpu_us: u64,
    /// The trial process's `getrusage(RUSAGE_SELF)` at its end.
    pub self_usage: Usage,
    pub memory_before: Memory,
    pub memory_after: Memory,
    pub worker: Option<WorkerObservation>,
    /// Spawn to exit of the whole trial process, measured by the driver.
    pub process_wall_us: u64,
}

pub fn self_usage() -> Usage {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // Writable rusage storage for the calling process only.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    Usage {
        maxrss_kib: usage.ru_maxrss,
        user_us: micros(usage.ru_utime.tv_sec, usage.ru_utime.tv_usec),
        system_us: micros(usage.ru_stime.tv_sec, usage.ru_stime.tv_usec),
    }
}

pub fn micros(seconds: i64, microseconds: i64) -> u64 {
    u64::try_from(seconds * 1_000_000 + microseconds).unwrap_or(0)
}

pub fn memory() -> Result<Memory> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let field = |name: &str| -> Result<u64> {
        let line = status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .with_context(|| format!("{name} missing"))?;
        let Some(kib) = line.trim().strip_suffix(" kB") else {
            bail!("{name} unit");
        };
        Ok(kib.trim().parse()?)
    };
    Ok(Memory {
        vm_peak_kib: field("VmPeak:")?,
        vm_hwm_kib: field("VmHWM:")?,
        vm_rss_kib: field("VmRSS:")?,
    })
}

pub fn descriptors() -> Result<usize> {
    // The listing's own directory descriptor is counted in every census.
    Ok(std::fs::read_dir("/proc/self/fd")?.count())
}

pub fn exit_and_signal(status: i32) -> (Option<i32>, Option<i32>) {
    (
        libc::WIFEXITED(status).then(|| libc::WEXITSTATUS(status)),
        libc::WIFSIGNALED(status).then(|| libc::WTERMSIG(status)),
    )
}
