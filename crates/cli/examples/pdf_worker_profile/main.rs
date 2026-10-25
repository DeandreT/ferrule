//! Finite PDF worker resource profile for Linux x86_64 (#328).
//!
//! Build the packaged CLI, worker and this program with the same profile, then
//! run it from the build directory:
//!
//! ```sh
//! cargo build --release --locked -p cli --bins --example pdf_worker_profile
//! target/release/examples/pdf_worker_profile --repeats 5 --raw trials.jsonl
//! ```
//!
//! Each trial runs in a fresh process of this program, so its lifetime
//! high-water values belong to that one extraction. The report neither changes
//! defaults nor qualifies hostile PDFs.
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod fixtures;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod record;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod report;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod trial;

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
fn main() -> anyhow::Result<()> {
    driver::main()
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
fn main() {
    eprintln!("the PDF worker profile requires Linux x86_64 GNU");
    std::process::exit(2);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod driver {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Instant;

    use anyhow::{Context, Result, bail, ensure};
    use sha2::{Digest, Sha256};

    use crate::fixtures::{FIXTURES, Fixture, fixture};
    use crate::record::{Mode, Outcome, Trial};
    use crate::{report, trial};

    struct Options {
        repeats: u32,
        build: PathBuf,
        raw: Option<PathBuf>,
        only: Option<Vec<String>>,
    }

    fn options(args: &[String]) -> Result<Options> {
        let exe = std::env::current_exe()?;
        let mut options = Options {
            repeats: 5,
            // target/<profile>/examples/<this> beside target/<profile>/ferrule.
            build: exe
                .parent()
                .and_then(Path::parent)
                .context("build directory")?
                .to_path_buf(),
            raw: None,
            only: None,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = || args.next().with_context(|| format!("{arg} needs a value"));
            match arg.as_str() {
                "--repeats" => options.repeats = value()?.parse()?,
                "--build-dir" => options.build = PathBuf::from(value()?),
                "--raw" => options.raw = Some(PathBuf::from(value()?)),
                "--fixtures" => {
                    options.only = Some(value()?.split(',').map(str::to_owned).collect());
                }
                other => bail!("unknown argument {other}"),
            }
        }
        ensure!(options.repeats > 0, "--repeats must be positive");
        Ok(options)
    }

    pub fn main() -> Result<()> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.first().map(String::as_str) == Some("--trial") {
            let [_, name, mode, repeat, input, package] = args.as_slice() else {
                bail!("--trial <fixture> <mode> <repeat> <input> <package>");
            };
            let trial = trial::run(trial::Request {
                fixture: fixture(name).context("unknown fixture")?,
                mode: Mode::parse(mode)?,
                repeat: repeat.parse()?,
                input: Path::new(input),
                package: Path::new(package),
            })?;
            let mut out = std::io::stdout().lock();
            serde_json::to_writer(&mut out, &trial)?;
            writeln!(out)?;
            return Ok(());
        }
        let options = options(&args)?;
        run(&options)
    }

    fn modes(fixture: &Fixture) -> Vec<Mode> {
        let mut modes = vec![Mode::InProcess, Mode::InProcessAs512, Mode::Worker];
        if fixture.name == "hello-page" {
            modes.push(Mode::WorkerBallast);
        }
        modes
    }

    fn private_dir(path: &Path) -> Result<PathBuf> {
        std::fs::create_dir(path).with_context(|| path.display().to_string())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        Ok(path.to_path_buf())
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn run(options: &Options) -> Result<()> {
        let selected = FIXTURES
            .iter()
            .filter(|fixture| {
                options
                    .only
                    .as_ref()
                    .is_none_or(|only| only.iter().any(|name| name == fixture.name))
            })
            .collect::<Vec<_>>();
        let built = options.build.join("ferrule-pdf-worker");
        let worker_bytes = std::fs::read(&built).with_context(|| built.display().to_string())?;
        let mut out = std::io::stdout().lock();
        report::environment(
            &mut out,
            &built,
            worker_bytes.len(),
            &sha256(&worker_bytes),
            options.repeats,
        )?;

        let dir = private_dir(
            &std::env::temp_dir().join(format!("ferrule-pdf-profile-run-{}", std::process::id())),
        )?;
        // Cargo hard-links build outputs, and admission requires a single-link
        // worker, so trials use a private package copy of the same bytes.
        let package = private_dir(&dir.join("package"))?;
        let worker = package.join("ferrule-pdf-worker");
        std::fs::write(&worker, &worker_bytes)?;
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o500))?;
        drop(worker_bytes);
        let mut inputs = Vec::new();
        for fixture in &selected {
            let bytes = (fixture.build)()?;
            let path = dir.join(format!("{}.pdf", fixture.name));
            std::fs::write(&path, &bytes)?;
            inputs.push(report::Input {
                fixture,
                bytes: bytes.len() as u64,
                sha256: sha256(&bytes),
                path,
            });
        }
        report::fixtures(&mut out, &inputs)?;

        let mut raw = options
            .raw
            .as_ref()
            .map(|path| std::fs::File::create_new(path).with_context(|| path.display().to_string()))
            .transpose()?;
        let mut trials = Vec::new();
        // Interleave fixtures and modes within each repeat so slow drift in
        // host load spreads across every cell instead of one.
        for repeat in 1..=options.repeats {
            for input in &inputs {
                for mode in modes(input.fixture) {
                    let trial = spawn_trial(input, mode, repeat, &package)?;
                    if let Some(raw) = &mut raw {
                        serde_json::to_writer(&mut *raw, &trial)?;
                        writeln!(raw)?;
                    }
                    trials.push(trial);
                }
            }
        }
        std::fs::remove_dir_all(&dir)?;
        report::summary(&mut out, &inputs, &trials)?;
        Ok(())
    }

    fn spawn_trial(
        input: &report::Input<'_>,
        mode: Mode,
        repeat: u32,
        package: &Path,
    ) -> Result<Trial> {
        let start = Instant::now();
        let output = Command::new(std::env::current_exe()?)
            .arg("--trial")
            .arg(input.fixture.name)
            .arg(mode.as_str())
            .arg(repeat.to_string())
            .arg(&input.path)
            .arg(package)
            .output()?;
        let wall = start.elapsed().as_micros() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut trial = match stdout.lines().last().filter(|_| output.status.success()) {
            Some(line) => serde_json::from_str::<Trial>(line)?,
            None => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let excerpt = stderr.chars().take(512).collect::<String>();
                // Only a signal-terminated trial is an observed outcome; any
                // other failure is a fault of this program and stops the run.
                if output.status.signal().is_none() {
                    bail!(
                        "trial {} {} failed: {}\n{excerpt}",
                        input.fixture.name,
                        mode.as_str(),
                        output.status
                    );
                }
                terminated(input.fixture, mode, repeat, &output.status, excerpt)
            }
        };
        trial.process_wall_us = wall;
        Ok(trial)
    }

    /// A trial process that did not complete is itself an observed outcome.
    fn terminated(
        fixture: &Fixture,
        mode: Mode,
        repeat: u32,
        status: &std::process::ExitStatus,
        stderr_excerpt: String,
    ) -> Trial {
        Trial {
            fixture: fixture.name.into(),
            mode,
            repeat,
            outcome: Outcome::TrialTerminated {
                exit: status.code(),
                signal: status.signal(),
                stderr_excerpt,
            },
            matches_expected: None,
            extraction_wall_us: 0,
            extraction_self_cpu_us: 0,
            self_usage: Default::default(),
            memory_before: Default::default(),
            memory_after: Default::default(),
            worker: None,
            process_wall_us: 0,
        }
    }
}
