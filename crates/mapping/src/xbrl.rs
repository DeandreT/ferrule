use std::error::Error;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// Reserved prefix for distinct static XBRL unit declarations in the target
/// schema. Runtime writers serialize every such group as `xbrli:unit`.
pub const XBRL_UNIT_FIELD_PREFIX: &str = "\u{1f}ferrule-xbrl-unit-";

/// Expanded namespace identity for one schema path exposed by an XBRL entry
/// tree. Paths are relative to the `xbrl` schema root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct XbrlNamespaceBinding {
    path: Vec<String>,
    namespace: String,
}

impl XbrlNamespaceBinding {
    pub fn new(
        path: Vec<String>,
        namespace: impl Into<String>,
    ) -> Result<Self, XbrlBoundaryOptionsError> {
        if path.is_empty() || path.iter().any(|segment| segment.trim().is_empty()) {
            return Err(XbrlBoundaryOptionsError::InvalidNamespacePath);
        }
        let namespace = namespace.into();
        let namespace = namespace.trim();
        if namespace.is_empty() {
            return Err(XbrlBoundaryOptionsError::EmptyNamespace);
        }
        Ok(Self {
            path,
            namespace: namespace.to_string(),
        })
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

impl<'de> Deserialize<'de> for XbrlNamespaceBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            path: Vec<String>,
            namespace: String,
        }
        let repr = Repr::deserialize(deserializer)?;
        Self::new(repr.path, repr.namespace).map_err(serde::de::Error::custom)
    }
}

/// Numeric item category used to select the matching XBRL default unit and
/// decimals for one concrete target fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XbrlFactType {
    Monetary,
    Numeric,
    Shares,
    PerShare,
}

/// Numeric item metadata for one concrete target fact path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct XbrlFactBinding {
    path: Vec<String>,
    fact_type: XbrlFactType,
}

impl XbrlFactBinding {
    pub fn new(
        path: Vec<String>,
        fact_type: XbrlFactType,
    ) -> Result<Self, XbrlBoundaryOptionsError> {
        if path.is_empty() || path.iter().any(|segment| segment.trim().is_empty()) {
            return Err(XbrlBoundaryOptionsError::InvalidFactPath);
        }
        Ok(Self { path, fact_type })
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub const fn fact_type(&self) -> XbrlFactType {
        self.fact_type
    }
}

impl<'de> Deserialize<'de> for XbrlFactBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            path: Vec<String>,
            fact_type: XbrlFactType,
        }
        let repr = Repr::deserialize(deserializer)?;
        Self::new(repr.path, repr.fact_type).map_err(serde::de::Error::custom)
    }
}

/// Which side of a mapping is supplied by an opaque XBRL boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XbrlBoundaryMode {
    ExternalSource,
    ExternalTarget,
}

/// Validated metadata retained for executable XBRL source and target
/// boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct XbrlBoundaryOptions {
    mode: XbrlBoundaryMode,
    taxonomy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    presentation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    namespace_bindings: Vec<XbrlNamespaceBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fact_bindings: Vec<XbrlFactBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XbrlBoundaryOptionsError {
    EmptyTaxonomy,
    EmptyPresentation,
    SourcePresentation,
    InvalidNamespacePath,
    EmptyNamespace,
    DuplicateNamespacePath,
    InvalidFactPath,
    DuplicateFactPath,
    SourceFactBindings,
    MissingFactNamespace,
}

impl fmt::Display for XbrlBoundaryOptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTaxonomy => formatter.write_str("XBRL taxonomy reference cannot be empty"),
            Self::EmptyPresentation => {
                formatter.write_str("XBRL target presentation reference cannot be empty")
            }
            Self::SourcePresentation => formatter
                .write_str("an external XBRL source cannot have a target presentation reference"),
            Self::InvalidNamespacePath => {
                formatter.write_str("an XBRL namespace binding path cannot be empty")
            }
            Self::EmptyNamespace => {
                formatter.write_str("an XBRL namespace binding cannot use an empty namespace")
            }
            Self::DuplicateNamespacePath => {
                formatter.write_str("XBRL namespace binding paths must be unique")
            }
            Self::InvalidFactPath => {
                formatter.write_str("an XBRL fact binding path cannot be empty")
            }
            Self::DuplicateFactPath => {
                formatter.write_str("XBRL fact binding paths must be unique")
            }
            Self::SourceFactBindings => {
                formatter.write_str("XBRL fact bindings are valid only for target boundaries")
            }
            Self::MissingFactNamespace => formatter
                .write_str("every XBRL fact binding requires a namespace binding at the same path"),
        }
    }
}

