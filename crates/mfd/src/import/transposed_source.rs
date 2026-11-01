//! Row alignment for transposed XLSX sources.
//!
//! The reference application reads every selected row as its own sequence of
//! non-empty cells, so ferrule's export reads each later row through
//! `filter(row Cell, equal(row Cell/@n, driver Cell/@n))`. Ferrule's
//! transposed records already line rows up by column, so that filter is the
//! plain row field only for direct scalar target reads in a proven driver
//! context. Sequence consumers must not lose the filter's cardinality.

use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaKind;

use super::function::{FnComponent, is_filter};
use super::schema::{ComponentFormat, SchemaComponent, TRANSPOSED_ROW_INDEX_PORT};

mod raw_dependencies;
use raw_dependencies::RawDependencies;

/// Absorbs only proven per-record scalar reads. Unsupported consumers and
/// their downstream function outputs are disconnected with a warning, making
/// executable import refuse the partial design rather than silently unfilter it.
pub(super) fn absorb_alignment_filters(
    structure: roxmltree::Node<'_, '_>,
    schema_components: &[SchemaComponent],
    fn_components: &mut Vec<FnComponent>,
    feeds: &mut BTreeMap<u32, u32>,
    warnings: &mut Vec<String>,
) {
    if !schema_components.iter().any(|component| {
        component.is_source
            && component.format == ComponentFormat::Xlsx
            && !component.options.xlsx_rows.is_empty()
    }) {
        return;
    }
    let raw_dependencies = RawDependencies::read(structure);
    let original_feeds = feeds.clone();
    let mut absorbed = BTreeSet::new();
    for source in schema_components.iter().filter(|component| {
        component.is_source
            && component.format == ComponentFormat::Xlsx
            && !component.options.xlsx_rows.is_empty()
    }) {
        let rows = RowPorts::read(source);
        for (index, filter) in fn_components.iter().enumerate() {
            if !is_filter(filter) || absorbed.contains(&index) {
                continue;
            }
            if !rows.alignment_candidate(filter, schema_components, fn_components, &original_feeds)
            {
                continue;
            }
            if let Some(alignment) = rows.alignment(filter, fn_components, &original_feeds) {
                let consumers = feeds
                    .iter()
                    .filter_map(|(&input, &output)| (output == alignment.aligned).then_some(input))
                    .collect::<Vec<_>>();
                let mut unsupported = false;
                for input in consumers {
                    if rows.scalar_target_read(input, schema_components, &original_feeds) {
                        feeds.insert(input, alignment.cell);
                    } else {
                        unsupported = true;
                    }
                }
                if unsupported {
                    disconnect_output(
                        alignment.aligned,
                        schema_components,
                        fn_components,
                        &raw_dependencies,
                        feeds,
                        &mut absorbed,
                    );
                    warnings.push(format!(
                        "xlsx component `{}` has a row-alignment filter with an unproved scalar context or sequence consumer; affected consumers were skipped",
                        source.name
                    ));
                }
                absorbed.extend([index, alignment.equal]);
                alignment.absorb(feeds);
            } else {
                for &output in &filter.outputs {
                    disconnect_output(
                        output,
                        schema_components,
                        fn_components,
                        &raw_dependencies,
                        feeds,
                        &mut absorbed,
                    );
                }
                absorbed.insert(index);
                warnings.push(format!(
                    "xlsx component `{}` has a row-alignment filter without unique matching-row and first-row driver indexes; affected consumers were skipped",
                    source.name
                ));
            }
        }
        let stray = feeds
            .iter()
            .filter(|(_, output)| rows.is_private_index(**output))
            .map(|(_, &output)| output)
            .collect::<BTreeSet<_>>();
        if !stray.is_empty() {
            for output in stray {
                disconnect_output(
                    output,
                    schema_components,
                    fn_components,
                    &raw_dependencies,
                    feeds,
                    &mut absorbed,
                );
            }
            warnings.push(format!(
                "xlsx component `{}` reads a row-alignment `n` port outside an alignment filter; those connections were skipped",
                source.name
            ));
        }
    }
    let mut position = 0;
    fn_components.retain(|_| {
        let keep = !absorbed.contains(&position);
        position += 1;
        keep
    });
}

