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

Unlike ordinary eager reducers, `DecimalSum` projects and folds in source
order: an overflow precedes an invalid value in a later item. This holds
for collection, generated-sequence and joined aggregates, including their
explicitly admitted owned collection/join profiles. A missing tuple
expression supplies Null and fails before the parent argument; an empty
join has no such operand, so its parent argument is still evaluated.
Selection and cohort construction keep their existing order.

The incremental collection helper changes its saved total only after a successful
append. A failed append does not discard or duplicate the previously accumulated
values. A new invocation starts with independent state.

Decimal results can be converted to canonical text with `string` and written
through an ordinary String target. Saved decimal literals also reconstruct the
exact bounded coefficient and scale in generated code. These operations do not
add a Decimal schema type or change MFD import rules. MFD
export refuses the explicit decimal operations until their representation is
separately supported. See [the carrier boundary](decimal-value-domain.md).
