//! Markdown summary of the run: environment, fixtures and per-cell brackets.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::fixtures::{Expected, Fixture};
use crate::record::{Mode, Trial};

pub struct Input<'a> {
    pub fixture: &'a Fixture,
    pub bytes: u64,
    pub sha256: String,
    pub path: PathBuf,
}

fn first_line(path: &str, prefix: &str) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix(prefix))
                .map(|value| value.trim_start_matches([' ', '\t', ':']).trim().to_owned())
        })
        .unwrap_or_else(|| "unavailable".into())
}

fn load() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|text| {
            text.split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|_| "unavailable".into())
}

pub fn environment(
    out: &mut impl Write,
    worker: &Path,
    worker_bytes: usize,
    worker_sha256: &str,
    repeats: u32,
) -> Result<()> {
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
    let parallelism = std::thread::available_parallelism().map_or(0, usize::from);
    let build = if cfg!(debug_assertions) {
        "debug assertions on"
    } else {
        "debug assertions off (release)"
    };
    writeln!(out, "## Environment\n")?;
    writeln!(out, "| Item | Value |\n| --- | --- |")?;
    writeln!(
        out,
        "| Platform | {} {} |",
        std::env::consts::OS,
        std::env::consts::ARCH
    )?;
    writeln!(out, "| Kernel | {} |", kernel.trim())?;
    writeln!(
        out,
        "| CPU | {} ({parallelism} available) |",
        first_line("/proc/cpuinfo", "model name")
    )?;
    writeln!(
        out,
        "| MemTotal | {} |",
        first_line("/proc/meminfo", "MemTotal")
    )?;
    writeln!(out, "| Build | {build} |")?;
    writeln!(
        out,
        "| Worker | `{}`, {worker_bytes} bytes, SHA-256 `{worker_sha256}` |",
        worker.display()
    )?;
    writeln!(out, "| Repeats | {repeats} per cell, interleaved |")?;
    writeln!(out, "| Load average at start | {} |\n", load())?;
    Ok(())
}

