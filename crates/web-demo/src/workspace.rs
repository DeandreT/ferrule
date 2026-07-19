//! Browser-independent assets for an in-memory mapping workspace.
//!
//! Browser file pickers, drag-and-drop, IndexedDB, and downloads belong to the
//! host layer. This module owns the portable validation rules shared by those
//! integrations.

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;

use serde::de::Error as _;
use serde::ser::SerializeStruct as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const WORKSPACE_DOCUMENT_VERSION: u32 = 1;
pub const DEFAULT_MAX_ASSET_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceLimits {
    pub max_asset_bytes: usize,
    pub max_total_bytes: usize,
}

impl WorkspaceLimits {
    pub fn new(max_asset_bytes: usize, max_total_bytes: usize) -> Result<Self, WorkspaceError> {
        let limits = Self {
            max_asset_bytes,
            max_total_bytes,
        };
        limits.validate()?;
        Ok(limits)
    }

    fn validate(self) -> Result<(), WorkspaceError> {
        if self.max_asset_bytes == 0 {
            return Err(WorkspaceError::InvalidLimits(
                "the per-asset byte limit must be greater than zero",
            ));
        }
        if self.max_total_bytes == 0 {
            return Err(WorkspaceError::InvalidLimits(
                "the total byte limit must be greater than zero",
            ));
        }
        Ok(())
    }
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        Self {
            max_asset_bytes: DEFAULT_MAX_ASSET_BYTES,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
        }
    }
}

/// A normalized, relative path within one browser workspace.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LogicalPath(String);

