//! Route capability and preparation queries shared by the host routes.
//!
//! Every route prepares an immutable request before it acquires anything. It
//! resolves the locations it will read and the artifacts it will publish into a
//! [`RoutePlan`], admits the requested observers, applies the execution policy
//! to every location known before evaluation, and refuses an unsupported
//! combination with a typed [`RouteRefusal`] or the route's existing typed
//! error. Preparation reads only the design and the filesystem metadata needed
//! to resolve paths: it never opens an input, reads a standard stream, or
//! creates an output, a staging file, or a directory.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use mapping::FormatOptions;

use crate::execution_policy::read_location;
use crate::file_run::{PreparedFileRun, RunRequest};
use crate::stdio::PreparedStreamRun;

/// The host route a request uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRoute {
    /// `run` with filesystem or HTTP inputs and filesystem outputs.
    Run,
    /// `run PROJECT - -`: one payload-compatible artifact on stdout.
    StandardStreams,
    RunPipeline,
    /// Host-owned bytes in and serialized artifacts out, as used by preview.
    Payload,
    Generate,
}

/// The role of one acquired input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputRole {
    Primary,
    Named(String),
    PipelineHost(String),
}

/// One input a route acquires, in acquisition order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedAcquisition {
    /// The project or pipeline file; it is read before preparation.
    Design(PathBuf),
    /// A local file, an HTTP(S) URL, or a local XML file-set pattern. The
    /// policy checks `location`, which is the base directory for a file set.
    Location {
        role: InputRole,
        path: PathBuf,
        location: PathBuf,
    },
    /// The primary document read from stdin; `identity` selects its format.
    Stdin { identity: PathBuf },
    /// Host-owned bytes with a logical identity; nothing is acquired.
    Payload { role: InputRole, identity: PathBuf },
    /// A per-item named source resolved during evaluation. Each location is
    /// checked immediately before it is loaded.
    Dynamic { name: String },
}

/// One artifact a successful route publishes, in publication order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedPublication {
    /// One file atomically replaced after every target succeeds.
    File { target: String, path: PathBuf },
    /// Per-document files confined beneath `base`, published with the batch.
    Documents { target: String, base: PathBuf },
    /// Raw artifact bytes on stdout; `identity` selects the output format.
    Stdout { target: String, identity: PathBuf },
    /// A host-owned artifact, such as a JSON trace, published after the
    /// mapping outputs. Outputs and consumed inputs may not overlap it.
    HostArtifact(PathBuf),
    /// Serialized artifacts returned to the host in memory.
    Payload { target: String },
    /// A generated library tree renamed into place only when complete.
    GeneratedTree(PathBuf),
}

/// What a prepared route will acquire and publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    route: HostRoute,
    acquisitions: Vec<PlannedAcquisition>,
    publications: Vec<PlannedPublication>,
}

impl RoutePlan {
    pub(crate) fn new(route: HostRoute, design: &Path) -> Self {
        Self {
            route,
            acquisitions: vec![PlannedAcquisition::Design(design.to_path_buf())],
            publications: Vec::new(),
        }
    }

    pub fn route(&self) -> HostRoute {
        self.route
    }

    pub fn acquisitions(&self) -> &[PlannedAcquisition] {
        &self.acquisitions
    }

    pub fn publications(&self) -> &[PlannedPublication] {
        &self.publications
    }

    pub(crate) fn acquire(&mut self, acquisition: PlannedAcquisition) {
        self.acquisitions.push(acquisition);
    }

    pub(crate) fn acquire_location(
        &mut self,
        role: InputRole,
        path: &Path,
        options: &FormatOptions,
    ) {
        self.acquire(PlannedAcquisition::Location {
            role,
            path: path.to_path_buf(),
            location: read_location(path, options).to_path_buf(),
        });
    }

    pub(crate) fn acquire_dynamic(&mut self, name: &str) {
        self.acquire(PlannedAcquisition::Dynamic {
            name: name.to_owned(),
        });
    }

    pub(crate) fn publish(&mut self, publication: PlannedPublication) {
        self.publications.push(publication);
    }

    pub(crate) fn publish_host_artifacts(&mut self, paths: &[&Path]) {
        self.publications.extend(
            paths
                .iter()
                .map(|path| PlannedPublication::HostArtifact(path.to_path_buf())),
        );
    }

    /// Applies the policy to every planned location before the first one is
    /// acquired. Acquisition still checks each location again when it opens it.
    /// `context` supplies the route's existing error context for each role.
    pub(crate) fn authorize_reads(
        &self,
        policy: &engine::ExecutionPolicy,
        context: impl Fn(&InputRole) -> Option<String>,
    ) -> anyhow::Result<()> {
        for acquisition in &self.acquisitions {
            if let PlannedAcquisition::Location { role, location, .. } = acquisition {
                let authorized = policy.authorize_read(location);
                match context(role) {
                    Some(context) => authorized.context(context)?,
                    None => authorized?,
                }
            }
        }
        Ok(())
    }
}

