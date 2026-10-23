//! Generated Rust package identity and runtime binding.
//!
//! A generated Rust library is hosted under its package name; every generated
//! item is scoped by the crate, so distinct package names are the whole
//! multi-mapping contract. The runtime is bound through the Cargo dependency of
//! [`crate::Options`]; with a compatibility manifest the library also records
//! the runtime identity it requires and refuses to compile against another.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use codegen::{IdentityError, IdentityKind, IdentityViolation, ManifestError, RuntimeIdentity};

#[path = "../../codegen-runtime/identity.rs"]
mod runtime_sources;

/// Source identity of the `codegen-runtime` this generator emits code for.
pub const RUNTIME_IDENTITY: &str = env!("FERRULE_GENERATOR_RUNTIME_IDENTITY");

/// The longest accepted package name, matching the crates.io limit.
pub const MAX_PACKAGE_NAME_BYTES: usize = 64;

/// Rust keywords, which are invalid crate names.
const KEYWORDS: [&str; 52] = [
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Crate names reserved by Rust, Cargo's artifact directories, and the
/// runtime dependency every generated package declares.
const RESERVED: [&str; 10] = [
    "alloc",
    "build",
    "codegen_runtime",
    "core",
    "deps",
    "examples",
    "incremental",
    "proc_macro",
    "std",
    "test",
];

/// Windows device names, which cannot name a package directory.
const DEVICE_NAMES: [&str; 4] = ["con", "prn", "aux", "nul"];

/// The `codegen-runtime` identity generated mappings require.
pub fn runtime_identity() -> Result<RuntimeIdentity, ManifestError> {
    RuntimeIdentity::parse(RUNTIME_IDENTITY)
}

/// A validated generated package name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageName(String);

impl PackageName {
    /// Accepts ASCII letters, digits, `-` and `_`, starting with a letter,
    /// whose crate name is not a keyword or reserved name.
    pub fn new(name: &str) -> Result<Self, IdentityError> {
        let refuse = |violation| {
            Err(IdentityError::new(
                IdentityKind::RustPackageName,
                name,
                violation,
            ))
        };
        let Some(first) = name.chars().next() else {
            return refuse(IdentityViolation::Empty);
        };
        if name.len() > MAX_PACKAGE_NAME_BYTES {
            return refuse(IdentityViolation::TooLong {
                maximum: MAX_PACKAGE_NAME_BYTES,
            });
        }
        if !first.is_ascii_alphabetic() {
            return refuse(IdentityViolation::InvalidStart { character: first });
        }
        if let Some(character) = name.chars().find(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        }) {
            return refuse(IdentityViolation::InvalidCharacter { character });
        }
        let crate_name = name.replace('-', "_");
        if let Some(keyword) = KEYWORDS.iter().find(|keyword| **keyword == crate_name) {
            return refuse(IdentityViolation::Keyword {
                keyword: (*keyword).to_owned(),
            });
        }
        let lower = crate_name.to_ascii_lowercase();
        let device = DEVICE_NAMES.contains(&lower.as_str())
            || ["com", "lpt"].iter().any(|prefix| {
                lower
                    .strip_prefix(prefix)
                    .is_some_and(|digit| digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit())
            });
        if device || RESERVED.contains(&crate_name.as_str()) {
            return refuse(IdentityViolation::Reserved {
                reserved: name.to_owned(),
            });
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The library crate name Cargo derives from the package name.
    pub fn crate_name(&self) -> String {
        self.0.replace('-', "_")
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A declared runtime directory that cannot be bound.
#[derive(Debug)]
pub enum RuntimeBindingError {
    Unreadable {
        path: PathBuf,
        error: io::Error,
    },
    Identity {
        path: PathBuf,
        error: ManifestError,
    },
    Mismatch {
        path: PathBuf,
        required: RuntimeIdentity,
        found: RuntimeIdentity,
    },
}

impl fmt::Display for RuntimeBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, error } => write!(
                formatter,
                "cannot read the Rust codegen runtime sources at {}: {error}",
                path.display()
            ),
            Self::Identity { path, error } => write!(
                formatter,
                "cannot identify the Rust codegen runtime at {}: {error}",
                path.display()
            ),
            Self::Mismatch {
                path,
                required,
                found,
            } => write!(
                formatter,
                "the Rust codegen runtime at {} has identity {found}, but this generator emits code for runtime {required}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RuntimeBindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { error, .. } => Some(error),
            Self::Identity { error, .. } => Some(error),
            Self::Mismatch { .. } => None,
        }
    }
}

/// The source identity of the `codegen-runtime` package at `path`.
pub fn runtime_identity_at(path: &Path) -> Result<RuntimeIdentity, RuntimeBindingError> {
    let identity = runtime_sources::runtime_source_identity(path).map_err(|error| {
        RuntimeBindingError::Unreadable {
            path: path.to_path_buf(),
            error,
        }
    })?;
    RuntimeIdentity::parse(&identity).map_err(|error| RuntimeBindingError::Identity {
        path: path.to_path_buf(),
        error,
    })
}

/// Refuses a declared runtime directory whose sources differ from the
/// runtime this generator emits code for.
pub fn verify_runtime_path(path: &Path) -> Result<RuntimeIdentity, RuntimeBindingError> {
    let found = runtime_identity_at(path)?;
    let required = runtime_identity().map_err(|error| RuntimeBindingError::Identity {
        path: path.to_path_buf(),
        error,
    })?;
    if found == required {
        Ok(found)
    } else {
        Err(RuntimeBindingError::Mismatch {
            path: path.to_path_buf(),
            required,
            found,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_follow_cargo_rules_with_typed_refusals() {
        for name in [
            "ferrule-generated-mapping",
            "orders_a",
            "Invoice-2",
            "tests",
        ] {
            assert_eq!(PackageName::new(name).unwrap().as_str(), name);
        }
        assert_eq!(
            PackageName::new("orders-mapping").unwrap().crate_name(),
            "orders_mapping"
        );
        let violation = |name: &str| PackageName::new(name).unwrap_err().violation;
        assert_eq!(violation(""), IdentityViolation::Empty);
        assert_eq!(
            violation(&"a".repeat(65)),
            IdentityViolation::TooLong { maximum: 64 }
        );
        assert_eq!(
            violation("9-invalid"),
            IdentityViolation::InvalidStart { character: '9' }
        );
        assert_eq!(
            violation("not/a/package"),
            IdentityViolation::InvalidCharacter { character: '/' }
        );
        assert_eq!(
            violation("crate"),
            IdentityViolation::Keyword {
                keyword: "crate".into()
            }
        );
        for reserved in [
            "std",
            "proc-macro",
            "codegen-runtime",
            "codegen_runtime",
            "COM1",
            "nul",
        ] {
            assert_eq!(
                violation(reserved),
                IdentityViolation::Reserved {
                    reserved: reserved.into()
                },
                "{reserved}"
            );
        }
        assert_eq!(
            PackageName::new("self").unwrap_err().to_string(),
            "invalid generated Rust package name \"self\": \"self\" is a language keyword"
        );
    }

    #[test]
    fn generator_runtime_identity_is_the_identity_of_the_workspace_runtime() {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
        assert_eq!(
            verify_runtime_path(&runtime).unwrap(),
            runtime_identity().unwrap()
        );
        assert_eq!(
            runtime_identity().unwrap().as_str(),
            codegen_runtime_identity_from_files()
        );
    }

    /// The shared build-script encoding matches the backend-neutral one.
    fn codegen_runtime_identity_from_files() -> String {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
        let mut files = Vec::new();
        for name in ["Cargo.toml", "build.rs", "identity.rs"] {
            files.push((name.to_owned(), std::fs::read(runtime.join(name)).unwrap()));
        }
        fn visit(directory: &Path, relative: &str, files: &mut Vec<(String, Vec<u8>)>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = format!("{relative}/{}", entry.file_name().to_str().unwrap());
                if entry.path().is_dir() {
                    visit(&entry.path(), &path, files);
                } else {
                    files.push((path, std::fs::read(entry.path()).unwrap()));
                }
            }
        }
        visit(&runtime.join("src"), "src", &mut files);
        RuntimeIdentity::of_sources(
            files
                .iter()
                .map(|(path, contents)| (path.as_str(), contents.as_slice())),
        )
        .as_str()
        .to_owned()
    }

    #[test]
    fn a_changed_runtime_directory_is_refused_before_binding() {
        let root =
            std::env::temp_dir().join(format!("ferrule_runtime_identity_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["Cargo.toml", "build.rs", "identity.rs"] {
            std::fs::write(root.join(name), name).unwrap();
        }
        std::fs::write(root.join("src/lib.rs"), "pub fn changed() {}").unwrap();
        let error = verify_runtime_path(&root).unwrap_err();
        assert!(matches!(
            &error,
            RuntimeBindingError::Mismatch { required, found, .. }
                if required.as_str() == RUNTIME_IDENTITY && found != required
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(matches!(
            verify_runtime_path(&root),
            Err(RuntimeBindingError::Unreadable { .. })
        ));
    }
}
