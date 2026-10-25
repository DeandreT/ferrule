//! Compatibility manifests recorded beside generated libraries.
//!
//! A manifest is opt-in through [`ManifestSelection::Included`], so default
//! generation keeps its artifact inventory. It records the generator, the
//! runtime the library was generated for (identified by a hash of the runtime
//! sources), the generation profile and the library identities. The generated
//! library carries the same record and refuses an incompatible runtime itself;
//! hosts and tools read the file through [`CompatibilityManifest::from_json`].

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Path of the manifest inside a generated artifact tree.
pub const MANIFEST_PATH: &str = "ferrule-manifest.json";
/// The only manifest format this generator writes and reads.
pub const MANIFEST_FORMAT: &str = "ferrule-generated-library";
pub const MANIFEST_FORMAT_VERSION: u32 = 1;
pub const GENERATOR_NAME: &str = "ferrule";
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether one generation request adds the compatibility manifest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ManifestSelection {
    /// The existing artifact inventory, without a manifest.
    #[default]
    Omitted,
    /// Add `ferrule-manifest.json` and the generated compatibility check.
    Included,
}

/// A SHA-256 identity of runtime sources: `sha256:` and 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RuntimeIdentity(String);

impl RuntimeIdentity {
    const PREFIX: &'static str = "sha256:";

    /// The identity of `sources`, independent of their order: each source
    /// contributes its path, a NUL, its little-endian 64-bit length and its
    /// bytes, in path order.
    pub fn of_sources<'a>(sources: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Self {
        let mut sources = sources.into_iter().collect::<Vec<_>>();
        sources.sort_by(|left, right| left.0.cmp(right.0));
        let mut hasher = Sha256::new();
        for (path, contents) in sources {
            hasher.update(path.as_bytes());
            hasher.update([0]);
            hasher.update((contents.len() as u64).to_le_bytes());
            hasher.update(contents);
        }
        let mut identity = String::from(Self::PREFIX);
        for byte in hasher.finalize() {
            identity.push_str(&format!("{byte:02x}"));
        }
        Self(identity)
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        let digits = value.strip_prefix(Self::PREFIX);
        if digits.is_some_and(|digits| {
            digits.len() == 64
                && digits
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            Ok(Self(value.to_owned()))
        } else {
            Err(ManifestError::InvalidRuntimeIdentity {
                found: value.to_owned(),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RuntimeIdentity {
    type Error = ManifestError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<RuntimeIdentity> for String {
    fn from(identity: RuntimeIdentity) -> Self {
        identity.0
    }
}

impl fmt::Display for RuntimeIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The generator that wrote a library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorRecord {
    pub name: String,
    pub version: String,
}

impl GeneratorRecord {
    /// This generator.
    pub fn current() -> Self {
        Self {
            name: GENERATOR_NAME.to_owned(),
            version: GENERATOR_VERSION.to_owned(),
        }
    }
}

/// How a generated library obtains its runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeBinding {
    /// The runtime sources are part of the generated library.
    Vendored,
    /// A declared local runtime package.
    Path { path: String },
    /// A versioned runtime package.
    Version { version: String },
    /// A runtime package at a pinned repository revision.
    Git {
        repository: String,
        revision: String,
    },
}

/// The runtime a library was generated for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRecord {
    pub package: String,
    pub version: String,
    pub identity: RuntimeIdentity,
    pub binding: RuntimeBinding,
}

/// The API families and companions one generation selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationProfile {
    /// Typed entry points with the complete JSON/XML companion inventory.
    Complete,
    /// Typed entry points only.
    TypedOnly,
    CsvOutput,
    CsvDocument,
    RawDelimitedDocument,
    Json5,
    X12,
    StaticDocuments,
    CsvX12,
    CsvJson,
    /// Primary filename document runs with complete CSV documents.
    PrimaryCsvDocument,
    /// Primary filename document runs with unescaped CRLF text output.
    PrimaryRawDelimitedDocument,
    /// Primary filename document runs with complete XML documents.
    PrimaryXmlDocument,
    /// Primary filename document runs with complete strict JSON documents.
    PrimaryJsonDocument,
    PrimaryFixedWidthDocument,
    PrimaryEdiDocument,
}

/// The names a generated library is hosted under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "language", deny_unknown_fields)]
pub enum LibraryIdentity {
    #[serde(rename = "rust")]
    Rust {
        package_name: String,
        crate_name: String,
    },
    #[serde(rename = "csharp")]
    CSharp {
        assembly_name: String,
        root_namespace: String,
        mapping_class: String,
    },
}

/// The compatibility record of one generated library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityManifest {
    pub format: String,
    pub format_version: u32,
    pub generator: GeneratorRecord,
    pub runtime: RuntimeRecord,
    pub profile: GenerationProfile,
    pub library: LibraryIdentity,
}

/// The fields every manifest version shares, read before the complete record.
#[derive(Deserialize)]
struct ManifestHeader {
    format: String,
    format_version: u32,
}

impl CompatibilityManifest {
    /// A manifest of the current format written by this generator.
    pub fn new(
        runtime: RuntimeRecord,
        profile: GenerationProfile,
        library: LibraryIdentity,
    ) -> Self {
        Self {
            format: MANIFEST_FORMAT.to_owned(),
            format_version: MANIFEST_FORMAT_VERSION,
            generator: GeneratorRecord::current(),
            runtime,
            profile,
            library,
        }
    }

    /// Deterministic pretty JSON with a trailing newline.
    pub fn to_json(&self) -> Result<String, ManifestError> {
        let mut json = serde_json::to_string_pretty(self)
            .map_err(|error| ManifestError::Malformed(error.to_string()))?;
        json.push('\n');
        Ok(json)
    }

    /// Reads a manifest, refusing another format or format version before
    /// interpreting the rest of the record.
    pub fn from_json(text: &str) -> Result<Self, ManifestError> {
        let header: ManifestHeader = serde_json::from_str(text)
            .map_err(|error| ManifestError::Malformed(error.to_string()))?;
        if header.format != MANIFEST_FORMAT {
            return Err(ManifestError::UnsupportedFormat {
                found: header.format,
            });
        }
        if header.format_version != MANIFEST_FORMAT_VERSION {
            return Err(ManifestError::UnsupportedFormatVersion {
                found: header.format_version,
            });
        }
        serde_json::from_str(text).map_err(|error| ManifestError::Malformed(error.to_string()))
    }

    /// Refuses a library generated for another runtime than `available`.
    pub fn require_runtime(&self, available: &RuntimeIdentity) -> Result<(), CompatibilityRefusal> {
        if &self.runtime.identity == available {
            Ok(())
        } else {
            Err(CompatibilityRefusal::RuntimeMismatch {
                required: self.runtime.identity.clone(),
                available: available.clone(),
            })
        }
    }
}

/// A manifest that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    Malformed(String),
    UnsupportedFormat { found: String },
    UnsupportedFormatVersion { found: u32 },
    InvalidRuntimeIdentity { found: String },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(message) => {
                write!(formatter, "malformed generated-library manifest: {message}")
            }
            Self::UnsupportedFormat { found } => write!(
                formatter,
                "unsupported generated-library manifest format {found:?}; expected {MANIFEST_FORMAT:?}"
            ),
            Self::UnsupportedFormatVersion { found } => write!(
                formatter,
                "unsupported generated-library manifest version {found}; this generator reads version {MANIFEST_FORMAT_VERSION}"
            ),
            Self::InvalidRuntimeIdentity { found } => write!(
                formatter,
                "invalid runtime identity {found:?}; expected sha256: and 64 lowercase hex digits"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// A readable manifest whose library is incompatible with what is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityRefusal {
    RuntimeMismatch {
        required: RuntimeIdentity,
        available: RuntimeIdentity,
    },
}

impl fmt::Display for CompatibilityRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeMismatch {
                required,
                available,
            } => write!(
                formatter,
                "generated library requires ferrule runtime {required}, but runtime {available} is bound"
            ),
        }
    }
}

