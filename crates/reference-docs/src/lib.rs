//! Shared technical references for the Markdown guides and published site.
//!
//! Canonical data lives in `docs/reference/*.toml`. Each [`Surface`] renders
//! one marked region of a hand-written document from that data, so curated
//! prose outside the region stays untouched.

mod html;
mod markdown;
mod model;
mod region;
mod text;

use std::path::{Path, PathBuf};

use thiserror::Error;

use model::{FORMATS_SOURCE, Reference, WORKSPACE_SOURCE};

/// The command that rewrites every stale generated region.
pub const REGENERATE: &str = "cargo run -p reference-docs";

/// The three claim categories every format reference distinguishes.
const CATEGORIES: [(&str, &str); 3] = [
    (
        "Implemented capability",
        "behavior of the native format adapters and the CLI that uses them, backed by the source and tests cited in the canonical reference.",
    ),
    (
        "Qualified route",
        "a separately named route, such as a generated library adapter or an editor workflow, with its own contract or qualification record. It covers only that route and profile.",
    ),
    (
        "Unsupported profile",
        "a named shape, direction, or behavior outside the implemented scope. The runtime refuses it or, where stated, deliberately does not apply it. The list is not exhaustive.",
    ),
];

const COVERAGE_NOTE: &str = "These references describe finite, tested coverage. They are not a parity claim for a complete format standard or another product.";

#[derive(Debug, Error)]
pub enum Error {
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Toml {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("{file}: {message}")]
    Invalid { file: String, message: String },
    #[error("{path}: {message}")]
    Region { path: String, message: String },
}

#[derive(Clone, Copy)]
enum Renderer {
    MarkdownFormats,
    HtmlFormats,
    MarkdownWorkspace,
    HtmlWorkspace,
}

/// One generated region in a committed document.
struct Surface {
    path: &'static str,
    region: &'static str,
    source: &'static str,
    renderer: Renderer,
}

const SURFACES: [Surface; 4] = [
    Surface {
        path: "docs/formats.md",
        region: "formats",
        source: FORMATS_SOURCE,
        renderer: Renderer::MarkdownFormats,
    },
    Surface {
        path: "site/docs/formats.html",
        region: "formats",
        source: FORMATS_SOURCE,
        renderer: Renderer::HtmlFormats,
    },
    Surface {
        path: "docs/architecture.md",
        region: "workspace",
        source: WORKSPACE_SOURCE,
        renderer: Renderer::MarkdownWorkspace,
    },
    Surface {
        path: "site/docs/architecture.html",
        region: "workspace",
        source: WORKSPACE_SOURCE,
        renderer: Renderer::HtmlWorkspace,
    },
];

/// The expected contents of one generated document.
pub struct Generated {
    /// Repository-relative path.
    pub path: &'static str,
    pub contents: String,
    /// Whether the committed file already matches `contents`.
    pub current: bool,
}

/// Renders every surface without writing anything.
pub fn generate(root: &Path) -> Result<Vec<Generated>, Error> {
    let reference = Reference::load(root)?;
    SURFACES
        .iter()
        .map(|surface| {
            let path = root.join(surface.path);
            let committed =
                std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
            let body = match surface.renderer {
                Renderer::MarkdownFormats => markdown::formats(&reference.formats, surface.path),
                Renderer::HtmlFormats => html::formats(&reference.formats, &reference.repository),
                Renderer::MarkdownWorkspace => markdown::workspace(&reference.areas),
                Renderer::HtmlWorkspace => html::workspace(&reference.areas),
            };
            let contents = region::splice(&committed, surface.region, surface.source, &body)
                .map_err(|message| Error::Region {
                    path: surface.path.to_owned(),
                    message,
                })?;
            Ok(Generated {
                path: surface.path,
                current: contents == committed,
                contents,
            })
        })
        .collect()
}

/// Repository-relative paths whose generated regions are stale.
pub fn stale(root: &Path) -> Result<Vec<&'static str>, Error> {
    Ok(generate(root)?
        .into_iter()
        .filter(|generated| !generated.current)
        .map(|generated| generated.path)
        .collect())
}

/// Rewrites stale surfaces and returns their repository-relative paths.
pub fn write(root: &Path) -> Result<Vec<&'static str>, Error> {
    let mut written = Vec::new();
    for generated in generate(root)? {
        if generated.current {
            continue;
        }
        let path = root.join(generated.path);
        std::fs::write(&path, generated.contents).map_err(|source| Error::Io { path, source })?;
        written.push(generated.path);
    }
    Ok(written)
}
