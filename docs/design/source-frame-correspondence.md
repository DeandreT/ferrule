# Source frame correspondence

The interpreter keeps borrowed source instances and collection positions in
separate vectors. A position's private `InstanceBinding` records its actual
instance slot. A singleton source descent can append an instance without adding
a position; it must not change any existing collection's owner.

## Construction and composition

- Source walks record extension-relative slots when adding repeated items or
  document-set members. Generated items and direct recursive/XML occurrence
  contexts record their absolute receiving-context slots.
- `WalkExtension::append_to` appends both vectors and rebases only the appended
  positions. `append_positions_to` provides the same rebasing when constructing
  positions separately. Do not concatenate executable position vectors directly.
- Join correlation starts a new row-local binding. Combining rows rebases the
  right row's slots before attaching the completed row to its receiving context.
- Grouping replaces the terminal binding with the new member collection's slot,
  after the optional named wrapper. Sorting and selection change ordinals, not
  instance bindings.
- `SourceFrame::position_slot` in the owned-source API is the inverse association:
  it addresses a position, not a source-instance slot. Its indexing is unchanged.

## Resolution and compatibility

`FrameView::receiving` and `latest_receiving` use the same resolver. A selected
explicit unbound or out-of-range slot never falls back to an earlier frame or to
an outward absolute lookup. Ordinary unframed relative reads retain their
innermost-first fallback.

`LegacyUnspecified` preserves end-relative receiving-context pairing and
index-relative joined-row pairing for hand-built legacy contexts. Unvalidated
join sources that violate the repeating-terminal contract retain that legacy
row pairing, including shifts caused by unpositioned members. Runtime source
walks otherwise always annotate correspondence.

This is private execution metadata, not a serialized project or source-ownership
change. The generated Rust runtime stores the instance and collection identity
together in each scope frame; the C# runtime's collection identities retain their
actual instance. Their existing correspondence does not need this slot layer.
