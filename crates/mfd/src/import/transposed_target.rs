//! Record iteration for transposed XLSX targets.
//!
//! A transposed target has no record port. Each worksheet row's Cell is its
//! own sequence, and the reference application creates one cell per item of
//! that Cell's repeated feed context, at the item's `n` when that input is
//! connected and otherwise at the next column from A. Ferrule writes one
//! record per worksheet column, so the driver row (the first data row) decides
//! the record iteration. The import is exact only when every other row and
//! every `n` input follows that same context; anything else is reported.

use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, TRANSPOSED_ROW_INDEX_PORT};
use super::scope::{IterationNodes, ScopeBuilder};

pub(super) fn is_target(target: &SchemaComponent) -> bool {
    !target.is_source
        && target.format == ComponentFormat::Xlsx
        && !target.options.xlsx_rows.is_empty()
}

pub(super) fn is_row_index_port(path: &[String]) -> bool {
    path.first().map(String::as_str) == Some(TRANSPOSED_ROW_INDEX_PORT)
}

/// Installs the root record iteration from the driver row's feed. A driver
/// without a repeated context writes one record, like the reference
/// application's single cell.
pub(super) fn install(
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    if !is_target(target) {
        return;
    }
    let SchemaKind::Group { children, .. } = &target.schema.kind else {
        return;
    };
    let data_fields = children
        .iter()
        .filter(|field| field.name != "n")
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    let Some(driver) = data_fields.first().copied() else {
        return;
    };
    let edge_from = builder.edge_from;
    let feed_at = |path: &[&str]| {
        target
            .ports
            .iter()
            .filter(|(_, port)| port.iter().map(String::as_str).eq(path.iter().copied()))
            .find_map(|(key, _)| edge_from.get(key).copied())
    };
    let Some(driver_feed) = feed_at(&[driver]) else {
        if target.ports.keys().any(|key| edge_from.contains_key(key)) {
            builder.warnings.push(format!(
                "transposed XLSX target `{}` has no connection to its driver row `{driver}`; ferrule writes one record per driver item, so no records were mapped",
                target.name
            ));
        }
        return;
    };
    let context = builder.computed_iteration_source(driver_feed);

    for field in &data_fields[1..] {
        if let Some(feed) = feed_at(&[field])
            && builder.computed_iteration_source(feed) != context
        {
            builder.warnings.push(format!(
                "transposed XLSX target `{}` row `{field}` is fed from another repeated context than driver row `{driver}`; ferrule writes it once per driver item",
                target.name
            ));
        }
    }
    if has_position(&target.schema) {
        check_positions(target, &data_fields, driver_feed, builder);
    }

    if let Some(context) = context {
        builder.note_framed_prefixes(&context);
        let context = builder.context_path(&context);
        scopes.add_iteration(
            &[],
            &context,
            IterationNodes::default(),
            mapping::IterationOutput::Repeated,
        );
    }
}

fn has_position(schema: &SchemaNode) -> bool {
    schema.child("n").is_some()
}

/// Every data row must take `n` from one feed in the driver's context; a row
/// without `n` would otherwise start again from column A.
fn check_positions(
    target: &SchemaComponent,
    data_fields: &[&str],
    driver_feed: u32,
    builder: &mut GraphBuilder<'_>,
) {
    let edge_from = builder.edge_from;
    let mut feeds = BTreeMap::new();
    for (key, path) in &target.ports {
        let position = match path.as_slice() {
            [field] if field == "n" => Some(1),
            [_, position] if is_row_index_port(path) => position.parse::<usize>().ok(),
            _ => None,
        };
        if let Some(position) = position {
            feeds.insert(position, edge_from.get(key).copied());
        }
    }
    let driver_index = feeds.get(&1).copied().flatten();
    let consistent = driver_index.is_some()
        && (1..=data_fields.len()).all(|position| feeds.get(&position) == Some(&driver_index));
    if !consistent {
        builder.warnings.push(format!(
            "transposed XLSX target `{}` does not connect one `n` value to every row; ferrule places every row of a record at the driver row's `n`",
            target.name
        ));
        return;
    }
    if let Some(index_feed) = driver_index
        && builder.computed_iteration_source(index_feed)
            != builder.computed_iteration_source(driver_feed)
    {
        builder.warnings.push(format!(
            "transposed XLSX target `{}` takes `n` from another repeated context than its driver row; ferrule reads it once per driver item",
            target.name
        ));
    }
}
