# Scope evaluation modules

`crates/engine/src/eval_scope.rs` owns scope dispatch and ordinary evaluation.
Its private `eval_scope/` modules separate responsibilities without changing
the public engine API or saved scope model:

- `owned_selection.rs` selects owned candidates and applies their filter/sort
  controls, window accounting, and output compaction. Candidate metadata keeps
  its existing work budget and optional bounded-First replay.
- `owned_grouping.rs` partitions candidate associations, retains groups, and
  builds each selected group's member frame with its exact owner and provenance.
- `owned_document_set.rs`, `owned_dynamic_loading.rs`, and
  `owned_join_scope.rs` handle their respective owned iteration domains.
- `windows.rs` evaluates bounds in the receiving parent and applies ordered
  windows. Owned observations retain that receiving context and the logical
  original cohort size for bounded First selection.
- `construction.rs` evaluates one selected item's construction, bindings,
  children, dynamic output path, XML mixed content, and target-write observations.

Ordinary and owned selection remain separate execution paths. Moving a stage
must not change when expressions, errors, cancellation, trace/debug hooks, or
budget counters are reached. Filters, sort keys, grouping controls, and output
bindings keep their distinct position phases.

Candidate contexts borrow source instances. Ordinary groups own only their
synthetic member collection and borrow intermediate ancestor frames from the
first candidate. Owned groups retain candidate associations until construction,
then pair their member container with the corresponding owner, origin, and
position metadata. Do not clone ancestor trees or merge the ordinary and owned
pipelines to simplify module interfaces.
