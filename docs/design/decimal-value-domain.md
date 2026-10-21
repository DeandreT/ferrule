# Decimal value boundary

`Value::Decimal` carries a boxed `Decimal96`. The C# equivalent is
`FerruleValueKind.Decimal` with a `FerruleDecimal96` payload. Decimal identity is
distinct from integer, binary64, string and missing/null values. Boxing preserves
the Rust value union's existing layout; the C# kind is appended without changing
earlier kind numbers.

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
  another existing schema type. Rust/C# literal emission and `.mfd` export remain
  refused, including stored unused literals and value-map entries in exports.
- The editor displays saved decimal literals read-only; it does not add a decimal
  authoring choice or automatically turn them into strings or floats.

Decimal conversion functions, decimal aggregates, schema/import rules and output
policies require separate end-to-end admission. A carrier round trip or successful
library build alone does not establish those capabilities.