pub fn fixtures(out: &mut impl Write, inputs: &[Input<'_>]) -> Result<()> {
    writeln!(out, "## Fixtures\n")?;
    writeln!(
        out,
        "| Fixture | Bytes | SHA-256 | Expected | Description |\n| --- | ---: | --- | --- | --- |"
    )?;
    for input in inputs {
        let expected = match (input.fixture.expected)() {
            Expected::Instance(_) => "typed tree".to_owned(),
            Expected::PdfError(variant) => format!("`PdfError::{variant}`"),
            Expected::Observed => "observed".to_owned(),
        };
        writeln!(
            out,
            "| `{}` | {} | `{}` | {expected} | {} |",
            input.fixture.name, input.bytes, input.sha256, input.fixture.description
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn range(values: impl IntoIterator<Item = u64>) -> String {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    match (values.first(), values.last()) {
        (Some(low), Some(high)) if low == high => low.to_string(),
        (Some(low), Some(high)) => format!("{low}–{high}"),
        _ => "—".into(),
    }
}

/// Minimum / median / maximum of microsecond values, in milliseconds.
fn millis(values: impl IntoIterator<Item = u64>) -> String {
    let mut values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        return "—".into();
    }
    values.sort_unstable();
    let ms = |us: u64| format!("{:.1}", us as f64 / 1000.0);
    format!(
        "{} / {} / {}",
        ms(values[0]),
        ms(values[values.len() / 2]),
        ms(values[values.len() - 1])
    )
}

fn cells<'a>(inputs: &[Input<'_>], trials: &'a [Trial]) -> Vec<(String, Mode, Vec<&'a Trial>)> {
    let mut cells = Vec::new();
    for input in inputs {
        let modes = trials
            .iter()
            .filter(|trial| trial.fixture == input.fixture.name)
            .map(|trial| trial.mode)
            .collect::<BTreeSet<_>>();
        for mode in modes {
            let group = trials
                .iter()
                .filter(|trial| trial.fixture == input.fixture.name && trial.mode == mode)
                .collect();
            cells.push((input.fixture.name.to_owned(), mode, group));
        }
    }
    cells
}

fn classes(group: &[&Trial]) -> String {
    group
        .iter()
        .map(|trial| trial.outcome.class())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn summary(out: &mut impl Write, inputs: &[Input<'_>], trials: &[Trial]) -> Result<()> {
    let cells = cells(inputs, trials);
    writeln!(out, "## Outcomes\n")?;
    writeln!(
        out,
        "| Fixture | Mode | Outcome (all repeats) | Matches expected | Worker child | Effective limits |\n| --- | --- | --- | --- | --- | --- |"
    )?;
    for (name, mode, group) in &cells {
        let matched = match group
            .iter()
            .map(|t| t.matches_expected)
            .collect::<BTreeSet<_>>()
        {
            set if set == BTreeSet::from([Some(true)]) => format!("yes ({})", group.len()),
            set if set == BTreeSet::from([None]) => "observed".into(),
            set => format!("{set:?}"),
        };
        let child = group
            .iter()
            .filter_map(|t| t.worker.as_ref())
            .map(|w| match &w.child {
                Some(c) => match (c.exit, c.signal) {
                    (Some(code), _) => format!("yes, exit {code}"),
                    (_, Some(signal)) => format!("yes, signal {signal}"),
                    _ => format!("yes, raw status {}", c.raw_wait_status),
                },
                None if w.child_spawned => "yes, unreaped".into(),
                None => "no".into(),
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("; ");
        let limits = group
            .iter()
            .filter_map(|t| t.worker.as_ref().and_then(|w| w.effective_limits))
            .map(|l| format!("{l:?}"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("; ");
        writeln!(
            out,
            "| `{name}` | {} | {} | {matched} | {} | {} |",
            mode.as_str(),
            classes(group),
            if child.is_empty() {
                "—".into()
            } else {
                child
            },
            if limits.is_empty() {
                "—".into()
            } else {
                limits
            },
        )?;
    }

    writeln!(out, "\n## Worker and in-process comparison\n")?;
    writeln!(
        out,
        "| Fixture | In-process | Worker | Same complete outcome |\n| --- | --- | --- | --- |"
    )?;
    for input in inputs {
        let of = |mode| {
            let group = trials
                .iter()
                .filter(|t| t.fixture == input.fixture.name && t.mode == mode)
                .collect::<Vec<_>>();
            (
                classes(&group),
                group.iter().map(|t| t.outcome.clone()).collect::<Vec<_>>(),
            )
        };
        let (library, library_all) = of(Mode::InProcess);
        let (worker, worker_all) = of(Mode::Worker);
        let same = library_all.iter().chain(&worker_all).all(|outcome| {
            match (outcome, library_all.first()) {
                (
                    crate::record::Outcome::PdfError { variant, .. },
                    Some(crate::record::Outcome::PdfError { variant: first, .. }),
                ) => variant == first,
                (outcome, Some(first)) => outcome == first,
                _ => false,
            }
        });
        writeln!(
            out,
            "| `{}` | {library} | {worker} | {} |",
            input.fixture.name,
            if same { "yes" } else { "no" }
        )?;
    }

    writeln!(out, "\n## Memory (KiB, min–max)\n")?;
    writeln!(
        out,
        "| Fixture | Mode | Trial VmHWM | Trial VmPeak | Trial ru_maxrss | Owner VmHWM before begin | Worker ru_maxrss (own wait4) | Worker value above owner VmHWM |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |"
    )?;
    for (name, mode, group) in &cells {
        let completed = || group.iter().filter(|t| t.memory_after.vm_hwm_kib > 0);
        let seen = || group.iter().filter_map(|t| t.worker.as_ref());
        let children = || seen().filter_map(|w| w.child.as_ref());
        // The worker's own-reap value is max(owner high-water at spawn, the
        // worker image's own peak); only a value above the owner's whole-trial
        // high-water is attributable to the worker image.
        let above = group
            .iter()
            .filter_map(|t| {
                let child = t.worker.as_ref()?.child.as_ref()?;
                Some(child.usage.maxrss_kib as u64 > t.memory_after.vm_hwm_kib)
            })
            .collect::<BTreeSet<_>>();
        let above = match (above.contains(&true), above.contains(&false)) {
            (true, false) => "yes",
            (false, true) => "no (upper bound only)",
            (true, true) => "mixed",
            (false, false) => "—",
        };
        writeln!(
            out,
            "| `{name}` | {} | {} | {} | {} | {} | {} | {above} |",
            mode.as_str(),
            range(completed().map(|t| t.memory_after.vm_hwm_kib)),
            range(completed().map(|t| t.memory_after.vm_peak_kib)),
            range(completed().map(|t| t.self_usage.maxrss_kib as u64)),
            range(seen().map(|w| w.owner_before_begin.vm_hwm_kib)),
            range(children().map(|c| c.usage.maxrss_kib as u64)),
        )?;
    }

    writeln!(out, "\n## Time (milliseconds: min / median / max)\n")?;
    writeln!(
        out,
        "| Fixture | Mode | Extraction or job wall | Process CPU in bracket | Worker CPU (own wait4) | Whole trial process wall |\n| --- | --- | --- | --- | --- | --- |"
    )?;
    for (name, mode, group) in &cells {
        let completed = group.iter().filter(|t| t.self_usage.maxrss_kib > 0);
        writeln!(
            out,
            "| `{name}` | {} | {} | {} | {} | {} |",
            mode.as_str(),
            millis(completed.clone().map(|t| t.extraction_wall_us)),
            millis(completed.map(|t| t.extraction_self_cpu_us)),
            millis(
                group
                    .iter()
                    .filter_map(|t| Some(t.worker.as_ref()?.child.as_ref()?.usage.cpu_us()))
            ),
            millis(group.iter().map(|t| t.process_wall_us)),
        )?;
    }

    writeln!(
        out,
        "\n## Owner event timeline (milliseconds after begin: min / median / max)\n"
    )?;
    writeln!(
        out,
        "| Fixture | Mode | Event | At |\n| --- | --- | --- | --- |"
    )?;
    for (name, mode, group) in &cells {
        let seen = group
            .iter()
            .filter_map(|t| t.worker.as_ref())
            .collect::<Vec<_>>();
        let Some(first) = seen.first() else { continue };
        let mut events = first
            .events
            .iter()
            .map(|(event, _)| event.clone())
            .collect::<Vec<_>>();
        events.push("terminal receipt".into());
        for event in events {
            let at = seen.iter().filter_map(|w| {
                if event == "terminal receipt" {
                    Some(w.receipt_elapsed_us)
                } else {
                    w.events
                        .iter()
                        .find(|(e, _)| *e == event)
                        .map(|(_, at)| *at)
                }
            });
            writeln!(
                out,
                "| `{name}` | {} | {event} | {} |",
                mode.as_str(),
                millis(at)
            )?;
        }
    }

    writeln!(out, "\n## Worker job artifacts and closure\n")?;
    writeln!(
        out,
        "| Fixture | Mode | Job leaves (bytes) | Stderr bytes | Cleanup removed | Job directory removed | Scratch empty after | Descriptors restored |\n| --- | --- | --- | ---: | --- | --- | --- | --- |"
    )?;
    let mut excerpts = BTreeSet::new();
    for (name, mode, group) in &cells {
        let seen = group
            .iter()
            .filter_map(|t| t.worker.as_ref())
            .collect::<Vec<_>>();
        let Some(first) = seen.first() else { continue };
        for w in &seen {
            if !w.stderr_excerpt.is_empty() {
                excerpts.insert((name.clone(), w.stderr_excerpt.clone()));
            }
        }
        let leaves = first
            .job_files
            .iter()
            .map(|(leaf, bytes)| format!("{leaf} {bytes}"))
            .collect::<Vec<_>>()
            .join(", ");
        let all = |f: &dyn Fn(&&crate::record::WorkerObservation) -> bool| {
            if seen.iter().all(f) { "yes" } else { "no" }
        };
        writeln!(
            out,
            "| `{name}` | {} | {} | {} | {} | {} | {} | {} |",
            mode.as_str(),
            if leaves.is_empty() {
                "—".into()
            } else {
                leaves
            },
            range(seen.iter().map(|w| w.stderr_bytes)),
            seen.iter()
                .map(|w| w.cleanup_removed.join(" "))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("; "),
            all(&|w| w.job_directory_removed),
            all(&|w| w.scratch_entries_after.is_empty()),
            all(&|w| w.descriptors_before == w.descriptors_after && w.closure_incomplete_polls == 0),
        )?;
    }
    for trial in trials {
        if let crate::record::Outcome::TrialTerminated { stderr_excerpt, .. } = &trial.outcome
            && !stderr_excerpt.is_empty()
        {
            excerpts.insert((
                format!("{} {}", trial.fixture, trial.mode.as_str()),
                stderr_excerpt.clone(),
            ));
        }
    }
    let observed = trials
        .iter()
        .filter_map(|trial| match &trial.outcome {
            crate::record::Outcome::Instance {
                observed_wire: Some(wire),
                ..
            } => Some((trial.fixture.clone(), wire.clone())),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if !observed.is_empty() {
        writeln!(out, "\n## Observed wire trees\n")?;
        for (name, wire) in observed {
            writeln!(out, "- `{name}`: `{wire}`")?;
        }
    }
    if !excerpts.is_empty() {
        writeln!(out, "\n## Observed stderr excerpts\n")?;
        for (name, excerpt) in excerpts {
            writeln!(out, "- `{name}`: `{}`", excerpt.trim().replace('\n', " ⏎ "))?;
        }
    }
    writeln!(out, "\nLoad average at end: {}", load())?;
    Ok(())
}
