# Primary EDI filename documents

The authored models run through the native CLI, generated Rust, and package-free
generated C#. `cases.json` specifies exact output bytes and ordered host calls.
The cohort covers direct and wildcard X12, EDIFACT, HL7 v2, TRADACOMS, and IDoc
inputs, cross-file grouping, current-document paths, custom delimiters, escaped
data, lenient partial schemas, implied decimals, compact date output, envelope completion, IDoc byte
widths and BOM handling, output code-list validation, and version-4 EDIFACT
repetitions distinct from version-3 literal asterisks. `strict-envelopes.json`
is shared by native and generated hosts for opt-in envelope validation; C#
also checks that strict output validates after completion.
Malformed input must stop before the next file is loaded.

Run the ordinary cohort and its generated hosts:

```sh
cargo test -p cli --features codegen-tests --test code_generation primary_edi
cargo test -p cli --features codegen-tests --test code_generation primary_edi_generated_hosts -- --ignored
```

The generated-host test requires Cargo and .NET 10. Set
`FERRULE_CODEGEN_HOST_TARGET_DIR` to a dedicated generated-host build directory
when the outer test run uses a shared Cargo target.

Schemas retain positional element order. `.edi` is deliberately ambiguous:
its saved boundary must select an EDI family. IDoc requires its embedded layout;
unresolved external configurations are not executable. SWIFT MT has no writer
and is not admitted to a same-format primary document plan.
