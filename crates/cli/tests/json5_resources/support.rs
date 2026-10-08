use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
// Identity is encoded explicitly below; no new serde derive dependency.
pub(super) struct Identity {
    pub(super) path: PathBuf,
    pub(super) bytes: u64,
    pub(super) sha256: String,
    stable: [i128; 9],
    cargo_elf_provenance: Option<String>,
}

pub(super) fn identity(path: &Path) -> TestResult<Identity> {
    observed_identity(path, None)
}

// Only receipt-authenticated Cargo hosts/CLI or a just-retained successful
// normal compiler-artifact observation can supply this private provenance.
// Every other source, corpus, DLL, tool, receipt and evidence file stays nlink1.
fn observed_identity(path: &Path, cargo_elf_provenance: Option<String>) -> TestResult<Identity> {
    if !path.is_absolute() || std::fs::canonicalize(path)? != path {
        return Err("guard path must be absolute and canonical".into());
    }
    let before = std::fs::symlink_metadata(path)?;
    if !before.is_file()
        || before.nlink() == 0
        || (cargo_elf_provenance.is_none() && before.nlink() != 1)
    {
        return Err("guard requires an ordinary single-link file unless exact normal Cargo ELF provenance is bound".into());
    }
    let stable = |m: &std::fs::Metadata| {
        [
            m.dev() as i128,
            m.ino() as i128,
            m.mode() as i128,
            m.nlink() as i128,
            m.len() as i128,
            m.mtime() as i128,
            m.mtime_nsec() as i128,
            m.ctime() as i128,
            m.ctime_nsec() as i128,
        ]
    };
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut block = [0_u8; 65536];
    let mut bytes = 0_u64;
    if cargo_elf_provenance.is_some() {
        let mut header = [0_u8; 20];
        file.read_exact(&mut header)?;
        let kind = match header[5] {
            1 => u16::from_le_bytes([header[16], header[17]]),
            2 => u16::from_be_bytes([header[16], header[17]]),
            _ => 0,
        };
        if &header[..4] != b"\x7fELF"
            || !matches!(header[4], 1 | 2)
            || header[6] != 1
            || !matches!(kind, 2 | 3)
            || before.mode() & 0o111 == 0
        {
            return Err(
                "qualified Cargo link exception requires executable ET_EXEC/ET_DYN ELF bytes"
                    .into(),
            );
        }
        hash.update(header);
        bytes = header.len() as u64;
    }
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
        bytes += n as u64;
    }
    let after = std::fs::symlink_metadata(path)?;
    if stable(&before) != stable(&after) || bytes != before.len() {
        return Err("file changed during hash observation".into());
    }
    Ok(Identity {
        path: path.to_owned(),
        bytes,
        sha256: format!("{:x}", hash.finalize()),
        stable: stable(&after),
        cargo_elf_provenance,
    })
}
impl Identity {
    pub(super) fn json(&self) -> Json {
        json!({"path":self.path,"bytes":self.bytes,"sha256":self.sha256,"stable":self.stable})
    }
}

pub(super) fn record(root: &Path, name: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
    std::fs::write(
        root.join(format!("{name}.debug.txt")),
        format!("{original:#?}\n"),
    )?;
    Ok(())
}
pub(super) fn json_file(path: &Path) -> TestResult<Json> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > 8 * 1024 * 1024 {
        return Err("bounded ordinary JSON record required".into());
    }
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub(super) fn required_path(name: &str) -> TestResult<PathBuf> {
    let path = PathBuf::from(
        std::env::var_os(name).ok_or_else(|| format!("explicit {name} is required"))?,
    );
    if !path.is_absolute() || std::fs::canonicalize(&path)? != path {
        return Err(format!("{name} must be absolute and canonical").into());
    }
    Ok(path)
}
fn compare_descriptor(value: &Json) -> TestResult<Identity> {
    let path = PathBuf::from(value["path"].as_str().ok_or("descriptor path required")?);
    let actual = identity(&path)?;
    if value["bytes"].as_u64() != Some(actual.bytes)
        || value["sha256"].as_str() != Some(actual.sha256.as_str())
    {
        return Err(format!("descriptor mismatch: {}", path.display()).into());
    }
    Ok(actual)
}