impl LogicalPath {
    pub fn new(path: impl AsRef<str>) -> Result<Self, WorkspaceError> {
        let original = path.as_ref();
        if original.is_empty() {
            return Err(invalid_path(original, "the path is empty"));
        }
        if original.contains('\0') {
            return Err(invalid_path(original, "the path contains a NUL byte"));
        }

        let portable = original.replace('\\', "/");
        if portable.starts_with('/') {
            return Err(invalid_path(original, "absolute paths are not allowed"));
        }

        let mut segments = Vec::new();
        for segment in portable.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    return Err(invalid_path(
                        original,
                        "parent-directory traversal is not allowed",
                    ));
                }
                value => segments.push(value),
            }
        }
        let Some(first) = segments.first() else {
            return Err(invalid_path(original, "the path has no filename"));
        };
        if first.as_bytes().get(1) == Some(&b':')
            && first
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
        {
            return Err(invalid_path(
                original,
                "drive-prefixed paths are not allowed",
            ));
        }

        Ok(Self(segments.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for LogicalPath {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for LogicalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LogicalPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = String::deserialize(deserializer)?;
        Self::new(path).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetRole {
    Unassigned,
    Project,
    PrimaryInput,
    SourceSchema,
    TargetSchema,
    NamedSource { name: String },
    ProjectDependency,
}

impl AssetRole {
    fn validate(&self) -> Result<(), WorkspaceError> {
        if let Self::NamedSource { name } = self
            && name.trim().is_empty()
        {
            return Err(WorkspaceError::InvalidRole(
                "a named source role requires a non-empty name",
            ));
        }
        Ok(())
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Project, Self::Project)
            | (Self::PrimaryInput, Self::PrimaryInput)
            | (Self::SourceSchema, Self::SourceSchema)
            | (Self::TargetSchema, Self::TargetSchema) => true,
            (Self::NamedSource { name: left }, Self::NamedSource { name: right }) => left == right,
            (Self::Unassigned | Self::ProjectDependency, _)
            | (_, Self::Unassigned | Self::ProjectDependency) => false,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetMediaType {
    FerruleProject,
    MapForceDesign,
    Xml,
    XmlSchema,
    Json,
    JsonSchema,
    JsonLines,
    Csv,
    FixedWidth,
    FlexText,
    Edi,
    Xbrl,
    Xlsx,
    Protobuf,
    Pdf,
    PlainText,
    Binary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAsset {
    path: LogicalPath,
    role: AssetRole,
    media_type: AssetMediaType,
    bytes: Vec<u8>,
}

impl WorkspaceAsset {
    pub fn new(
        path: impl AsRef<str>,
        role: AssetRole,
        media_type: AssetMediaType,
        bytes: Vec<u8>,
    ) -> Result<Self, WorkspaceError> {
        role.validate()?;
        Ok(Self {
            path: LogicalPath::new(path)?,
            role,
            media_type,
            bytes,
        })
    }

    pub fn path(&self) -> &LogicalPath {
        &self.path
    }

    pub fn role(&self) -> &AssetRole {
        &self.role
    }

    pub fn media_type(&self) -> AssetMediaType {
        self.media_type
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    limits: WorkspaceLimits,
    total_bytes: usize,
    assets: BTreeMap<LogicalPath, WorkspaceAsset>,
}

impl Workspace {
    pub fn new(limits: WorkspaceLimits) -> Result<Self, WorkspaceError> {
        limits.validate()?;
        Ok(Self {
            limits,
            total_bytes: 0,
            assets: BTreeMap::new(),
        })
    }

    pub fn version(&self) -> u32 {
        WORKSPACE_DOCUMENT_VERSION
    }

    pub fn limits(&self) -> WorkspaceLimits {
        self.limits
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn assets(&self) -> impl ExactSizeIterator<Item = &WorkspaceAsset> {
        self.assets.values()
    }

    pub fn get(&self, path: &LogicalPath) -> Option<&WorkspaceAsset> {
        self.assets.get(path)
    }

    pub fn find(&self, path: impl AsRef<str>) -> Result<Option<&WorkspaceAsset>, WorkspaceError> {
        let path = LogicalPath::new(path)?;
        Ok(self.assets.get(&path))
    }

    pub fn asset_for_role(&self, role: &AssetRole) -> Option<&WorkspaceAsset> {
        self.assets.values().find(|asset| asset.role() == role)
    }

    pub fn insert(
        &mut self,
        path: impl AsRef<str>,
        role: AssetRole,
        media_type: AssetMediaType,
        bytes: Vec<u8>,
    ) -> Result<&WorkspaceAsset, WorkspaceError> {
        self.insert_asset(WorkspaceAsset::new(path, role, media_type, bytes)?)
    }

    pub fn insert_asset(
        &mut self,
        asset: WorkspaceAsset,
    ) -> Result<&WorkspaceAsset, WorkspaceError> {
        asset.role.validate()?;
        if self.assets.contains_key(asset.path()) {
            return Err(WorkspaceError::DuplicatePath(asset.path.clone()));
        }
        self.validate_role_available(asset.path(), asset.role())?;
        let next_total = self.validate_size(asset.path(), asset.bytes.len())?;
        let path = asset.path.clone();
        self.assets.insert(path.clone(), asset);
        self.total_bytes = next_total;
        self.assets
            .get(&path)
            .ok_or(WorkspaceError::InternalInvariant)
    }

    pub fn remove(
        &mut self,
        path: impl AsRef<str>,
    ) -> Result<Option<WorkspaceAsset>, WorkspaceError> {
        let path = LogicalPath::new(path)?;
        let removed = self.assets.remove(&path);
        if let Some(asset) = &removed {
            self.total_bytes -= asset.bytes.len();
        }
        Ok(removed)
    }

    pub fn assign_role(
        &mut self,
        path: impl AsRef<str>,
        role: AssetRole,
    ) -> Result<(), WorkspaceError> {
        role.validate()?;
        let path = LogicalPath::new(path)?;
        if !self.assets.contains_key(&path) {
            return Err(WorkspaceError::MissingPath(path));
        }
        self.validate_role_available(&path, &role)?;
        let asset = self
            .assets
            .get_mut(&path)
            .ok_or(WorkspaceError::InternalInvariant)?;
        asset.role = role;
        Ok(())
    }

    pub fn clear_role(&mut self, path: impl AsRef<str>) -> Result<(), WorkspaceError> {
        self.assign_role(path, AssetRole::Unassigned)
    }

    fn validate_size(
        &self,
        path: &LogicalPath,
        asset_bytes: usize,
    ) -> Result<usize, WorkspaceError> {
        if asset_bytes > self.limits.max_asset_bytes {
            return Err(WorkspaceError::AssetTooLarge {
                path: path.clone(),
                bytes: asset_bytes,
                limit: self.limits.max_asset_bytes,
            });
        }
        let total =
            self.total_bytes
                .checked_add(asset_bytes)
                .ok_or(WorkspaceError::WorkspaceTooLarge {
                    bytes: usize::MAX,
                    limit: self.limits.max_total_bytes,
                })?;
        if total > self.limits.max_total_bytes {
            return Err(WorkspaceError::WorkspaceTooLarge {
                bytes: total,
                limit: self.limits.max_total_bytes,
            });
        }
        Ok(total)
    }

    fn validate_role_available(
        &self,
        path: &LogicalPath,
        role: &AssetRole,
    ) -> Result<(), WorkspaceError> {
        let conflict = self
            .assets
            .values()
            .find(|asset| asset.path() != path && asset.role().conflicts_with(role));
        if let Some(asset) = conflict {
            return Err(WorkspaceError::RoleConflict {
                role: role.clone(),
                existing: asset.path.clone(),
            });
        }
        Ok(())
    }
}

impl Serialize for Workspace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Workspace", 3)?;
        state.serialize_field("version", &WORKSPACE_DOCUMENT_VERSION)?;
        state.serialize_field("limits", &self.limits)?;
        state.serialize_field("assets", &self.assets.values().collect::<Vec<_>>())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Workspace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WorkspaceDocument {
            version: u32,
            #[serde(default)]
            limits: WorkspaceLimits,
            #[serde(default)]
            assets: Vec<WorkspaceAsset>,
        }

        let document = WorkspaceDocument::deserialize(deserializer)?;
        if document.version != WORKSPACE_DOCUMENT_VERSION {
            return Err(D::Error::custom(WorkspaceError::UnsupportedVersion {
                found: document.version,
                supported: WORKSPACE_DOCUMENT_VERSION,
            }));
        }
        let mut workspace = Workspace::new(document.limits).map_err(D::Error::custom)?;
        for asset in document.assets {
            workspace.insert_asset(asset).map_err(D::Error::custom)?;
        }
        Ok(workspace)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceError {
    InvalidLimits(&'static str),
    InvalidPath {
        path: String,
        reason: &'static str,
    },
    InvalidRole(&'static str),
    DuplicatePath(LogicalPath),
    MissingPath(LogicalPath),
    RoleConflict {
        role: AssetRole,
        existing: LogicalPath,
    },
    AssetTooLarge {
        path: LogicalPath,
        bytes: usize,
        limit: usize,
    },
    WorkspaceTooLarge {
        bytes: usize,
        limit: usize,
    },
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    InternalInvariant,
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits(reason) => write!(formatter, "invalid workspace limits: {reason}"),
            Self::InvalidPath { path, reason } => {
                write!(formatter, "invalid workspace path `{path}`: {reason}")
            }
            Self::InvalidRole(reason) => write!(formatter, "invalid asset role: {reason}"),
            Self::DuplicatePath(path) => {
                write!(formatter, "workspace asset `{path}` already exists")
            }
            Self::MissingPath(path) => write!(formatter, "workspace asset `{path}` does not exist"),
            Self::RoleConflict { role, existing } => write!(
                formatter,
                "asset role {role:?} is already assigned to `{existing}`"
            ),
            Self::AssetTooLarge { path, bytes, limit } => write!(
                formatter,
                "workspace asset `{path}` is {bytes} bytes, exceeding the {limit}-byte limit"
            ),
            Self::WorkspaceTooLarge { bytes, limit } => write!(
                formatter,
                "workspace assets total {bytes} bytes, exceeding the {limit}-byte limit"
            ),
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "workspace document version {found} is unsupported; expected {supported}"
            ),
            Self::InternalInvariant => formatter.write_str("workspace index is inconsistent"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

fn invalid_path(path: &str, reason: &'static str) -> WorkspaceError {
    WorkspaceError::InvalidPath {
        path: path.to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(path: &str, role: AssetRole, bytes: usize) -> WorkspaceAsset {
        WorkspaceAsset::new(path, role, AssetMediaType::Xml, vec![b'x'; bytes])
            .expect("test asset is valid")
    }

    #[test]
    fn logical_paths_are_normalized_for_portable_lookup() {
        let path = LogicalPath::new(r"schemas\\orders//./order.xsd")
            .expect("portable relative path is valid");
        assert_eq!(path.as_str(), "schemas/orders/order.xsd");

        let mut workspace = Workspace::default();
        workspace
            .insert_asset(asset(path.as_str(), AssetRole::SourceSchema, 1))
            .expect("asset fits");
        assert!(
            workspace
                .find("schemas/./orders/order.xsd")
                .expect("lookup path is valid")
                .is_some()
        );
    }

    #[test]
    fn unsafe_or_empty_paths_are_rejected() {
        for path in [
            "",
            ".",
            "..",
            "../data.xml",
            "a/../data.xml",
            "/data.xml",
            r"C:\data.xml",
            "a\0b",
        ] {
            assert!(
                matches!(
                    LogicalPath::new(path),
                    Err(WorkspaceError::InvalidPath { .. })
                ),
                "`{path}` should be rejected"
            );
        }
    }

    #[test]
    fn normalized_duplicates_are_rejected_without_changing_totals() {
        let mut workspace = Workspace::default();
        workspace
            .insert_asset(asset("data/input.xml", AssetRole::PrimaryInput, 3))
            .expect("first asset is inserted");
        let error = workspace
            .insert_asset(asset("data/./input.xml", AssetRole::Unassigned, 5))
            .expect_err("normalized duplicate must fail");

        assert!(matches!(error, WorkspaceError::DuplicatePath(_)));
        assert_eq!(workspace.len(), 1);
        assert_eq!(workspace.total_bytes(), 3);
    }

    #[test]
    fn per_asset_and_total_limits_are_enforced_atomically() {
        let mut workspace =
            Workspace::new(WorkspaceLimits::new(5, 8).expect("test limits are valid"))
                .expect("workspace is valid");
        workspace
            .insert_asset(asset("one.xml", AssetRole::Unassigned, 4))
            .expect("first asset fits");
        assert!(matches!(
            workspace.insert_asset(asset("large.xml", AssetRole::Unassigned, 6)),
            Err(WorkspaceError::AssetTooLarge { .. })
        ));
        assert!(matches!(
            workspace.insert_asset(asset("total.xml", AssetRole::Unassigned, 5)),
            Err(WorkspaceError::WorkspaceTooLarge { .. })
        ));
        assert_eq!(workspace.len(), 1);
        assert_eq!(workspace.total_bytes(), 4);
    }

    #[test]
    fn exclusive_roles_and_named_sources_cannot_be_ambiguous() {
        let mut workspace = Workspace::default();
        workspace
            .insert_asset(asset("one.xml", AssetRole::PrimaryInput, 1))
            .expect("primary input is assigned");
        workspace
            .insert_asset(asset(
                "catalog.xml",
                AssetRole::NamedSource {
                    name: "catalog".into(),
                },
                1,
            ))
            .expect("named source is assigned");

        assert!(matches!(
            workspace.insert_asset(asset("two.xml", AssetRole::PrimaryInput, 1)),
            Err(WorkspaceError::RoleConflict { .. })
        ));
        assert!(matches!(
            workspace.insert_asset(asset(
                "other.xml",
                AssetRole::NamedSource {
                    name: "catalog".into()
                },
                1
            )),
            Err(WorkspaceError::RoleConflict { .. })
        ));
    }

    #[test]
    fn roles_can_be_cleared_and_reassigned_without_moving_assets() {
        let mut workspace = Workspace::default();
        workspace
            .insert_asset(asset("one.xml", AssetRole::PrimaryInput, 1))
            .expect("first asset is inserted");
        workspace
            .insert_asset(asset("two.xml", AssetRole::Unassigned, 1))
            .expect("second asset is inserted");
        assert!(matches!(
            workspace.assign_role("two.xml", AssetRole::PrimaryInput),
            Err(WorkspaceError::RoleConflict { .. })
        ));

        workspace.clear_role("one.xml").expect("role is cleared");
        workspace
            .assign_role("two.xml", AssetRole::PrimaryInput)
            .expect("role is reassigned");
        assert_eq!(
            workspace
                .asset_for_role(&AssetRole::PrimaryInput)
                .map(WorkspaceAsset::path),
            Some(&LogicalPath::new("two.xml").expect("test path is valid"))
        );
    }

    #[test]
    fn removal_updates_the_byte_budget() {
        let mut workspace =
            Workspace::new(WorkspaceLimits::new(5, 5).expect("test limits are valid"))
                .expect("workspace is valid");
        workspace
            .insert_asset(asset("one.xml", AssetRole::Unassigned, 5))
            .expect("asset fills budget");
        let removed = workspace
            .remove("one.xml")
            .expect("path is valid")
            .expect("asset exists");
        assert_eq!(removed.bytes().len(), 5);
        assert_eq!(workspace.total_bytes(), 0);
        workspace
            .insert_asset(asset("two.xml", AssetRole::Unassigned, 5))
            .expect("released budget can be reused");
    }

    #[test]
    fn serialized_workspaces_roundtrip_through_all_validation() {
        let mut workspace =
            Workspace::new(WorkspaceLimits::new(32, 64).expect("test limits are valid"))
                .expect("workspace is valid");
        workspace
            .insert(
                "mapping/project.json",
                AssetRole::Project,
                AssetMediaType::FerruleProject,
                br#"{}"#.to_vec(),
            )
            .expect("project asset is inserted");
        let json = serde_json::to_string(&workspace).expect("workspace serializes");
        let decoded: Workspace = serde_json::from_str(&json).expect("workspace deserializes");
        assert_eq!(decoded, workspace);
        assert_eq!(decoded.version(), WORKSPACE_DOCUMENT_VERSION);
    }

    #[test]
    fn deserialization_rejects_versions_duplicates_and_limit_bypasses() {
        let unsupported = r#"{"version":2,"assets":[]}"#;
        let error = serde_json::from_str::<Workspace>(unsupported)
            .expect_err("unsupported version must fail");
        assert!(error.to_string().contains("version 2 is unsupported"));

        let duplicate = r#"{
            "version": 1,
            "assets": [
                {"path":"a/./b.xml","role":{"kind":"unassigned"},"media_type":"xml","bytes":[]},
                {"path":"a/b.xml","role":{"kind":"unassigned"},"media_type":"xml","bytes":[]}
            ]
        }"#;
        assert!(
            serde_json::from_str::<Workspace>(duplicate)
                .expect_err("normalized duplicate must fail")
                .to_string()
                .contains("already exists")
        );

        let oversized = r#"{
            "version": 1,
            "limits": {"max_asset_bytes":2,"max_total_bytes":2},
            "assets": [
                {"path":"a.xml","role":{"kind":"unassigned"},"media_type":"xml","bytes":[1,2,3]}
            ]
        }"#;
        assert!(
            serde_json::from_str::<Workspace>(oversized)
                .expect_err("serialized bytes cannot bypass limits")
                .to_string()
                .contains("exceeding the 2-byte limit")
        );
    }
}
