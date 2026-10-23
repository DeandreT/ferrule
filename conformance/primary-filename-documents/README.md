# Primary filename documents

This self-authored corpus checks complete primary CSV document runs through
`engine::run_primary_csv_document` and the `emit_with_primary_csv_documents`
emitters, in the interpreter, generated Rust and generated C#, all using the
explicit local filesystem host. See
[the design](../../docs/design/primary-document-filenames.md).

- [`projects.rs`](projects.rs) builds three document plans over one grouped
  mapping: rows with equal `Key` combine in first-seen order across every
  selected document, `Text` joins with `+`, `Count` sums and `Status` comes
  from the `Status` runtime parameter. Saved preview paths and parameter
  previews are present but never used.
  - `literal_direct` (model 0): the constant `orders/input.csv`, direct.
  - `parameter_wildcard` (model 1): the `Input` parameter, filename wildcard,
    an empty match refused.
  - `parameter_wildcard_map_no_documents` (model 2): the `Input` parameter,
    filename wildcard, an empty match mapped as zero documents.
- [`cases.json`](cases.json) lists each case's file tree (`files`, `dirs`, and
  `symlinks` for Unix-only cases), runtime parameters, optional document and
  byte limits, the ordered host calls the run reaches, and the complete output
  document or a typed failure `[stage, kind, subject]`.

| Case | Model | Expected outcome |
|---|---|---|
| P01 | 0 | One direct document, grouped. |
| P02 | 0 | A missing direct file is a load failure. |
| P03 | 0 | A directory at the direct path is a load failure. |
| P04 | 1 | Three documents in Unicode scalar order (`B`, `a`, `é`), grouped across documents; a non-matching file and a matching directory are skipped. |
| P05 | 1 | An empty match is refused at selection; nothing loads. |
| P06 | 2 | An empty match maps zero documents: a header-only document. |
| P07 | 1 | The second document fails to parse; the third is never loaded and nothing is mapped. |
| P08 | 1 | A missing filename parameter fails before selection. |
| P09 | 1 | A URI is refused at selection; no network access. |
| P10 | 1 | A parent component is refused. |
| P11 | 1 | A rooted path is refused. |
| P12 | 1 | A drive path is refused. |
| P13 | 1 | A wildcard in a directory component is refused. |
| P14 | 1 | A document limit of one stops before the second load. |
| P15 | 1 | Every document loads, then the single mapping run fails on the missing `Status` parameter. |
| P16 | 1 | A wildcard-mode filename without wildcards, with `.` and `\` separators, names one document. |
| P17 | 1 | A document over the byte limit is a load failure. |
| P18 | 1 | A matching symbolic link is refused at selection (Unix). |
| P19 | 0 | A direct path through a linked directory is refused at load (Unix). |
| P20 | 2 | A header-only document contributes no rows beside a later document. |
| P21 | 2 | A missing wildcard directory is a selection failure even when empty matches are mapped. |

Expected documents are literal authored values, not output copied from any
backend. The CLI harness
[`primary_filename_documents.rs`](../../crates/cli/tests/code_generation/primary_filename_documents.rs)
checks the case inventory, the saved plans and their lowering, deterministic
emission, and every interpreter case. Its ignored test builds one generated
Rust host for all models and one generated C# host per model and compares
every case:

```sh
cargo test -p cli --test code_generation --features codegen-tests primary_filename_documents -- --include-ignored
```

The ignored test generates model 1 through `ferrule generate
--primary-csv-documents` under a configured identity with the compatibility
manifest. [`primary_document_run.rs`](../../crates/cli/tests/primary_document_run.rs)
runs every case through `ferrule run-document` and the CLI document run
library and compares them with the interpreter.
