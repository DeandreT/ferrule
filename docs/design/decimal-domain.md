# Explicit decimal arithmetic domain

`ir::Decimal96` represents a signed, at-most-96-bit coefficient multiplied by
`10^-scale`, with scale from zero through 28. Its canonical form removes
fractional trailing zeroes and normalizes every zero to positive scale-zero.
Equality, hashing and ordering describe the numeric value, not input spelling.

`from_parts` rejects a coefficient or scale outside those bounds. `parse_exact`
accepts a plain ASCII decimal, optionally signed and with a decimal point, with
at least one digit. It rejects whitespace, grouping separators, exponents,
nonfinite values and inputs longer than 4,096 UTF-8 bytes. No Float conversion
occurs. Input that needs rounding returns `PrecisionLoss`.

`parse_rounded` explicitly selects nearest rounding with ties to even, at the
most precise scale whose rounded coefficient fits. Overflow remains an error.
`Display` writes canonical non-exponent decimal text without unnecessary zeroes.

`add_rounded` and `subtract_rounded` align operands exactly in a private 192-bit
temporary. If the result does not fit, they reduce scale with a single effective
nearest/ties-to-even rounding, retaining discarded-digit information rather
than repeatedly rounding an already rounded value. A result outside the range
at scale zero is an overflow. Parsing precedes arithmetic: two operands that
each round to zero do not become nonzero through delayed parsing.

This type does not change `Value`, `ScalarType`, existing Int/Float arithmetic,
serialization, import admission, generated libraries or format readers/writers.
Those boundaries require an explicit decimal profile before they can use it.
Exponent conversion, multiplication, division and external runtime equivalence
are not supplied by this type's plain-decimal/addition contract.
