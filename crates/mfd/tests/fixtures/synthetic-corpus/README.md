# Synthetic whole-design corpus

These designs are authored from scratch from public X12 004010 structure and
MapForce component vocabulary. Every name, code, identifier and value is
invented; none is captured from a real integration.

Each design is shaped like an ordinary warehouse EDI integration rather than a
single feature: JSON documents to X12 945, 944, 947 and 846; X12 940, 943, 850
and 856 to JSON or XML; an X12 997 acknowledgement built from an inbound 940;
and a CSV extract to X12 846. Designs combine the companions such integrations
usually carry together: envelope constants with `now`/`format-dateTime`,
custom, absent and String-to-String input value-map default modes, a named
same-file function library, `auto-number`, `group-by`, `sum`/`count`,
`filter`, JSON string parsing and date and number conversions.

Some designs are realistic variants that Ferrule currently refuses, such as
connected `FileInstance` file-name ports, an `if-else` driving an optional
segment, cloned target segments fed by parsed values, and counts over filtered
or distinct sequences.

`synthetic_corpus.rs` imports every design and compares each one with
`ledger.json`: best-effort import, the executable assessment with every
import, runtime-dependency and validation issue, and Rust and C# source
generation from the lowered program. A refusal is recorded by its issue kind
and a stable fragment of its message. When a change alters an outcome, the
failing test prints every observed outcome; update the ledger entry and keep
each fragment short.

The ledger records static admission only. It does not execute designs or
compare their output.

All designs share `Defs.Segment`, `Envelope.Config` and one `<code>.Config`
per message type, and select messages through `config="Envelope.Config"`
with a `<messages>` list.
