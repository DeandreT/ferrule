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
weakened to imitate these isolated outputs. [#1134](https://github.com/DeandreT/ferrule/issues/1134)
tracks the remaining native value boundary.

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