impl Error for XbrlBoundaryOptionsError {}

impl XbrlBoundaryOptions {
    pub fn external_source(taxonomy: impl Into<String>) -> Result<Self, XbrlBoundaryOptionsError> {
        Ok(Self {
            mode: XbrlBoundaryMode::ExternalSource,
            taxonomy: validate_taxonomy(taxonomy.into())?,
            presentation: None,
            namespace_bindings: Vec::new(),
            fact_bindings: Vec::new(),
        })
    }

    pub fn external_target(
        taxonomy: impl Into<String>,
        presentation: Option<&str>,
    ) -> Result<Self, XbrlBoundaryOptionsError> {
        Ok(Self {
            mode: XbrlBoundaryMode::ExternalTarget,
            taxonomy: validate_taxonomy(taxonomy.into())?,
            presentation: presentation
                .map(str::to_owned)
                .map(validate_presentation)
                .transpose()?,
            namespace_bindings: Vec::new(),
            fact_bindings: Vec::new(),
        })
    }

    pub const fn mode(&self) -> XbrlBoundaryMode {
        self.mode
    }

    pub fn taxonomy(&self) -> &str {
        &self.taxonomy
    }

    pub fn presentation(&self) -> Option<&str> {
        self.presentation.as_deref()
    }

    pub fn with_namespace_bindings(
        mut self,
        namespace_bindings: Vec<XbrlNamespaceBinding>,
    ) -> Result<Self, XbrlBoundaryOptionsError> {
        for (index, binding) in namespace_bindings.iter().enumerate() {
            if namespace_bindings[..index]
                .iter()
                .any(|existing| existing.path == binding.path)
            {
                return Err(XbrlBoundaryOptionsError::DuplicateNamespacePath);
            }
        }
        self.namespace_bindings = namespace_bindings;
        Ok(self)
    }

    pub fn namespace_bindings(&self) -> &[XbrlNamespaceBinding] {
        &self.namespace_bindings
    }

    pub fn with_fact_bindings(
        mut self,
        fact_bindings: Vec<XbrlFactBinding>,
    ) -> Result<Self, XbrlBoundaryOptionsError> {
        if self.mode == XbrlBoundaryMode::ExternalSource && !fact_bindings.is_empty() {
            return Err(XbrlBoundaryOptionsError::SourceFactBindings);
        }
        for (index, binding) in fact_bindings.iter().enumerate() {
            if fact_bindings[..index]
                .iter()
                .any(|existing| existing.path == binding.path)
            {
                return Err(XbrlBoundaryOptionsError::DuplicateFactPath);
            }
            if !self
                .namespace_bindings
                .iter()
                .any(|namespace| namespace.path == binding.path)
            {
                return Err(XbrlBoundaryOptionsError::MissingFactNamespace);
            }
        }
        self.fact_bindings = fact_bindings;
        Ok(self)
    }

    pub fn fact_bindings(&self) -> &[XbrlFactBinding] {
        &self.fact_bindings
    }
}

fn validate_taxonomy(taxonomy: String) -> Result<String, XbrlBoundaryOptionsError> {
    let taxonomy = taxonomy.trim();
    if taxonomy.is_empty() {
        Err(XbrlBoundaryOptionsError::EmptyTaxonomy)
    } else {
        Ok(taxonomy.to_owned())
    }
}

fn validate_presentation(presentation: String) -> Result<String, XbrlBoundaryOptionsError> {
    let presentation = presentation.trim();
    if presentation.is_empty() {
        Err(XbrlBoundaryOptionsError::EmptyPresentation)
    } else {
        Ok(presentation.to_owned())
    }
}

#[derive(Deserialize)]
struct SerializedXbrlBoundaryOptions {
    mode: XbrlBoundaryMode,
    taxonomy: String,
    #[serde(default)]
    presentation: Option<String>,
    #[serde(default)]
    namespace_bindings: Vec<XbrlNamespaceBinding>,
    #[serde(default)]
    fact_bindings: Vec<XbrlFactBinding>,
}

