# Explicit decimal conversion and sum

`decimal_from_nullable_text` is an internal scalar operation with one argument.
It accepts String, missing Null and JSON null. Missing/null becomes decimal zero;
an empty or invalid string is an error, not zero. Other scalar kinds, including
an already-decimal value, are refused. The standalone numeric-text parser keeps
its string-only contract.

String conversion uses `Decimal96::parse_decimal_text` and
`FerruleDecimal96.ParseDecimalText`: surrounding whitespace is trimmed and
exponents shift the decimal point exactly before one nearest/ties-to-even
rounding to the bounded decimal domain. Equivalent plain and exponent spellings
have the same result. No Float conversion occurs. The original input is bounded
to 4,096 UTF-8 bytes; extreme exponents produce zero or overflow without allocating
an exponent-sized string. Invalid input and overflow retain typed causes. The
separate legacy `parse_number`/`ParseNumber` binary64 policy is unchanged.

`DecimalSum` is a separate aggregate operation. It consumes only Decimal values,
returns decimal zero for an empty sequence, and refuses an extra aggregate
argument. It never coerces or skips strings, missing values or other scalar kinds.
Every addition uses the bounded decimal arithmetic policy in source order.

Projection and decimal folding are interleaved within the existing aggregate
iteration. An arithmetic error stops before the next projected operand is read;
a later conversion failure must not replace an earlier overflow. Source/cohort
construction retains its own existing evaluation contract. Ordinary aggregate
operations retain their existing eager value collection.

Ordinary collection, generated-sequence and join aggregates use the ordered
decimal fold. Explicit owned-collection and owned-join companion profiles also
admit it: each selected item's expression is evaluated and folded before the
next item is read, using exactly the selected endpoint and its active context.
Other reducers retain their existing eager collection behavior. Earlier profiles
that do not admit owned collections or joins remain closed to those nodes.

The incremental collection helper changes its saved total only after a successful
append. A failed append does not discard or duplicate the previously accumulated
values. A new invocation starts with independent state.

Decimal results can be converted to canonical text with `string` and written
through an ordinary String target. Saved decimal literals also reconstruct the
exact bounded coefficient and scale in generated code. These operations do not
add a Decimal schema type or change MFD import rules. MFD
export refuses the explicit decimal operations until their representation is
separately supported. See [the carrier boundary](decimal-value-domain.md).
