use std::fmt;
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};
use serde::Serialize;

use crate::MfdError;

/// Compatibility policy applied before any export artifacts are published.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportProfile {
    /// Preserve the existing Ferrule round-trip representation.
    #[default]
    FerruleExtensions,
    /// Reject known extension dependencies, semantic differences, and omissions.
    NativeMfd,
}

/// Static assessment of the rendered design, not a vendor execution certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportCompatibility {
    /// No known semantic extension dependencies or export warnings were found.
    NativeMfd,
    /// Ferrule components, metadata, or captured-source semantics are required.
    FerruleExtensions,
    /// Export warnings indicate that some project features were omitted.
    Incomplete,
}

/// A known reason an export cannot claim native .mfd equivalence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportCompatibilityFeature {
    FerruleComponent,
    RecursiveComponent,
    EdiSchema,
    EdiLayout,
    EdiConfigDescriptor,
    EdiLexicalFormats,
    EdiImpliedDecimals,
    EdiValueConstraints,
    EdiAutocomplete,
    UnresolvedEdiConfiguration,
    UnresolvedJsonSchema,
    CapturedUserFunction,
    CapturedHttpPost,
    PdfLayout,
    XmlSerializationIndent,
    /// Newly emitted metadata is conservative until its native behavior is known.
    UnknownExtension,
}

/// One deterministic compatibility finding attached to its emitted component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportCompatibilityIssue {
    pub feature: ExportCompatibilityFeature,
    /// The closest owning component, including function definitions when relevant.
    pub component: String,
    /// Distinguishes components that have the same displayed name.
    pub component_uid: Option<u32>,
    pub message: String,
}

/// Compatibility findings are separate from legacy lossy-export warnings, so
/// existing callers' warning counts and default export behavior stay unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportReport {
    pub compatibility: ExportCompatibility,
    pub issues: Vec<ExportCompatibilityIssue>,
    pub warnings: Vec<String>,
}

impl ExportReport {
    /// Whether strict native mode accepts this report. This checks known export
    /// dependencies only, not external resources or vendor runtime behavior.
    pub fn is_native_compatible(&self) -> bool {
        self.issues.is_empty() && self.warnings.is_empty()
    }
}

impl fmt::Display for ExportReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut separator = "";
        for issue in &self.issues {
            write!(
                formatter,
                "{separator}{}: {}",
                issue.component, issue.message
            )?;
            separator = "; ";
        }
        for warning in &self.warnings {
            write!(formatter, "{separator}{warning}")?;
            separator = "; ";
        }
        if separator.is_empty() {
            formatter.write_str("no known native compatibility blockers")?;
        }
        Ok(())
    }
}

