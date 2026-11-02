# Decimal value boundary

`Value::Decimal` carries a boxed `Decimal96`. The C# equivalent is
`FerruleValueKind.Decimal` with a `FerruleDecimal96` payload. Decimal identity is
distinct from integer, binary64, string and missing/null values. Boxing preserves
the Rust value union's existing layout; the C# kind is appended without changing
earlier kind numbers.

Every generated C# package includes the bounded decimal implementation because
the shared scalar type references it, even when the mapping has no decimal values.

## Persistence

Decimal values use `{"$decimal96":"<canonical decimal>"}` in model serialization.
The text has no exponent, leading plus, redundant leading or fractional trailing
zeroes, or negative zero. The coefficient and scale retain the bounded decimal
contract. Malformed, overflowing and noncanonical text, duplicate keys and extra
properties are refused. Project files continue through the lossless file codec;
the decimal tag does not reinterpret ordinary JSON numbers.

This is a model encoding, not a JSON document writer policy. Existing scalar
schemas still describe String, Int, Float and Bool, not Decimal.

## Operations and admission

- Explicit scalar text conversion uses canonical decimal text, never binary64.
- Value equality and value-map matching retain decimal identity. Sorting uses a
  separate decimal domain after the existing domains and exact decimal ordering
  within that domain; it does not compare decimals through Float.
- Legacy arithmetic, comparison functions, numeric conversion, numeric reducers
  and decimal item indices remain refused. Numeric aggregates report an
  unsupported scalar rather than treating a finite decimal as non-finite or
  skipping it. Operand failures retain input order.
- Count and item selection preserve scalar identity; text joins use canonical
  text. These are not decimal arithmetic operations.
- Ordinary typed writers and user-function boundaries do not admit Decimal as
  another existing schema type. Rust/C# literal emission reconstructs the exact
  bounded coefficient and scale without Float. `.mfd` export remains refused,
  including stored unused literals and value-map entries in exports.

## Native constant authoring

The constant type picker offers **Exact decimal**, distinct from **Decimal**
(binary64 floating point). Selecting it starts at exact zero. The text field
retains an uncommitted draft across frames and popup closures. Only **Apply** or
Enter parses that draft with `Decimal96::parse_exact`; invalid, incomplete,
overflowing or precision-losing input leaves the stored value unchanged and
shows the parser's error. Accepted input is displayed in canonical form.

The field accepts at most 4,097 characters; the parser enforces the 4,096-byte
decimal lexical limit. The extra character rejects oversized pastes rather than
applying a truncated valid prefix.
No decimal authoring path converts through a float or rounds input. Main,
named-target and function canvases use the same control and existing edit locks,
typed constant commands, Undo/Redo and lossless project persistence. An external
value change, including Undo/Redo, resets an obsolete draft. Drafts are UI state,
not saved project data; ValueMap-cell authoring is unchanged.
Constant and ValueMap controls use private document, semantic canvas, and node
incarnation identities, independent of rebuildable canvas geometry. Source
refreshes and unrelated Undo/Redo retain surviving nodes' drafts; named-target
reordering retains ownership by target rather than tab position. Node deletion,
target/function replacement, document replacement, and new stage-edit sessions
retire ownership even when reusable IDs and saved values are equal. Cell value
changes invalidate stale drafts without changing serialized projects or history.
Output-path, format-option, and scope edits retain the existing named-target
canvas; removal/recreation and schema replacement retire it. Each property
editor registers its exact typed draft, text-edit, and table-scroll state keys
under its private owner. Retiring a cell, node, canvas, document, or stage editor
removes those keys and the ownership registry, without clearing unrelated egui
state. A control's own saved edit keeps its current cursor and undo state.
Private egui temporary keys created inside a property scope (such as Grid
state) are registered by raw key; nested editor registrations keep precedence
and preexisting context entries are excluded. Nested controls share the outer
capture baseline and skip temporary-key discovery when changes consist only of
already registered keys; table/container state is discovered by the outer scope.
Shared egui timing metadata is not claimed by editor scopes. New rows/defaults
rendered before their cell lifetime is published register first-frame state
with the enclosing node, so node disposal also cleans those controls.
Rendering-container identities use reusable slots independent of the logical
draft owners. Retiring an owner closes only its registered popups, surrenders
its text-field focus, and returns its slots for reuse; retained egui Areas and
paint layers are bounded by the peak simultaneously leased containers rather
than edit history. Canvas paint layers are separate from generation-specific
camera, selection, and interaction state. A newly claimed editing-state key is
cleared before rendering, including state restored from egui persistence.
Published cell owners share an immutable hash map, so per-cell lookups do not
copy the table and identity bookkeeping is linear in its cell count.

Decimal constants can feed the explicit `string` conversion for exact text
output. This does not admit Decimal directly at an existing typed output or
function boundary or change schema and arithmetic support.

The admitted conversion and aggregate path is described in
[decimal mapping operations](decimal-mapping-operations.md). Schema/import rules
and native decimal output policies remain separate from explicit canonical text
output; a carrier round trip alone does not establish those capabilities.
