# XML target float conversion

In the qualified native XML controls, the reported failure matches the first
declared Amount feed. This identifies diagnostic selection, not the native
application's internal evaluation timing. The bounded import path for
[#1133](https://github.com/DeandreT/ferrule/issues/1133) reproduces that selection
by evaluating an internal `coerce_float` graph call at the binding, rather than
deferring the conversion to output serialization.

## Scalar behavior

The helper trims and parses String values directly as finite binary64 values.
It does not first parse integers, so signed zero and large numeric strings keep
the existing XML writer's conversion behavior. Every non-String tag passes
through unchanged, including absent/null markers, XML nil, decimals and
non-finite typed floats. Their existing target policies are not moved or
changed. The helper is internal, not a new GUI authoring function.

## Import boundary

The initial native witness requires one ordinary primary XML source and a flat,
uncontrolled XML target scope with repeated scalar bindings. The exact protocol
document wrapper must declare `cast-in-subtree` or use the documented enabled
default with no cast marker. Unknown settings and explicit overrides are not
treated as the default.

Each admitted binding has one unambiguous raw input, a direct output from the
source's required nonnillable String field, and a plain `xs:double` target field.
Resolved flat XSD metadata, raw owners/ports, the lowered source node and the
post-canonicalization binding occurrence must agree. Target/source controls,
overrides, unresolved metadata and other types do not acquire this helper.
Canonical entry metadata must agree with the schema; it does not replace the
schema's type authority. Namespace slots on payload roots and fields must resolve
to the unqualified schema namespace. Separate cloned entries receive separate
helper nodes.

## Export boundary

The helper remains an explicit Ferrule component in extension exports. Strict
native export refuses it: native numeric output formatting and lexical
acceptance are not qualified as lossless substitutes for the helper. Independent
date-time lowering remains available without removing the float helper.
Canonical generic Float schemas remain `xs:decimal`, so they do not introduce
another double-only conversion during reimport.

The [native target-casting controls](../conformance/xml-target-casting.md)
record the value counterexamples and the distinct behavior with casting disabled.

Extension export also refuses before publication when canonical port order
would change a flat root's cross-field binding order. This check applies even
to a single converted binding; retaining the helper alone would not preserve
its position relative to a failing expression on another target field.

This boundary does not claim nested or multi-source
conversion ordering, arbitrary numeric aliases, or general native evaluation
scheduling. Singular duplicate bindings retain their existing refusal.