pub(super) fn compiler_artifact_identity(
    path: &Path,
    stdout: &[u8],
    name: &str,
    target: &Path,
) -> TestResult<Identity> {
    let mut selected = Vec::new();
    for line in std::str::from_utf8(stdout)?.lines() {
        let record: Json = serde_json::from_str(line)?;
        if record["reason"] == "compiler-artifact"
            && record["target"]["name"] == name
            && record["target"]["kind"] == json!(["bin"])
            && record["profile"]["test"] == false
            && let Some(executable) = record["executable"].as_str()
        {
            selected.push(std::fs::canonicalize(executable)?);
        }
    }
    selected.sort();
    selected.dedup();
    if selected.len() != 1 || selected[0] != path || !path.starts_with(target) {
        return Err(
            "exact selected normal Cargo binary/provenance required before link exception".into(),
        );
    }
    observed_identity(
        path,
        Some(format!(
            "actual-normal-Cargo-artifact; name={name}; target={}; stdout-sha256={:x}",
            target.display(),
            Sha256::digest(stdout)
        )),
    )
}

fn authenticated_descriptor(
    value: &Json,
    qualified_path: &Path,
    provenance: &str,
) -> TestResult<Identity> {
    let path = PathBuf::from(value["path"].as_str().ok_or("descriptor path required")?);
    if path != qualified_path {
        return compare_descriptor(value);
    }
    let actual = observed_identity(&path, Some(provenance.to_owned()))?;
    if value["bytes"].as_u64() != Some(actual.bytes)
        || value["sha256"].as_str() != Some(actual.sha256.as_str())
    {
        return Err("authenticated normal Cargo descriptor body mismatch".into());
    }
    Ok(actual)
}