/// Inspect emitted components rather than only the input graph: lowering can
/// introduce extension components, and unreachable nodes may not be exported.
pub(super) fn profile(
    xml: &str,
    warnings: Vec<String>,
    mfd_path: &Path,
    siblings: &[(PathBuf, String)],
) -> Result<ExportReport, MfdError> {
    let document = Document::parse(xml)?;
    let mut issues = Vec::new();
    for node in document.descendants().filter(Node::is_element) {
        if node.has_tag_name("component")
            && node.attribute("library") == Some("pdf")
            && node.attribute("kind") == Some("34")
        {
            let template = node
                .descendants()
                .find(|child| child.has_tag_name("document"));
            let native = template
                .and_then(|template| {
                    Some((
                        template.attribute("schemafile")?,
                        template.attribute("root")?,
                    ))
                })
                .and_then(|(name, root)| {
                    let expected = mfd_path
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(name);
                    siblings
                        .iter()
                        .find(|(path, _)| path == &expected)
                        .map(|(_, contents)| (contents, root))
                })
                .is_some_and(|(contents, root)| {
                    crate::import::parse_native_pdf_template_text(contents, root).is_ok()
                });
            if !native {
                push_issue(
                    &mut issues,
                    node,
                    ExportCompatibilityFeature::PdfLayout,
                    "the PDF extraction template requires Ferrule's layout format or has no validated native template",
                );
            }
        }
        if node.has_tag_name("component") && node.attribute("library") == Some("ferrule") {
            let recursive = node
                .descendants()
                .any(|child| child.has_tag_name("ferrule-recursive"));
            push_issue(
                &mut issues,
                node,
                if recursive {
                    ExportCompatibilityFeature::RecursiveComponent
                } else {
                    ExportCompatibilityFeature::FerruleComponent
                },
                "uses a function in Ferrule's component library; the reference application has no native implementation",
            );
        }
        if node.has_tag_name("text") && node.attribute("type") == Some("edi") {
            push_issue(
                &mut issues,
                node,
                ExportCompatibilityFeature::EdiSchema,
                "EDI entry shape and cardinality rely on Ferrule metadata; native boundary compatibility has not been established",
            );
        }
        if node.has_tag_name("wsdl") && node.attribute("httpmethod") == Some("POST") {
            push_issue(
                &mut issues,
                node,
                ExportCompatibilityFeature::CapturedHttpPost,
                "Ferrule consumes a captured response, but this exported component describes a live HTTP POST call",
            );
        }
        if let Some((feature, message)) = element_dependency(node.tag_name().name()) {
            push_issue(&mut issues, node, feature, message);
        }
        for attribute in node.attributes() {
            match attribute.name() {
                "ferrule-unresolved-config" | "ferrule-missing-config" => push_issue(
                    &mut issues,
                    node,
                    ExportCompatibilityFeature::UnresolvedEdiConfiguration,
                    "the native EDI configuration is missing or unresolved",
                ),
                "ferrule-unresolved-json-schema" => push_issue(
                    &mut issues,
                    node,
                    ExportCompatibilityFeature::UnresolvedJsonSchema,
                    "the original JSON Schema was unavailable; the generated schema only reflects the imported entry tree",
                ),
                // Native XML string serializers use indentation by default.
                // Export emits this extension only for an explicit false override.
                "ferrule-indent" => push_issue(
                    &mut issues,
                    node,
                    ExportCompatibilityFeature::XmlSerializationIndent,
                    "XML string serialization indentation is retained only in Ferrule metadata",
                ),
                "ferrulemessagetype" | "ferruletransactionset" => push_issue(
                    &mut issues,
                    node,
                    ExportCompatibilityFeature::EdiAutocomplete,
                    "EDI message or transaction selection for autocomplete depends on Ferrule metadata",
                ),
                // These annotations accompany a native schema/component/edge
                // representation. Their presence alone is not an execution
                // dependency; EDI's entry-only schema is reported above.
                "ferrule-primary-source"
                | "ferrule-database-wrapper"
                | "ferrule-kind"
                | "ferrule-repeating"
                | "ferrule-text"
                | "ferrule-nillable"
                | "ferrule-fixed"
                | "ferrule-default"
                | "ferrule-text-index"
                | "ferrulefield" => {}
                name if name.starts_with("ferrule") => push_issue(
                    &mut issues,
                    node,
                    ExportCompatibilityFeature::UnknownExtension,
                    &format!(
                        "unclassified Ferrule attribute `{name}` requires compatibility review"
                    ),
                ),
                _ => {}
            }
        }
    }
    let compatibility = if !warnings.is_empty() {
        ExportCompatibility::Incomplete
    } else if issues.is_empty() {
        ExportCompatibility::NativeMfd
    } else {
        ExportCompatibility::FerruleExtensions
    };
    Ok(ExportReport {
        compatibility,
        issues,
        warnings,
    })
}

fn element_dependency(name: &str) -> Option<(ExportCompatibilityFeature, &str)> {
    use ExportCompatibilityFeature::*;
    Some(match name {
        "ferrule-layout" => (
            EdiLayout,
            "the embedded IDoc or SWIFT layout uses Ferrule's layout format",
        ),
        "ferrule-idoc-native-config" => (
            EdiConfigDescriptor,
            "the IDoc descriptor remains in Ferrule metadata alongside a generated native configuration; native settings and runtime behavior are not yet proven equivalent",
        ),
        "ferrule-lexical-formats" => (
            EdiLexicalFormats,
            "EDI lexical formatting depends on Ferrule metadata",
        ),
        "ferrule-implied-decimals" => (
            EdiImpliedDecimals,
            "EDI decimal scaling depends on Ferrule metadata",
        ),
        "ferrule-value-constraints" => (
            EdiValueConstraints,
            "EDI length and code-list validation depends on Ferrule metadata",
        ),
        "ferrule-external-source" => (
            CapturedUserFunction,
            "the original user function is represented by captured JSON input, not an executable native function",
        ),
        // The function dependency is already reported at its owning component.
        // Scope sources also have native connection edges and are retained only
        // to make Ferrule's source-frame recovery deterministic.
        "ferrule-recursive" | "ferrule-scope-sources" => return None,
        name if name.starts_with("ferrule-") => (
            UnknownExtension,
            "unclassified Ferrule element requires compatibility review",
        ),
        _ => return None,
    })
}

fn push_issue(
    issues: &mut Vec<ExportCompatibilityIssue>,
    node: Node<'_, '_>,
    feature: ExportCompatibilityFeature,
    message: &str,
) {
    let component = node
        .ancestors()
        .find(|ancestor| ancestor.has_tag_name("component"));
    let issue = ExportCompatibilityIssue {
        feature,
        component: component
            .and_then(|component| component.attribute("name"))
            .unwrap_or("mapping")
            .to_string(),
        component_uid: component
            .and_then(|component| component.attribute("uid"))
            .and_then(|uid| uid.parse().ok()),
        message: message.to_string(),
    };
    if !issues.contains(&issue) {
        issues.push(issue);
    }
}
