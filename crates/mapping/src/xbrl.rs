use std::error::Error;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// Which side of a mapping is supplied by an opaque XBRL boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XbrlBoundaryMode {
    ExternalSource,
    ExternalTarget,
}

/// Validated metadata retained for XBRL components that ferrule can inspect
/// but cannot execute yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct XbrlBoundaryOptions {
    mode: XbrlBoundaryMode,
    taxonomy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    presentation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XbrlBoundaryOptionsError {
    EmptyTaxonomy,
    EmptyPresentation,
    SourcePresentation,
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
                Self::external_source(serialized.taxonomy).map_err(serde::de::Error::custom)
            }
            XbrlBoundaryMode::ExternalTarget => {
                Self::external_target(serialized.taxonomy, serialized.presentation.as_deref())
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
        let options = XbrlBoundaryOptions::external_target("taxonomy.xsd", Some("layout.sps"))?;
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
                r#"{"mode":"external_target","taxonomy":" ","presentation":null}"#,
            )
            .is_err()
        );
        Ok(())
    }
}