pub(super) fn small_barrier(root: &Path) -> TestResult<Vec<Identity>> {
    let path = required_path("FERRULE_JSON5_SMALL_GATE_RECEIPT")?;
    let actual = identity(&path)?;
    let pinned = std::env::var("FERRULE_JSON5_SMALL_GATE_SHA256")?;
    if pinned.len() != 64 || pinned != actual.sha256 {
        return Err("root-authenticated actual184 receipt hash required".into());
    }
    let receipt = json_file(&path)?;
    std::fs::write(
        root.join("ORIGINAL-ALL184-RECEIPT.json"),
        std::fs::read(&path)?,
    )?;
    for (key, expected) in [
        ("schema_version", 1),
        ("issue", 144),
        ("rust", 92),
        ("csharp", 92),
        ("total", 184),
        ("failures", 0),
        ("physical_calls", 0),
    ] {
        if receipt[key].as_u64() != Some(expected) {
            return Err(format!("small prerequisite {key} mismatch").into());
        }
    }
    if receipt["qualification"] != "ALL184_SMALL_PUBLIC_CALLS_PASS"
        || receipt["complete_source_after_exact"] != true
        || receipt["complete_library_after_exact"] != true
    {
        return Err("actual both-language complete guard/pass receipt required".into());
    }
    let small_root = PathBuf::from(receipt["root"].as_str().ok_or("small root required")?);
    if !small_root.is_absolute()
        || std::fs::canonicalize(&small_root)? != small_root
        || path != small_root.join("ALL184_SMALL_PUBLIC_CALLS_COMPLETE.json")
    {
        return Err("small root/receipt confinement mismatch".into());
    }
    let small_cargo_host =
        required_path("FERRULE_CODEGEN_HOST_TARGET_DIR")?.join("debug/ferrule-json5-small-host");
    let current_cargo_cli = std::fs::canonicalize(Path::new(env!("CARGO_BIN_EXE_ferrule")))?;
    let provenance = format!(
        "root-authenticated144-receipt; path={}; sha256={}",
        actual.path.display(),
        actual.sha256
    );
    let mut before = vec![actual];
    let mut names = std::collections::BTreeSet::new();
    for group in ["sources", "libraries", "evidence", "call_results"] {
        let entries = receipt[group]
            .as_array()
            .ok_or("complete receipt group required")?;
        if entries.is_empty()
            || (group == "call_results" && entries.len() != 276)
            || (group == "libraries" && entries.len() != 12)
        {
            return Err(format!("incomplete {group} inventory").into());
        }
        for descriptor in entries {
            let item = match group {
                "libraries" => {
                    authenticated_descriptor(descriptor, &small_cargo_host, &provenance)?
                }
                "sources" => authenticated_descriptor(descriptor, &current_cargo_cli, &provenance)?,
                _ => compare_descriptor(descriptor)?,
            };
            if (group == "evidence" || group == "call_results")
                && (item.path.parent() != Some(small_root.as_path())
                    || !names.insert(item.path.clone()))
            {
                return Err("duplicate/outside small original evidence".into());
            }
            before.push(item);
        }
    }
    let cases = json_file(&small_root.join("CASES.json"))?;
    let cases = cases["cases"]
        .as_array()
        .ok_or("literal small cases required")?;
    if cases.len() != 23 {
        return Err("complete23 small case inventory required".into());
    }
    let mut expected = std::collections::BTreeSet::new();
    for case in cases {
        let fixture = case["fixture"].as_str().ok_or("literal fixture required")?;
        let id = case["id"].as_str().ok_or("literal case id required")?;
        if fixture.contains('/') || id.contains('/') {
            return Err("simple small case names required".into());
        }
        for route in 0..4 {
            for name in [
                format!("RUST-{fixture}-{id}-route{route}-ORIGINAL.txt"),
                format!("RUST-{fixture}-{id}-route{route}-PUBLIC-ORIGINAL.txt"),
                format!("CSHARP-{fixture}-{id}-route{route}-ORIGINAL.json"),
            ] {
                expected.insert(small_root.join(name));
            }
        }
    }
    let observed = receipt["call_results"]
        .as_array()
        .ok_or("call originals required")?
        .iter()
        .map(|entry| {
            entry["path"]
                .as_str()
                .map(PathBuf::from)
                .ok_or("call path required")
        })
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    if observed != expected || expected.len() != 276 {
        return Err("exact276 original-call membership required".into());
    }
    for language in ["RUST", "CSHARP"] {
        let count_path = small_root.join(format!("COMPLETE_CALL_COUNT_{language}.json"));
        if !names.contains(&count_path) {
            return Err("actual count must be bound as original evidence".into());
        }
        let count = json_file(&count_path)?;
        for (key, value) in [
            ("total", 92),
            ("expected", 92),
            ("failures", 0),
            ("physical_calls", 0),
        ] {
            if count[key].as_u64() != Some(value) {
                return Err("actual small host count/status mismatch".into());
            }
        }
        for suffix in [
            "COMMAND.debug.txt",
            "ORIGINAL-RESULT.debug.txt",
            "stdout.bin",
            "stderr.bin",
        ] {
            if !names.contains(&small_root.join(format!("{language}-HOST-{suffix}"))) {
                return Err("complete original host command/status/streams required".into());
            }
        }
    }
    for file in [
        "CASES.json",
        "COMPLETE-INPUT-SOURCE-GUARD-AFTER.debug.txt",
        "COMPLETE-LIBRARY-GUARD-AFTER.debug.txt",
    ] {
        if !names.contains(&small_root.join(file)) {
            return Err("complete small literal/after evidence required".into());
        }
    }
    record(root, "AUTHENTIC-ALL184-BARRIER-ORIGINAL", &before)?;
    // The pinned digest is supplied only after root inspects actual exit0 and
    // raw outputs; this code never creates or upgrades a small receipt.
    Ok(before)
}