/// Removing only an aggregate's input would let its default/empty behavior
/// remain connected. Cut every physical component's dependent outputs as well,
/// regardless of which specialized importer registry owns their recipes.
fn disconnect_output(
    output: u32,
    schemas: &[SchemaComponent],
    functions: &[FnComponent],
    raw_dependencies: &RawDependencies,
    feeds: &mut BTreeMap<u32, u32>,
    removed: &mut BTreeSet<usize>,
) {
    let mut pending = vec![output];
    let mut visited = BTreeSet::new();
    let mut expanded_components = BTreeSet::new();
    while let Some(output) = pending.pop() {
        if !visited.insert(output) {
            continue;
        }
        let inputs = feeds
            .iter()
            .filter_map(|(&input, &feed)| (feed == output).then_some(input))
            .collect::<BTreeSet<_>>();
        feeds.retain(|input, _| !inputs.contains(input));
        for &input in &inputs {
            raw_dependencies.extend_outputs(input, &mut expanded_components, &mut pending);
        }
        for (index, function) in functions.iter().enumerate() {
            if function
                .inputs
                .iter()
                .flatten()
                .any(|input| inputs.contains(input))
            {
                removed.insert(index);
                pending.extend(&function.outputs);
            }
        }
        for schema in schemas
            .iter()
            .filter(|schema| schema.is_variable || schema.is_pass_through)
        {
            if schema.input_keys.iter().any(|input| inputs.contains(input)) {
                pending.extend(&schema.output_keys);
            }
        }
    }
}

/// One-based row positions of a transposed source's Cell and `n` outputs.
struct RowPorts {
    cells: BTreeMap<u32, usize>,
    /// The unambiguous first row's unmarked `n` field.
    record_index: BTreeSet<u32>,
    alignment_index: BTreeMap<u32, usize>,
    ambiguous_index: BTreeSet<u32>,
}

impl RowPorts {
    fn read(source: &SchemaComponent) -> Self {
        let SchemaKind::Group { children, .. } = &source.schema.kind else {
            return Self {
                cells: BTreeMap::new(),
                record_index: BTreeSet::new(),
                alignment_index: BTreeMap::new(),
                ambiguous_index: BTreeSet::new(),
            };
        };
        let row_names = children
            .iter()
            .take(source.options.xlsx_rows.len())
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>();
        let mut rows = Self {
            cells: BTreeMap::new(),
            record_index: BTreeSet::new(),
            alignment_index: BTreeMap::new(),
            ambiguous_index: BTreeSet::new(),
        };
        for (&key, path) in &source.ports {
            match path.as_slice() {
                [field] if field == "n" && !row_names.contains(&field.as_str()) => {
                    rows.record_index.insert(key);
                }
                [field] => {
                    if let Some(position) = row_names.iter().position(|name| name == field) {
                        rows.cells.insert(key, position + 1);
                    }
                }
                [marker, position] if marker == TRANSPOSED_ROW_INDEX_PORT => {
                    if let Ok(position) = position.parse() {
                        rows.alignment_index.insert(key, position);
                    } else {
                        rows.ambiguous_index.insert(key);
                    }
                }
                [marker, ..] if marker == TRANSPOSED_ROW_INDEX_PORT => {
                    rows.ambiguous_index.insert(key);
                }
                _ => {}
            }
        }
        rows
    }

    /// The driver row's column for the current record: its alignment port or
    /// the record's `n` field.
    fn is_driver_index(&self, port: u32) -> bool {
        self.record_index.contains(&port) || self.alignment_index.get(&port) == Some(&1)
    }

    fn is_private_index(&self, port: u32) -> bool {
        self.alignment_index.contains_key(&port) || self.ambiguous_index.contains(&port)
    }

    /// Leave ordinary value predicates to generic filter import. An index
    /// dependency (including an undeclared saved port) identifies an attempted
    /// alignment even when its exact proof below fails.
    fn alignment_candidate(
        &self,
        filter: &FnComponent,
        schemas: &[SchemaComponent],
        functions: &[FnComponent],
        feeds: &BTreeMap<u32, u32>,
    ) -> bool {
        let Some(cell) = filter
            .inputs
            .first()
            .copied()
            .flatten()
            .and_then(|pin| feeds.get(&pin))
        else {
            return false;
        };
        if !self.cells.get(cell).is_some_and(|row| *row > 1) {
            return false;
        }
        let mut pending = filter
            .inputs
            .get(1)
            .copied()
            .flatten()
            .and_then(|pin| feeds.get(&pin))
            .copied()
            .into_iter()
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(port) = pending.pop() {
            if !seen.insert(port) {
                continue;
            }
            if self.record_index.contains(&port) || self.is_private_index(port) {
                return true;
            }
            let producers = functions
                .iter()
                .filter(|function| function.outputs.contains(&port))
                .collect::<Vec<_>>();
            if producers.is_empty()
                && !schemas
                    .iter()
                    .any(|schema| schema.output_keys.contains(&port))
            {
                return true;
            }
            for producer in producers {
                pending.extend(
                    producer
                        .inputs
                        .iter()
                        .flatten()
                        .filter_map(|input| feeds.get(input))
                        .copied(),
                );
            }
        }
        false
    }

