# XML target casting controls

These bounded observations use MapForce Enterprise 2026r2 x64, executable
28.2.0.0, offline under an isolated Wine prefix and virtual display. They are
not native Windows ABI evidence. Designs were independently authored; original
schemas, inputs and output sentinels were guarded. Each tested design was saved,
reopened and resaved before execution.

## Enabled casting and failure selection

One source-driven divide expression and one direct String feed populate two
separate, repeated `Amount` entries declared as `xs:double`. A `Label` follows.
Positive controls retain values in either declared order. Separate failing
controls distinguish division by zero from an invalid numeric string.

When both fail, the reference reports the failure of the first declared Amount
feed: `Numeric overflow.` for divide first, or
`Conversion to double failed for 'not-a-number'` for String first. These are
complete observable diagnostics, not claims about internal evaluation timing.
The [bounded conversion implementation](../design/target-float-cast.md) preserves
that selection in Ferrule and generated Rust/C# XML paths. The native and
Ferrule error types and wording are not treated as equivalent.

Native save/re-save makes the enabled document setting explicit as
`casttotargettypemode="cast-in-subtree"`. The independently authored absent-marker
controls exercise the documented enabled default as well.

## Enabled casting does not prove general value fidelity

Separate direct String-to-double controls retain the same second Amount `17`
and Label `safe`:

| Input String | Native observation |
| --- | --- |
| `-0` | Emits `-0`; the serialized value retains the expected negative-zero bits when reparsed |
| `9007199254740993` | Emits `9.00719925474099E+15`, which does not reparse to the expected binary64 value |
| Tab, space, `2.5`, space, newline | Execution fails with conversion-to-double failure |
| `1.25e2` | Emits `125` |

The large-number and whitespace controls are counterexamples, not passing
qualification. The output does not establish where or how the reference lost
precision internally. Ferrule's existing XML writer accepts the tested
whitespace and uses its own round-trip numeric rendering; those rules are not
weakened to imitate these isolated outputs.

## Enabled casting lexical boundary

Further enabled controls reuse the independently authored exponent design and
schemas unchanged; only the input String differs. Each design was saved,
reopened and resaved, and its complete output and messages were retained.

| Input String | Native first Amount | Ferrule `coerce_float` |
| --- | --- | --- |
| `+2.5`, `.5`, `5.`, `1E+03` | `2.5`, `0.5`, `5`, `1000` | Same |
| Leading space, `2.5` | `2.5` | `2.5` |
| `2.5`, trailing space | Fails | `2.5` (trimmed) |
| Empty, `Infinity`, `0x10`, `2,5` | Fails | Fails |
| `INF`, `NaN` | `INF`, `NaN` | Fails (finite values only) |
| `-INF` | `INF` | Fails |
| `1e400` | `INF` | Fails |
| `1e-400` | `0` | `0` |
| `00029`, `0.1`, `0.0001`, `123456789012345` | `29`, `0.1`, `0.0001`, `123456789012345` | Same |
| `0.30000000000000004` | `0.3` | `0.30000000000000004` |
| `3.141592653589793` | `3.14159265358979` | `3.141592653589793` |
| `1234567890123456` | `1.23456789012346E+15` | `1234567890123456` |
| `123456789012345678` | `1.23456789012346E+17` | `123456789012345680` |
| `1e21`, `1e-7` | `1E+21`, `1E-07` | `1000000000000000000000`, `0.0000001` |
| `1.5e300`, `1.7976931348623157e308` | `1.5E+300`, `1.79769313486232E+308` | Full positional round-trip decimal |
| `4.9406564584124654e-324` | `4.94065645841247E-324` | Full positional round-trip decimal |

Native failures report `Conversion to double failed for '<text>'`. Native output
keeps at most 15 significant digits, switches to an exponent from 16 digits or
below `1E-05`, and writes signed two-digit exponents. It accepts XML Schema
special values and overflow, rejects trailing whitespace, and writes `-INF` as
`INF`. The second Amount `17` and Label `safe` were unchanged in every
successful run.

Ferrule keeps its own contract: trimmed input, finite binary64 values only, and
shortest round-trip positional output. Native `xs:double` casting therefore has
no value-independent lossless subset that could replace the helper, so strict
native export keeps its typed refusal. `target_float_cast_lexical_edges_match_across_backends`
pins these inputs for the interpreter and generated Rust and C#.

## Disabling casting is a different contract

The Target component's **Cast values to target types** option was explicitly
unchecked through the native Component Settings dialog. The saved Target then
uses `casttotargettypemode="suppress-cast-in-subtree"`; the Source retains its
enabled setting. The graph and target-entry order are unchanged.

| Input String | Native first Amount with target casting disabled | Target XSD |
| --- | --- | --- |
| `9007199254740993` | Exact input text | Valid |
| Tab, space, `2.5`, space, newline | Exact whitespace and text | Valid |
| `not-a-number` | Exact input text | Invalid |
| `00029` | `00029` | Valid |

All four runs succeed, retain the second Amount `17` and Label `safe`, and keep
their source files and sentinels unchanged. Their native saved/resaved designs
are byte-identical. Successful execution of the invalid-number case does not
make its output schema-valid.

Ferrule's ordinary typed writer normalizes the three numeric strings and
rejects the invalid one. Its explicit
[target-cast suppression policy](../design/xml-cast-suppression.md) preserves
the raw values on the supported flat XML boundary while retaining the numeric
schema. Unsupported explicit requests remain execution-blocking. Disabling
native casting is not a lossless substitute for `coerce_float`: it changes both
formatting and invalid value detection. The conversion helper therefore remains
an explicit Ferrule extension, and strict native export remains refused.

## Nested formatted output

A reference sample design writes `format-number(price, "00000.00")` into a
repeated `Article` whose `SinglePrice` is a pattern-restricted `xs:decimal`,
with target casting disabled on the document. It was observed with case-local
instance paths, the built-in engine and its preview-only stylesheet reference
removed.

| Design | Native `SinglePrice` values | Target XSD |
| --- | --- | --- |
| Target casting disabled | `00025.00`, `00002.30`, `00034.00`, `00057.50` | Valid |
| Same design, casting enabled | `25`, `2.3`, `34`, `57.5` | Invalid |
| Ferrule native export of its import | `00025.00`, `00002.30`, `00034.00`, `00057.50` | Valid |

Ferrule imports the disabled design with the document-wide suppression
profile; its interpreter, generated Rust and generated C# produce the same
article values. All three native runs saved and resaved byte-identical
designs. The reference application also adds an absolute
`xsi:noNamespaceSchemaLocation` when `AddSchemaLocation` is absent; Ferrule's
XML import does not reproduce that automatic hint for any target;
[#1138](https://github.com/DeandreT/ferrule/issues/1138) tracks it.

These controls do not establish other numeric types, arbitrary schemas, general
null semantics, or a way to select a winner among competing singular bindings.
