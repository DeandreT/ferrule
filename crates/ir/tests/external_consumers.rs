//! Separate Cargo workspaces prevent this regression's serde_json feature from
//! being unified into IR's ordinary workspace tests.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ConsumerDirectory(PathBuf);

impl Drop for ConsumerDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn schema_metadata_in_default_and_arbitrary_precision_consumers() -> Result<(), Box<dyn Error>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ferrule-schema-consumers-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir(&path)?;
    // Own cleanup only after exclusive creation succeeds; a stale directory
    // with a reused process ID must not be removed by a failed setup.
    let root = ConsumerDirectory(path);
    let ir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = ir.join("tests/schema_consumer/tests.rs");
    for (name, arbitrary_precision, raw_value) in [
        ("default", false, false),
        ("arbitrary", true, false),
        ("arbitrary-raw", true, true),
    ] {
        let consumer = root.0.join(name);
        std::fs::create_dir(&consumer)?;
        std::fs::write(
            consumer.join("Cargo.toml"),
            format!(
                "[package]\nname = \"schema-{name}-consumer\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             [workspace]\n[lib]\npath = {:?}\n\
              [features]\narbitrary_precision = [\"serde_json/arbitrary_precision\"]\nraw_value = [\"serde_json/raw_value\"]\n\
             [dependencies]\nir = {{ path = {:?} }}\n\
             serde = {{ version = \"1\", features = [\"derive\"] }}\n\
             serde_json = {{ version = \"1\", features = [\"float_roundtrip\", \"preserve_order\"] }}\n",
                source.to_string_lossy().replace('\\', "/"),
                ir.to_string_lossy().replace('\\', "/"),
            ),
        )?;
        // Reuse the checked-in dependency versions, not the outer workspace's
        // feature graph. Cargo only adds this standalone consumer to the lock.
        std::fs::copy(ir.join("../../Cargo.lock"), consumer.join("Cargo.lock"))?;
        let mut command = Command::new(env!("CARGO"));
        command
            .current_dir(&consumer)
            .args(["test", "--offline", "-j2"])
            .arg("--target-dir")
            .arg(consumer.join("target"))
            .args([
                "--config",
                "profile.dev.debug=0",
                "--config",
                "profile.dev.incremental=false",
                "--config",
                "profile.dev.strip=\"symbols\"",
            ])
            .env_remove("CARGO_BUILD_BUILD_DIR")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR");
        if arbitrary_precision {
            command.args(["--features", "arbitrary_precision"]);
        }
        if raw_value {
            command.args(["--features", "raw_value"]);
        }
        let output = command.output()?;
        println!(
            "{name}: {command:?}\nstatus: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{name} external consumer failed");
        let mut graph = Command::new(env!("CARGO"));
        graph.current_dir(&consumer).args([
            "tree",
            "--offline",
            "--prefix",
            "none",
            "--edges",
            "features",
            "--invert",
            "serde_json",
        ]);
        if arbitrary_precision {
            graph.args(["--features", "arbitrary_precision"]);
        }
        if raw_value {
            graph.args(["--features", "raw_value"]);
        }
        let versions = graph.output()?;
        println!(
            "{name} dependency versions:\n{}",
            String::from_utf8_lossy(&versions.stdout)
        );
        assert!(versions.status.success());
    }
    Ok(())
}
