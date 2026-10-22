//! Filesystem acquisition of declared SQLite query inputs (`sqlite_read_v1`).
//!
//! Only the filesystem run and preview routes own this acquisition. They
//! validate every query of the planned primary and static named inputs and
//! preflight all bindings before any input is opened; each database is then
//! opened read-only at its turn in the existing acquisition order and closed
//! before mapping evaluation. Every other route refuses the profile before it
//! reads anything.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::Context;
use ir::{Instance, SchemaNode, Value};
use mapping::{SqliteReadOwner, SqliteReadProfile, ValidatedSqliteRead};

use crate::http_url;
use crate::route_capability::{HostRoute, InputRole, RouteRefusal};

/// One admitted query input with its host-resolved bindings.
pub(crate) struct PreparedQuery {
    plan: ValidatedSqliteRead,
    bindings: Vec<(String, Value)>,
}

impl PreparedQuery {
    /// Validates the descriptor, extracts this query's declared runtime
    /// parameters through the shared typed resolution, and preflights the
    /// bindings against `path` without touching the filesystem.
    pub(crate) fn prepare(
        role: &InputRole,
        path: &Path,
        schema: &SchemaNode,
        profile: &SqliteReadProfile,
        parameters: Option<&engine::RuntimeParameters>,
    ) -> anyhow::Result<Self> {
        let context = || match role {
            InputRole::Named(name) => format!("preparing SQLite query input `{name}`"),
            InputRole::Primary | InputRole::PipelineHost(_) => {
                "preparing primary SQLite query input".to_owned()
            }
        };
        if http_url(path).is_some() {
            anyhow::bail!(
                "{}: a declared SQLite query requires a local database file",
                context()
            );
        }
        let plan = mapping::validate_sqlite_read_profile(schema, profile).with_context(context)?;
        let bindings = plan
            .parameters()
            .iter()
            .filter_map(|parameter| {
                let resolved = parameters?.resolve(&parameter.runtime_parameter, parameter.ty)?;
                // An uncoercible value keeps its tag so the query reports it.
                let value = resolved.unwrap_or_else(Value::clone);
                Some((parameter.name.clone(), value))
            })
            .collect::<Vec<_>>();
        format_db::query::bind_query(path, schema, &plan, &bindings).with_context(context)?;
        Ok(Self { plan, bindings })
    }

    /// Opens, reads and closes the database before returning its instance.
    pub(crate) fn acquire(
        &self,
        path: &Path,
        schema: &SchemaNode,
        policy: &engine::ExecutionPolicy,
        cancellation: Option<&AtomicBool>,
    ) -> anyhow::Result<Instance> {
        policy.authorize_read(path)?;
        let mut control = format_db::query::QueryReadControl::new();
        if let Some(cancellation) = cancellation {
            control = control.with_cancellation(cancellation);
        }
        format_db::query::read_query(path, schema, &self.plan, &self.bindings, &control)
            .with_context(|| format!("reading SQLite query input {}", path.display()))
    }
}

/// Refuses a route that cannot own declared query acquisition, naming the
/// first query input in primary-then-declaration order.
pub(crate) fn reject_query_inputs(
    project: &mapping::Project,
    route: HostRoute,
) -> Result<(), RouteRefusal> {
    match project.sqlite_read_inputs().first() {
        Some(input) => Err(RouteRefusal::UnsupportedInputProfile {
            route,
            input: match input.owner {
                SqliteReadOwner::Primary => InputRole::Primary,
                SqliteReadOwner::Named(name) => InputRole::Named(name.to_owned()),
            },
        }),
        None => Ok(()),
    }
}