pub(super) fn source_binding(root: &Path) -> TestResult<Vec<Identity>> {
    let path = required_path("FERRULE_JSON5_RESOURCE_SOURCE_BINDING")?;
    let pin = std::env::var("FERRULE_JSON5_RESOURCE_SOURCE_BINDING_SHA256")?;
    let actual = identity(&path)?;
    if pin != actual.sha256 {
        return Err("actual current source/toolchain binding digest required".into());
    }
    let binding = json_file(&path)?;
    if binding["issue"] != 145
        || binding["status"] != "ROOT_CURRENT_RESOURCE_SOURCE_TOOLCHAIN_BINDING"
    {
        return Err("root resource source binding required".into());
    }
    let mut identities = vec![actual];
    for group in ["sources", "tools"] {
        let entries = binding[group]
            .as_array()
            .ok_or("complete source/tool inventory required")?;
        if entries.is_empty() {
            return Err("empty source/tool binding refuses".into());
        }
        for entry in entries {
            let item = if group == "sources" {
                let current_cargo_cli =
                    std::fs::canonicalize(Path::new(env!("CARGO_BIN_EXE_ferrule")))?;
                authenticated_descriptor(
                    entry,
                    &current_cargo_cli,
                    &format!("root-authenticated-current-source-binding; sha256={pin}"),
                )?
            } else {
                compare_descriptor(entry)?
            };
            identities.push(item);
        }
    }
    let workspace = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/cli/tests/json5_resources.rs",
        "crates/cli/tests/json5_resources/corpus.rs",
        "crates/cli/tests/json5_resources/support.rs",
        "crates/cli/tests/fixtures/json5_resources_rust.rs.txt",
        "crates/cli/tests/fixtures/json5_resources_csharp.cs.txt",
        "crates/codegen-rust/src/json5_api.rs",
        "crates/codegen-runtime/src/json5.rs",
        "crates/codegen-runtime/src/json5/codec.rs",
        "crates/codegen-csharp/src/json5_output.rs",
        "runtime/csharp/Ferrule.Runtime/Json5/FerruleJson.Json5Codec.cs",
        "crates/format-json/src/json5_boundary.rs",
        "crates/format-json/src/json5_boundary/strings.rs",
        "runtime/csharp/Ferrule.Runtime/Json5/FerruleJson5Syntax.cs",
        "runtime/csharp/Ferrule.Runtime/Json5/FerruleJson5Syntax.Strings.cs",
    ] {
        if !identities
            .iter()
            .any(|value| value.path == workspace.join(relative))
        {
            return Err(format!("missing reached source binding {relative}").into());
        }
    }
    record(root, "ROOT-SOURCE-TOOLCHAIN-BINDING-ORIGINAL", &identities)?;
    Ok(identities)
}

