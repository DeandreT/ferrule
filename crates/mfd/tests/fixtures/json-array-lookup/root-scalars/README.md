# Independent root scalars and an array lookup

This self-authored fixture connects String, integer and Boolean root outputs
from the same global-input parser as a primary-item array lookup. Root outputs
retain ordinary scalar parsing; array key/value feeds remain exclusive to the
lookup. Unconnected nested, structural and extra array ports are refusal controls,
not additional supported outputs.

`cases.json` checks complete typed output, primary and duplicate order, missing
versus explicit JSON null, absent arrays, schema-required root fields and
full-document validation. Missing optional root scalars remain absent whether or
not their schema allows explicit null. `Enabled` is required, so its absence is
a full-document schema failure. Empty primary cases isolate root parse demand from
lookup demand. For malformed complete input with a nonempty primary, either
reader may observe the schema failure first; the oracle requires the same typed
failure and exact detail without inventing an evaluation order between
independent target bindings. Empty-primary error controls require the root reader.

The Rust and C# hosts run this same finite case file in Release mode. Their
oracles follow Ferrule's existing JSON reader, not recorded MapForce observations:
missing optional scalars become `Value::Null`, the absence marker, while permitted
explicit JSON null becomes `Value::JsonNull`. Expected JSON omits absent root
properties; parsing it against the target schema supplies the typed absence
markers used by the complete-output assertions.
Reference absence/null/lazy-branch qualification uses the separate `filtered-key`
fixture and must not be inferred from these hosts or from emission alone.
