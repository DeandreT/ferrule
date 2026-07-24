# Rust Code Generation Host

This standalone application passes `input.json` to the generated mapping's
bounded `execute_json` API, compares the returned document with
`expected-output.json`, and prints the invoices.

From the repository root, generate the mapping into a new destination:

```sh
cargo +nightly run -p cli -- generate \
  --project examples/codegen/project.json \
  --language rust \
  --out examples/codegen/generated/rust \
  --rust-runtime-path crates/codegen-runtime
```

Then run the host application:

```sh
cargo +nightly run --manifest-path examples/codegen/rust/Cargo.toml
```

Expected output:

```text
Generated 3 invoices:
  1. Ada: ADA / 30.00 EUR (amount 30.00)
  2. Ada: ADA / 12.25 USD (amount 12.25)
  3. Lin: LIN / 19.50 USD (amount 19.50)
```

The generator requires an output directory that does not already exist. Remove
`examples/codegen/generated/rust` before regenerating it.