impl std::error::Error for CompatibilityRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> CompatibilityManifest {
        CompatibilityManifest::new(
            RuntimeRecord {
                package: "Ferrule.Runtime".into(),
                version: GENERATOR_VERSION.into(),
                identity: RuntimeIdentity::of_sources([("Runtime/A.cs", b"class A {}".as_slice())]),
                binding: RuntimeBinding::Vendored,
            },
            GenerationProfile::Complete,
            LibraryIdentity::CSharp {
                assembly_name: "Acme.Orders".into(),
                root_namespace: "Acme.Orders".into(),
                mapping_class: "OrderMapping".into(),
            },
        )
    }

    #[test]
    fn manifest_round_trips_through_its_typed_json() {
        let manifest = manifest();
        let json = manifest.to_json().unwrap();
        assert!(json.ends_with("}\n"));
        assert!(json.contains("\"language\": \"csharp\""));
        assert!(json.contains("\"kind\": \"vendored\""));
        assert_eq!(CompatibilityManifest::from_json(&json).unwrap(), manifest);
    }

    #[test]
    fn identity_is_independent_of_source_order_and_sensitive_to_content() {
        let a = ("a.cs", b"one".as_slice());
        let b = ("b.cs", b"two".as_slice());
        let forward = RuntimeIdentity::of_sources([a, b]);
        assert_eq!(forward, RuntimeIdentity::of_sources([b, a]));
        assert_ne!(
            forward,
            RuntimeIdentity::of_sources([a, ("b.cs", b"tw0".as_slice())])
        );
        assert_ne!(
            forward,
            RuntimeIdentity::of_sources([("a.cs", b"onetwo".as_slice())])
        );
        assert_eq!(RuntimeIdentity::parse(forward.as_str()).unwrap(), forward);
    }

    #[test]
    fn unsupported_or_malformed_manifests_are_refused_explicitly() {
        let json = manifest().to_json().unwrap();
        let other = json.replace(MANIFEST_FORMAT, "other-format");
        assert_eq!(
            CompatibilityManifest::from_json(&other),
            Err(ManifestError::UnsupportedFormat {
                found: "other-format".into()
            })
        );
        let newer = json.replace("\"format_version\": 1", "\"format_version\": 2");
        assert_eq!(
            CompatibilityManifest::from_json(&newer),
            Err(ManifestError::UnsupportedFormatVersion { found: 2 })
        );
        let identity = manifest().runtime.identity;
        let bad = json.replace(identity.as_str(), "sha256:XYZ");
        assert!(matches!(
            CompatibilityManifest::from_json(&bad),
            Err(ManifestError::Malformed(message)) if message.contains("invalid runtime identity")
        ));
        let extra = json.replacen('{', "{\"unexpected\": true,", 1);
        assert!(matches!(
            CompatibilityManifest::from_json(&extra),
            Err(ManifestError::Malformed(_))
        ));
    }

    #[test]
    fn a_library_for_another_runtime_is_refused() {
        let manifest = manifest();
        manifest
            .require_runtime(&manifest.runtime.identity)
            .unwrap();
        let other = RuntimeIdentity::of_sources([("Runtime/A.cs", b"class B {}".as_slice())]);
        assert_eq!(
            manifest.require_runtime(&other),
            Err(CompatibilityRefusal::RuntimeMismatch {
                required: manifest.runtime.identity.clone(),
                available: other,
            })
        );
    }
}
