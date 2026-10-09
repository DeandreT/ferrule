// Shared only by the generated JSON5 tests; it is not part of either emitter.
use std::error::Error;
use std::fmt::Debug;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use codegen::{ArtifactPath, ArtifactSet};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub struct Evidence {
    directory: PathBuf,
}

impl Evidence {
    pub fn new(tag: &str) -> Self {
        let root = if let Some(root) = std::env::var_os("FERRULE_CODEGEN_EVIDENCE_DIR") {
            let root = PathBuf::from(root);
            assert!(
                root.is_dir(),
                "configured evidence root must exist: {}",
                root.display()
            );
            root
        } else {
            let root = std::env::temp_dir().join("ferrule-codegen-evidence");
            fs::create_dir_all(&root).expect("create portable generated-test evidence root");
            root
        };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("generated-test evidence clock precedes Unix epoch")
            .as_nanos();
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = root.join(format!("{tag}-{}-{nonce}-{serial}", std::process::id()));
        fs::create_dir(&directory).unwrap_or_else(|error| {
            panic!(
                "create exclusive generated-test evidence {}: {error}",
                directory.display()
            )
        });
        Self { directory }
    }

    pub fn path(&self) -> &Path {
        &self.directory
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        ArtifactPath::new(relative).expect("canonical test evidence path");
        let path = self.directory.join(relative);
        fs::create_dir_all(path.parent().unwrap()).expect("create evidence parent");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap_or_else(|error| {
                panic!("create exclusive evidence {}: {error}", path.display())
            });
        file.write_all(bytes)
            .expect("retain complete evidence bytes");
        file.flush().expect("flush complete evidence bytes");
        eprintln!(
            "evidence path={} bytes={} fnv1a64={:016x}",
            path.display(),
            bytes.len(),
            hash(bytes)
        );
    }

    pub fn debug<T: Debug>(&self, relative: &str, value: &T) {
        self.write(relative, format!("{value:#?}\n").as_bytes());
    }

    pub fn bytes<B: AsRef<[u8]>, E: Debug>(&self, relative: &str, observed: &Result<B, E>) {
        match observed {
            Ok(bytes) => self.write(relative, bytes.as_ref()),
            Err(error) => self.debug(&format!("{relative}.READ_ERROR.original.txt"), error),
        }
    }

    pub fn artifacts<E: Error + Debug>(&self, label: &str, observed: &Result<ArtifactSet, E>) {
        let manifest = match observed {
            Ok(artifacts) => {
                let files = artifacts
                    .files()
                    .iter()
                    .map(|file| {
                        let relative = format!("{label}/artifacts/{}", file.path.as_str());
                        self.bytes(
                            &relative,
                            &Ok::<_, std::io::Error>(file.contents.as_slice()),
                        );
                        serde_json::json!({"path": file.path.as_str(), "bytes": file.contents.len(),
                        "fnv1a64": format!("{:016x}", hash(&file.contents))})
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({"status": "ok", "count": files.len(), "files": files,
                    "total_bytes": artifacts.files().iter().map(|file| file.contents.len()).sum::<usize>()})
            }
            Err(error) => {
                let mut sources = Vec::new();
                let mut current = error.source();
                while let Some(source) = current {
                    sources.push(serde_json::json!({"debug": format!("{source:#?}"),
                        "display": source.to_string()}));
                    current = source.source();
                }
                let original = serde_json::json!({"debug": format!("{error:#?}"),
                    "display": error.to_string(), "source_chain": sources});
                self.write(
                    &format!("{label}/ERROR.original.json"),
                    &serde_json::to_vec_pretty(&original).unwrap(),
                );
                serde_json::json!({"status": "error", "count": 0, "error": "ERROR.original.json"})
            }
        };
        self.write(
            &format!("{label}/MANIFEST.original.json"),
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        eprintln!(
            "artifact observation={label} status={} files={} manifest={}",
            manifest["status"],
            manifest["count"],
            self.directory
                .join(format!("{label}/MANIFEST.original.json"))
                .display()
        );
    }

    // Called after an intentionally caught comparison failure. Full body equality
    // verifies retention; the explicitly noncryptographic hash is a log summary.
    pub fn assert_retained(&self, label: &str, artifacts: &ArtifactSet) {
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(
                self.directory
                    .join(format!("{label}/MANIFEST.original.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["status"], "ok");
        assert_eq!(manifest["count"], artifacts.files().len());
        let files = manifest["files"].as_array().unwrap();
        assert_eq!(files.len(), artifacts.files().len());
        for (entry, actual) in files.iter().zip(artifacts.files()) {
            assert_eq!(entry["path"], actual.path.as_str());
            let bytes = fs::read(
                self.directory
                    .join(format!("{label}/artifacts/{}", actual.path)),
            )
            .unwrap();
            assert_eq!(bytes, actual.contents);
            assert_eq!(entry["bytes"], bytes.len());
            assert_eq!(entry["fnv1a64"], format!("{:016x}", hash(&bytes)));
        }
    }
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |value, byte| {
        (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