pub(super) fn check_guard(root: &Path, label: &str, before: &[Identity]) -> TestResult<()> {
    let observed = before
        .iter()
        .map(|entry| observed_identity(&entry.path, entry.cargo_elf_provenance.clone()))
        .collect::<Vec<_>>();
    record(root, label, &observed)?;
    if !observed
        .iter()
        .zip(before)
        .all(|(actual, expected)| matches!(actual,Ok(actual) if actual == expected))
    {
        return Err("complete original source/corpus/library guard changed".into());
    }
    Ok(())
}
pub(super) fn tree(root: &Path) -> TestResult<Vec<Identity>> {
    fn visit(root: &Path, result: &mut Vec<Identity>, depth: usize) -> TestResult<()> {
        if depth > 32 || result.len() > 10_000 {
            return Err("bounded source tree census exceeded".into());
        }
        let mut paths = std::fs::read_dir(root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        for path in paths {
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.is_symlink() {
                return Err("no generated tree link accepted".into());
            }
            if meta.is_dir() {
                if !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("target" | "bin" | "obj")
                ) {
                    visit(&path, result, depth + 1)?;
                }
            } else {
                result.push(identity(&path)?);
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    visit(root, &mut result, 0)?;
    Ok(result)
}

pub(super) struct Runner {
    pub(super) root: PathBuf,
    cutoff: Instant,
    pub(super) time: PathBuf,
    timeout: PathBuf,
}
impl Runner {
    pub(super) fn new(root: PathBuf, time: PathBuf, timeout: PathBuf) -> TestResult<Self> {
        let absolute = std::env::var("FERRULE_JSON5_RESOURCE_DEADLINE_UNIX")?.parse::<u64>()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let remaining = absolute
            .checked_sub(now)
            .filter(|remaining| *remaining > 0 && *remaining <= 7200)
            .ok_or("positive root outer deadline <=7200s required")?;
        Ok(Self {
            root,
            cutoff: Instant::now() + Duration::from_secs(remaining),
            time,
            timeout,
        })
    }
    pub(super) fn command(
        &self,
        label: &str,
        program: &Path,
        args: &[String],
        cwd: &Path,
        measured: bool,
        target: &Path,
    ) -> TestResult<Output> {
        let remaining = self
            .cutoff
            .checked_duration_since(Instant::now())
            .ok_or("campaign deadline expired before launch")?;
        let remaining = remaining
            .checked_sub(Duration::from_secs(15))
            .ok_or("reserve kill-after and final observation budget before launch")?;
        let limit = remaining.min(Duration::from_secs(if measured { 600 } else { 300 }));
        if limit < Duration::from_secs(1) {
            return Err("positive launch budget required".into());
        }
        let mut command = Command::new(&self.timeout);
        command.args(["--signal=TERM", "--kill-after=5s"]);
        command.arg(format!("{:.3}s", limit.as_secs_f64()));
        if measured {
            command
                .arg(&self.time)
                .args(["--verbose", "--output"])
                .arg(self.root.join(format!("{label}-GNU-TIME.txt")));
        }
        command
            .arg(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-Dwarnings")
            .env("RUSTC", required_path("FERRULE_CODEGEN_RUSTC")?)
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_CLI_HOME", self.root.join(".dotnet-home"))
            .env("NUGET_PACKAGES", self.root.join(".nuget-packages"))
            .env("TMPDIR", self.root.join(".tmp"));
        record(&self.root, &format!("{label}-COMMAND"), &command)?;
        let started = Instant::now();
        let launched = command.spawn();
        record(
            &self.root,
            &format!("{label}-SPAWN-ORIGINAL"),
            &launched.as_ref().map(std::process::Child::id),
        )?;
        let child = launched?;
        let pid = child.id();
        let birth = std::fs::read(format!("/proc/{pid}/stat"));
        record(&self.root, &format!("{label}-INITIAL-PID-ORIGINAL"), &birth)?;
        let original = child.wait_with_output();
        record(&self.root, &format!("{label}-ORIGINAL-RESULT"), &original)?;
        if let Ok(output) = &original {
            std::fs::write(
                self.root.join(format!("{label}-stdout.bin")),
                &output.stdout,
            )?;
            std::fs::write(
                self.root.join(format!("{label}-stderr.bin")),
                &output.stderr,
            )?;
        }
        record(&self.root, &format!("{label}-ELAPSED"), &started.elapsed())?;
        let current = std::fs::read(format!("/proc/{pid}/stat"));
        record(&self.root, &format!("{label}-AFTER-PID-ORIGINAL"), &current)?;
        // GNU timeout's default process group owns TERM/kill-after. Any live
        // original group after direct-child reaping stops the campaign.
        let group = process_group_members(pid)?;
        record(&self.root, &format!("{label}-AFTER-GROUP-ORIGINAL"), &group)?;
        if (measured && birth.is_err()) || current.is_ok() || !group.is_empty() {
            std::fs::write(
                self.root.join("PROCESS-UNCERTAINTY-STOP.txt"),
                format!("{label}; pid={pid}; root must inspect owned descendants; no next call\n"),
            )?;
            return Err(
                "owned process closure uncertain; raw originals retained and no next call".into(),
            );
        }
        Ok(original?)
    }
}
fn process_group_members(group: u32) -> TestResult<Vec<(PathBuf, String)>> {
    let mut observed = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .and_then(|text| text.parse::<u32>().ok())
            .is_none()
        {
            continue;
        }
        let path = entry.path().join("stat");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let fields = text
            .rsplit_once(')')
            .ok_or("malformed process stat")?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.get(2).and_then(|text| text.parse::<u32>().ok()) == Some(group) {
            observed.push((path, text));
        }
    }
    Ok(observed)
}

pub(super) fn headroom(runner: &Runner, label: &str, target: &Path) -> TestResult<()> {
    let memory = std::fs::read_to_string("/proc/meminfo")?;
    std::fs::write(runner.root.join(format!("{label}-meminfo.txt")), &memory)?;
    let available = memory
        .lines()
        .find_map(|line| {
            line.strip_prefix("MemAvailable:")
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok())
        })
        .ok_or("actual MemAvailable missing")?;
    let df = required_path("FERRULE_CODEGEN_DF")?;
    let output = runner.command(
        &format!("{label}-DF"),
        &df,
        &[
            "-Pk".into(),
            runner.root.display().to_string(),
            target.display().to_string(),
        ],
        &runner.root,
        false,
        target,
    )?;
    if !output.status.success() {
        return Err("disk observation failed".into());
    }
    let text = std::str::from_utf8(&output.stdout)?;
    let free = text
        .lines()
        .skip(1)
        .map(|line| {
            line.split_whitespace()
                .nth(3)
                .ok_or("disk available missing")?
                .parse::<u64>()
                .map_err(|error| error.into())
        })
        .collect::<TestResult<Vec<_>>>()?;
    if available < 8 * 1024 * 1024
        || free.is_empty()
        || free.iter().any(|available| *available < 20 * 1024 * 1024)
    {
        return Err(
            "8GiB available memory/20GiB free disk reserve required before each build/call".into(),
        );
    }
    Ok(())
}
