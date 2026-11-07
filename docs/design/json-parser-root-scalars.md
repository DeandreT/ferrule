# JSON parser root scalars with a bounded array lookup

An admitted parsed-array lookup can coexist with independent, connected root
scalar outputs from the same parser. The parser still requires one uniquely
declared required global String input and a resolved schema. Each extra output
must be a declared, nonrepeating scalar at a one-property root path, with a unique
raw output declaration and no competing producer. Untyped JSON values, nested
scalars, structural outputs and additional connected array fields do not extend
this profile.

Root values use ordinary `json_parse_field` nodes and retain their own target
bindings. They do not supply array records, become physical source fields or
acquire a primary-item frame. Lookup key and value feeds remain exclusive to the
owned equality filter. The receiving primary item still owns result repetition
and order; a private generated index owns only the lookup scan. All existing
raw-pin, expression, cardinality and actual receiving-frame checks remain in
force. See [the lookup contract](../mfd-interop.md) for those bounds.

Both scalar reads and array helpers validate the complete parser document,
including later duplicate records and unconnected schema properties. Missing
optional values retain the absence marker; permitted explicit JSON null remains
distinct. The first matching record wins even if its value is missing or null.
An empty primary iteration does not demand a lookup, but independently connected
metadata still demands its root reader and complete validation. No evaluation
order is promised between independent target bindings.

## Regression and qualification boundaries

The self-authored [root fixture](../../crates/mfd/tests/fixtures/json-array-lookup/root-scalars/README.md)
contains a finite literal catalog and independent Rust/C# host oracles. It covers
complete typed values, primary and duplicate order, absence/null, empty and
absent arrays, required root fields, malformed late records, unused fields and
missing input parameters. Graph assertions check global input identity, complete
schema identity, root bindings, the physical receiving frame and the private
lookup index separately from output assertions.

The [filtered-key controls](../../crates/mfd/tests/fixtures/json-array-lookup/filtered-key/README.md)
are reference questions, not expected results. A filtered final conditional
branch remains refused with or without independent metadata. No-result,
scalar absence, explicit null and lazy branch demand cannot be equated without
executed reference evidence and a complete receiving-owner proof. Shared
predicates, foreign frames, multivalued keys and unsupported expressions remain
refused. Local interpreter/generated-host agreement is not vendor parity.

Runtime parsing reuse and work bounds belong to a separate layer. This importer
extension introduces no cache, schema/wire change or large-array performance
guarantee.
