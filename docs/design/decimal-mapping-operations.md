# Explicit decimal conversion and sum

`decimal_from_nullable_text` is an internal scalar operation with one argument.
It accepts String, missing Null and JSON null. Missing/null becomes decimal zero;
an empty or invalid string is an error, not zero. Other scalar kinds, including
an already-decimal value, are refused. The standalone numeric-text parser keeps
its string-only contract.

String conversion uses the bounded numeric-text policy of `Decimal96` and
`FerruleDecimal96`: surrounding whitespace is trimmed, plain decimal text is
rounded to the bounded decimal domain, and exponent text explicitly passes
through binary64 conversion. The exponent route is not interchangeable with an
equivalent plain decimal spelling. Invalid input and overflow retain typed causes.

`DecimalSum` is a separate aggregate operation. It consumes only Decimal values,
returns decimal zero for an empty sequence, and refuses an extra aggregate
argument. It never coerces or skips strings, missing values or other scalar kinds.
Every addition uses the bounded decimal arithmetic policy in source order.

Projection and decimal folding are interleaved within the existing aggregate
iteration. An arithmetic error stops before the next projected operand is read;
a later conversion failure must not replace an earlier overflow. Source/cohort
construction retains its own existing evaluation contract. Ordinary aggregate
operations retain their existing eager value collection.

The explicit owned-collection and owned scalar-sequence companion profiles keep
their previously qualified reducers. They do not admit `DecimalSum`: the owned
collection interpreter refuses it before reading item values, and generation,
emission and named-input admission refuse the unqualified operation. Legacy
collection, generated-sequence and join aggregates retain their independently
implemented ordered decimal fold.

The incremental collection helper changes its saved total only after a successful
append. A failed append does not discard or duplicate the previously accumulated
values. A new invocation starts with independent state.

Decimal results can be converted to canonical text with `string` and written
through an ordinary String target. These operations do not add a Decimal schema
type, admit decimal literals to generated code, or change MFD import rules. MFD
export refuses the explicit decimal operations until their representation is
separately supported. See [the carrier boundary](decimal-value-domain.md).