/// An unsupported route combination, refused before anything is acquired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteRefusal {
    PdfWorkerRequiresFilesystem,
    TraceToStdout,
    StdinRequiresStdout,
    StandardStreamsRequireStdout,
    StandardStreamsDynamicSources {
        names: Vec<String>,
    },
    /// Standard streams accept a trace sink but no debug hook or cancellation.
    StandardStreamsDebugging,
    GeneratedOutputExists {
        path: PathBuf,
    },
    /// Execution supplied an observer the prepared request did not admit.
    UndeclaredObservers,
}

impl fmt::Display for RouteRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PdfWorkerRequiresFilesystem => formatter.write_str(
                "--pdf-worker requires filesystem input and output; standard-stream payloads remain in-process",
            ),
            Self::TraceToStdout => formatter.write_str(
                "`--trace-json -` is not supported because `run` reserves stdout for its result report",
            ),
            Self::StdinRequiresStdout => formatter.write_str("`--input -` requires `--output -`"),
            Self::StandardStreamsRequireStdout => formatter.write_str(
                "standard-stream execution requires `--output -`; filesystem outputs use the ordinary `run` path",
            ),
            Self::StandardStreamsDynamicSources { names } => write!(
                formatter,
                "standard-stream execution cannot supply per-item dynamic source(s): {}; use the filesystem runner or PayloadRunOptions",
                names.join(", ")
            ),
            Self::StandardStreamsDebugging => formatter.write_str(
                "standard-stream execution supports tracing but not debugging or cancellation",
            ),
            Self::GeneratedOutputExists { path } => write!(
                formatter,
                "generated output directory {} already exists",
                path.display()
            ),
            Self::UndeclaredObservers => formatter
                .write_str("run observers must be declared when the request is prepared"),
        }
    }
}

impl std::error::Error for RouteRefusal {}

/// The stream and artifact arguments of one `run` invocation.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunRouteArgs<'a> {
    pub input_path: Option<&'a Path>,
    pub output_path: Option<&'a Path>,
    pub pdf_worker: bool,
    pub trace_path: Option<&'a Path>,
}

/// Selects the `run` route from its arguments alone. A `-` output selects
/// standard streams; a `-` input is valid only with it.
pub fn select_run_route(args: &RunRouteArgs<'_>) -> Result<HostRoute, RouteRefusal> {
    let stdin = args.input_path == Some(Path::new("-"));
    let stdout = args.output_path == Some(Path::new("-"));
    if args.pdf_worker && (stdin || stdout) {
        return Err(RouteRefusal::PdfWorkerRequiresFilesystem);
    }
    if args.trace_path == Some(Path::new("-")) {
        return Err(RouteRefusal::TraceToStdout);
    }
    match (stdin, stdout) {
        (_, true) => Ok(HostRoute::StandardStreams),
        (true, false) => Err(RouteRefusal::StdinRequiresStdout),
        (false, false) => Ok(HostRoute::Run),
    }
}

/// Prepares a `run` request for a loaded project without acquiring anything
/// and reports its plan. `-` paths select standard streams as on the command
/// line; the request's protected paths are the host artifacts it will publish.
pub fn plan_run(
    project: &mapping::Project,
    project_path: &Path,
    request: RunRequest<'_>,
) -> anyhow::Result<RoutePlan> {
    let route = select_run_route(&RunRouteArgs {
        input_path: request.input_path,
        output_path: request.output_path,
        ..RunRouteArgs::default()
    })?;
    Ok(match route {
        HostRoute::StandardStreams => PreparedStreamRun::prepare(project, project_path, request)?
            .plan()
            .clone(),
        _ => PreparedFileRun::prepare(project, project_path, request)?
            .plan()
            .clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(input: Option<&str>, output: Option<&str>) -> Result<HostRoute, RouteRefusal> {
        select_run_route(&RunRouteArgs {
            input_path: input.map(Path::new),
            output_path: output.map(Path::new),
            ..RunRouteArgs::default()
        })
    }

    #[test]
    fn stream_arguments_select_or_refuse_a_route() {
        assert_eq!(route(None, None), Ok(HostRoute::Run));
        assert_eq!(route(Some("in.csv"), Some("out.csv")), Ok(HostRoute::Run));
        assert_eq!(route(Some("-"), Some("-")), Ok(HostRoute::StandardStreams));
        assert_eq!(route(None, Some("-")), Ok(HostRoute::StandardStreams));
        assert_eq!(
            route(Some("-"), Some("out.csv")),
            Err(RouteRefusal::StdinRequiresStdout)
        );
        assert_eq!(
            route(Some("-"), None),
            Err(RouteRefusal::StdinRequiresStdout)
        );
    }

    #[test]
    fn host_artifact_and_worker_arguments_refuse_streams() {
        let refusal = |pdf_worker, trace: Option<&str>, output: Option<&str>| {
            select_run_route(&RunRouteArgs {
                input_path: None,
                output_path: output.map(Path::new),
                pdf_worker,
                trace_path: trace.map(Path::new),
            })
        };
        assert_eq!(
            refusal(true, None, Some("-")),
            Err(RouteRefusal::PdfWorkerRequiresFilesystem)
        );
        assert_eq!(refusal(true, None, None), Ok(HostRoute::Run));
        assert_eq!(
            refusal(false, Some("-"), None),
            Err(RouteRefusal::TraceToStdout)
        );
        assert_eq!(
            refusal(false, Some("trace.jsonl"), Some("-")),
            Ok(HostRoute::StandardStreams)
        );
    }
}
