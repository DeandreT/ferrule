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
Constant controls also use the current canvas instance identity, so replacing
or reloading a project cannot inherit drafts or text-edit state from another
instance, even when the project contents, node IDs and values are identical.

Decimal constants can feed the explicit `string` conversion for exact text
output. This does not admit Decimal directly at an existing typed output or
function boundary or change schema and arithmetic support.

The admitted conversion and aggregate path is described in
[decimal mapping operations](decimal-mapping-operations.md). Schema/import rules
and native decimal output policies remain separate from explicit canonical text
output; a carrier round trip alone does not establish those capabilities.
