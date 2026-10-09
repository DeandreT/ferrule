use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

static NEXT: AtomicU64 = AtomicU64::new(0);
const CHILD_DATABASE: &str = "FERRULE_METADATA_CHILD_DATABASE";

pub const AUTHORED_SQL: &str = "PRAGMA journal_mode=DELETE;
CREATE TABLE Groups(GroupId INTEGER PRIMARY KEY, Name TEXT);
CREATE TABLE Records(Id INTEGER PRIMARY KEY AUTOINCREMENT,
    GroupId INTEGER REFERENCES Groups(GroupId), Label TEXT, Score REAL, Active BOOLEAN);
INSERT INTO Groups VALUES(1, 'group');";

pub struct Fixture {
    directory: PathBuf,
    evidence: PathBuf,
}

impl Fixture {
    pub fn new(tag: &str) -> Self {
        assert!(
            tag.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        );
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!(
            "ferrule-metadata-{tag}-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let directory = std::env::temp_dir().join(&name);
        fs::create_dir(&directory).unwrap();
        let root = std::env::var_os("FERRULE_METADATA_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("ferrule-metadata-evidence"));
        fs::create_dir_all(&root).unwrap();
        let evidence = root.join(name);
        fs::create_dir(&evidence).unwrap();
        eprintln!("metadata evidence={}", evidence.display());
        Self {
            directory,
            evidence,
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn database(&self) -> PathBuf {
        self.directory.join("database.sqlite")
    }

    pub fn retain(&self, relative: &str, bytes: impl AsRef<[u8]>) {
        let relative = Path::new(relative);
        assert!(
            relative
                .components()
                .all(|component| { matches!(component, std::path::Component::Normal(_)) })
        );
        let path = self.evidence.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        std::io::Write::write_all(&mut file, bytes.as_ref()).unwrap();
    }

    pub fn original<T: Debug, E: Error + Debug>(&self, name: &str, result: &Result<T, E>) {
        let mut text = format!("{result:#?}\n");
        if let Err(error) = result {
            text.push_str(&format!("display: {error}\n"));
            let mut source = error.source();
            while let Some(error) = source {
                text.push_str(&format!("cause: {error}\n"));
                source = error.source();
            }
        }
        self.retain(name, text);
    }

    pub fn snapshot(&self, label: &str) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        for entry in fs::read_dir(&self.directory).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            let name = entry.file_name().into_string().unwrap();
            let bytes = fs::read(entry.path()).unwrap();
            self.retain(&format!("{label}/{name}"), &bytes);
            files.insert(name, bytes);
        }
        self.retain(
            &format!("{label}/FILE_SIZES.original.txt"),
            format!(
                "{:#?}\n",
                files
                    .iter()
                    .map(|(name, bytes)| (name, bytes.len()))
                    .collect::<Vec<_>>()
            ),
        );
        files
    }

    pub fn author_database(&self) {
        self.retain("authored.sql", AUTHORED_SQL);
        self.retain(
            "authored-rows.txt",
            "200 Records rows: Id auto, GroupId=1, Label=('before-'+index+'-') repeated 300 times, Score=1.5, Active=1\n",
        );
        let connection = Connection::open(self.database()).unwrap();
        connection.execute_batch(AUTHORED_SQL).unwrap();
        for index in 0..200 {
            connection
                .execute(
                    "INSERT INTO Records(GroupId, Label, Score, Active) VALUES(1, ?1, 1.5, 1)",
                    [format!("before-{index}-").repeat(300)],
                )
                .unwrap();
        }
    }

    pub fn author_hot_journal(&self) {
        self.author_database();
        self.retain(
            "uncommitted.sql",
            "PRAGMA cache_size=1; PRAGMA synchronous=FULL; BEGIN IMMEDIATE; UPDATE Records SET Label='uncommitted-' || Label; process exits without committing or dropping the connection\n",
        );
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "fixture::author_hot_journal_child",
                "--nocapture",
            ])
            .env(CHILD_DATABASE, self.database())
            .output()
            .unwrap();
        self.retain("author-child.stdout.original.bin", &result.stdout);
        self.retain("author-child.stderr.original.bin", &result.stderr);
        self.retain(
            "author-child.status.original.txt",
            format!("{}\n", result.status),
        );
        assert!(result.status.success());
        let journal = fs::read(self.directory.join("database.sqlite-journal")).unwrap();
        assert!(journal.len() > 512 && journal[..8].iter().any(|byte| *byte != 0));
    }
}

#[test]
#[ignore = "fixture authoring child; invoked with an owned temporary database"]
fn author_hot_journal_child() {
    let path = PathBuf::from(std::env::var_os(CHILD_DATABASE).expect("fixture child path"));
    assert_eq!(path.file_name().unwrap(), "database.sqlite");
    assert!(
        path.parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ferrule-metadata-")
    );
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "PRAGMA cache_size=1; PRAGMA synchronous=FULL; BEGIN IMMEDIATE; \
         UPDATE Records SET Label='uncommitted-' || Label;",
        )
        .unwrap();
    // Do not run SQLite's normal rollback-on-drop; retain the original hot journal.
    std::process::exit(0);
}
