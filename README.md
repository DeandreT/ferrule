# ferrule

ferrule is an open-source graphical data mapper written in Rust. Connect source
fields to target fields in the editor, then run the saved mapping from the CLI.
For example, turn nested XML orders into one CSV row per item.

## Quick start

From the repository root, with [rustup](https://rustup.rs/) installed:

```sh
cargo run -p cli -- run \
  --project crates/cli/tests/fixtures/orders/project.json \
  --input crates/cli/tests/fixtures/orders/Orders.xml \
  --output order-lines.csv
```

`order-lines.csv` should contain [six item rows](crates/cli/tests/fixtures/orders/expected_order_lines.csv).
To build a mapping visually, run `cargo run -p gui` and follow
[Your first mapping](docs/getting-started.md).

## Learn more

- [Run and validate mappings](docs/running-mappings.md)
- [Supported formats](docs/formats.md), including XML, JSON, spreadsheets and EDI
- [Generate Rust or C# mapping libraries](docs/code-generation.md) for the supported subset
- [All guides](docs/README.md) and [contributing](CONTRIBUTING.md)

Licensed under the [GNU General Public License v3.0](LICENSE).