impl<'de> Deserialize<'de> for XbrlBoundaryOptions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = SerializedXbrlBoundaryOptions::deserialize(deserializer)?;
        match serialized.mode {
            XbrlBoundaryMode::ExternalSource => {
                if serialized.presentation.is_some() {
                    return Err(serde::de::Error::custom(
                        XbrlBoundaryOptionsError::SourcePresentation,
                    ));
                }
                Self::external_source(serialized.taxonomy)
                    .and_then(|options| {
                        options.with_namespace_bindings(serialized.namespace_bindings)
                    })
                    .and_then(|options| options.with_fact_bindings(serialized.fact_bindings))
                    .map_err(serde::de::Error::custom)
            }
            XbrlBoundaryMode::ExternalTarget => {
                Self::external_target(serialized.taxonomy, serialized.presentation.as_deref())
                    .and_then(|options| {
                        options.with_namespace_bindings(serialized.namespace_bindings)
                    })
                    .and_then(|options| options.with_fact_bindings(serialized.fact_bindings))
                    .map_err(serde::de::Error::custom)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_validate_references_and_expose_mode() -> Result<(), Box<dyn Error>> {
        let source = XbrlBoundaryOptions::external_source("taxonomy/source.xsd")?;
        assert_eq!(source.mode(), XbrlBoundaryMode::ExternalSource);
        assert_eq!(source.taxonomy(), "taxonomy/source.xsd");
        assert_eq!(source.presentation(), None);
        assert_eq!(
            XbrlBoundaryOptions::external_source("  taxonomy/trimmed.xsd  ")?.taxonomy(),
            "taxonomy/trimmed.xsd"
        );

        let target = XbrlBoundaryOptions::external_target(
            "  taxonomy/target.xsd ",
            Some(" presentation.sps  "),
        )?;
        assert_eq!(target.mode(), XbrlBoundaryMode::ExternalTarget);
        assert_eq!(target.taxonomy(), "taxonomy/target.xsd");
        assert_eq!(target.presentation(), Some("presentation.sps"));

        assert_eq!(
            XbrlBoundaryOptions::external_source("  "),
            Err(XbrlBoundaryOptionsError::EmptyTaxonomy)
        );
        assert_eq!(
            XbrlBoundaryOptions::external_target("taxonomy.xsd", Some("")),
            Err(XbrlBoundaryOptionsError::EmptyPresentation)
        );
        Ok(())
    }

    #[test]
    fn serde_roundtrip_revalidates_private_fields() -> Result<(), Box<dyn Error>> {
        let options = XbrlBoundaryOptions::external_target("taxonomy.xsd", Some("layout.sps"))?
            .with_namespace_bindings(vec![XbrlNamespaceBinding::new(
                vec!["Table".to_string(), "Amount".to_string()],
                "urn:example",
            )?])?
            .with_fact_bindings(vec![XbrlFactBinding::new(
                vec!["Table".to_string(), "Amount".to_string()],
                XbrlFactType::Monetary,
            )?])?;
        let format_options = crate::FormatOptions {
            xbrl: Some(options.clone()),
            ..crate::FormatOptions::default()
        };
        let encoded = serde_json::to_string(&format_options)?;
        let decoded: crate::FormatOptions = serde_json::from_str(&encoded)?;
        assert_eq!(decoded.xbrl, Some(options));

        assert!(
            serde_json::from_str::<XbrlBoundaryOptions>(
                r#"{"mode":"external_source","taxonomy":"taxonomy.xsd","presentation":"layout.sps"}"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<XbrlBoundaryOptions>(
                r#"{"mode":"external_target","taxonomy":"taxonomy.xsd","namespace_bindings":[{"path":[],"namespace":"urn:test"}]}"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<XbrlBoundaryOptions>(
                r#"{"mode":"external_target","taxonomy":"taxonomy.xsd","fact_bindings":[{"path":["Amount"],"fact_type":"monetary"}]}"#,
            )
            .is_err()
        );
        assert!(
            XbrlBoundaryOptions::external_source("taxonomy.xsd")?
                .with_namespace_bindings(vec![XbrlNamespaceBinding::new(
                    vec!["Amount".to_string()],
                    "urn:test",
                )?])?
                .with_fact_bindings(vec![XbrlFactBinding::new(
                    vec!["Amount".to_string()],
                    XbrlFactType::Numeric,
                )?])
                .is_err()
        );
        assert!(
            serde_json::from_str::<XbrlBoundaryOptions>(
                r#"{"mode":"external_target","taxonomy":" ","presentation":null}"#,
            )
            .is_err()
        );
        Ok(())
    }
}
