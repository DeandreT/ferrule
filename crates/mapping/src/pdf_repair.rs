use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{FormatOptions, Project, RuntimeBoundary};

/// Native PDF behavior retained for editing but not implemented by the PDF reader.
/// The closed state carries no template text, paths, or document data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PdfRepairDependency {
    ObjectFind,
}

impl<'de> Deserialize<'de> for PdfRepairDependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            ObjectFind,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            kind: Kind,
        }
        match Repr::deserialize(deserializer)?.kind {
            Kind::ObjectFind => Ok(Self::ObjectFind),
        }
    }
}

impl fmt::Display for PdfRepairDependency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectFind => formatter.write_str(
                "unsupported native PDF ObjectFind detection; the stored extraction layout is a repair draft",
            ),
        }
    }
}

/// One PDF boundary that cannot faithfully decode physical PDF data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfRuntimeDependency {
    pub boundary: RuntimeBoundary,
    pub dependency: PdfRepairDependency,
}

impl fmt::Display for PdfRuntimeDependency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} requires {}", self.boundary, self.dependency)
    }
}

impl Project {
    /// Reports persisted PDF extraction blockers without reopening template files.
    /// Typed host-preparsed instances remain usable with the interpreter.
    pub fn pdf_runtime_dependencies(&self) -> Vec<PdfRuntimeDependency> {
        let mut dependencies = Vec::new();
        collect(
            &mut dependencies,
            RuntimeBoundary::PrimarySource,
            &self.source_options,
        );
        collect(
            &mut dependencies,
            RuntimeBoundary::PrimaryTarget,
            &self.target_options,
        );
        for source in &self.extra_sources {
            collect(
                &mut dependencies,
                RuntimeBoundary::NamedSource(source.name.clone()),
                &source.options,
            );
        }
        for target in &self.extra_targets {
            collect(
                &mut dependencies,
                RuntimeBoundary::NamedTarget(target.name.clone()),
                &target.options,
            );
        }
        dependencies
    }
}

fn collect(
    dependencies: &mut Vec<PdfRuntimeDependency>,
    boundary: RuntimeBoundary,
    options: &FormatOptions,
) {
    if let Some(dependency) = options
        .pdf
        .as_ref()
        .and_then(crate::PdfLayout::repair_dependency)
    {
        dependencies.push(PdfRuntimeDependency {
            boundary,
            dependency,
        });
    }
}