    /// A direct scalar binding is per-record only if a structural ancestor is
    /// driven by the first row's Cell and no intervening repetition changes it.
    fn scalar_target_read(
        &self,
        input: u32,
        schemas: &[SchemaComponent],
        feeds: &BTreeMap<u32, u32>,
    ) -> bool {
        schemas
            .iter()
            .filter(|schema| schema.is_target() && !schema.is_variable && !schema.is_pass_through)
            .any(|target| {
                // The legacy CSV reader stores target keys only in `ports`,
                // not `input_keys`; its target role and flat field paths own
                // these bindings. Other families retain explicit input sets.
                let Some(path) = target.ports.get(&input).filter(|_| {
                    target.input_keys.contains(&input)
                        || target.format == ComponentFormat::Csv && target.input_keys.is_empty()
                }) else {
                    return false;
                };
                let mut node = &target.schema;
                let mut repetitions = Vec::new();
                for (depth, segment) in path.iter().enumerate() {
                    let Some(child) = node.child(segment) else {
                        return false;
                    };
                    node = child;
                    if node.repeating {
                        repetitions.push(depth + 1);
                    }
                }
                if path.is_empty() || !matches!(node.kind, SchemaKind::Scalar { .. }) {
                    return false;
                }
                if target.ambiguous_branch_inputs.contains(&input) {
                    return false;
                }
                let ancestors = target
                    .ports
                    .iter()
                    .filter(|&(pin, ancestor)| {
                        ancestor.len() < path.len()
                            && path.starts_with(ancestor)
                            && feeds.contains_key(pin)
                            && target
                                .input_ancestors
                                .get(&input)
                                .is_none_or(|lineage| lineage.contains(pin))
                    })
                    .collect::<Vec<_>>();
                let Some(depth) = ancestors.iter().map(|(_, path)| path.len()).max() else {
                    return false;
                };
                let mut closest = ancestors
                    .into_iter()
                    .filter(|(_, path)| path.len() == depth);
                let Some((pin, _)) = closest.next() else {
                    return false;
                };
                closest.next().is_none()
                    && !repetitions.iter().any(|repetition| *repetition > depth)
                    && feeds
                        .get(pin)
                        .is_some_and(|feed| self.cells.get(feed) == Some(&1))
            })
    }

    fn alignment(
        &self,
        filter: &FnComponent,
        fn_components: &[FnComponent],
        feeds: &BTreeMap<u32, u32>,
    ) -> Option<Alignment> {
        let [Some(nodes), Some(predicate)] = filter.inputs.as_slice() else {
            return None;
        };
        let (Some(aligned), rest) = filter.output_pins.split_first()? else {
            return None;
        };
        let consumed = |output: u32| feeds.values().filter(|feed| **feed == output).count();
        if rest.iter().flatten().any(|output| consumed(*output) > 0) {
            return None;
        }
        let cell = *feeds.get(nodes)?;
        let row = *self.cells.get(&cell)?;
        if row < 2 {
            return None;
        }
        let matched = *feeds.get(predicate)?;
        if consumed(matched) != 1 {
            return None;
        }
        let (equal_index, equal) = fn_components.iter().enumerate().find(|(_, component)| {
            component.library == "core"
                && component.name == "equal"
                && component.kind == 5
                && component.outputs == [matched]
        })?;
        let [Some(left), Some(right)] = equal.inputs.as_slice() else {
            return None;
        };
        let (left_port, right_port) = (*feeds.get(left)?, *feeds.get(right)?);
        let row_port = |port: u32| self.alignment_index.get(&port) == Some(&row);
        let aligned_pair = (row_port(left_port) && self.is_driver_index(right_port))
            || (row_port(right_port) && self.is_driver_index(left_port));
        aligned_pair.then_some(Alignment {
            cell,
            aligned: *aligned,
            equal: equal_index,
            inputs: [*nodes, *predicate, *left, *right],
        })
    }
}

struct Alignment {
    cell: u32,
    aligned: u32,
    /// Index of the `equal` component in the function components.
    equal: usize,
    inputs: [u32; 4],
}

impl Alignment {
    fn absorb(self, feeds: &mut BTreeMap<u32, u32>) {
        for input in self.inputs {
            feeds.remove(&input);
        }
        for feed in feeds.values_mut().filter(|feed| **feed == self.aligned) {
            *feed = self.cell;
        }
    }
}
