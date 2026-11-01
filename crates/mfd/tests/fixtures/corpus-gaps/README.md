# Composed import contracts

`../../corpus_gap_contracts.rs` authors its small MFD packages, schemas and
synthetic values in temporary directories. No external sample corpus or vendor
installation is required.

- Filter-before-distinct preserves first-seen string identity, including leading
  zeros and case. Distinct-before-filter and distinct-before-sort require exact
  diagnostics and executable-import refusal.
- Structured EDI counts accept an enclosing context, but reject sibling contexts
  and a context from another source with the same schema. Typed input instances
  isolate this contract from EDI wire parsing.
- A JSON string parser preserves complete values with an unconnected target
  wrapper. Connecting an unsupported runtime filename must refuse executable
  import without losing the parser.

These tests establish interpreter and strict-import contracts only, not generated
backend execution or native application parity. Best-effort placeholder results
are not accepted as successful execution of unsupported mappings.
