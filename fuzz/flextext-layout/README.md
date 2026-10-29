# FlexText layout-and-input fuzz target

A standalone [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) harness for
the FlexText layout validator in `crates/mapping` (`FlexTextLayout::new` and its
JSON deserialization) and the structured-text reader and writer in
`crates/format-flextext` (`format_flextext::from_str` and `to_string`). It is
not a member of the root workspace: the empty `[workspace]` table in
`Cargo.toml` makes this directory its own workspace with its own `Cargo.lock`,
so root builds, tests and CI never compile it.

## Input format

Each fuzz input takes one of two forms:

- **JSON layout** — the input starts with `{`. Bytes up to the first NUL are a
  JSON `FlexTextLayout`; the bytes after it are the input text.
- **Layout program** — any other input is a compact byte program
  (`src/program.rs`) that builds a command tree through the public `mapping`
  constructors, followed by the input text. Every byte string decodes: reads
  past the end yield zero and every selector wraps. Opcodes cover `Store` (with
  trimming), `Ignore`, `SplitOnce`, `SplitMany`, fixed-width and delimited
  records, `Switch` with literal and regular-expression arms, a chain of up to
  70 nested wrappers (past the depth limit) and a switch with up to 4,200
  generated arms (past the node limit and, for regular expressions, the
  64-pattern limit). String operands come from small pools with empty,
  line-break, BOM, multi-byte and NUL entries, raw bytes, or strings exactly at
  and one byte over the size limits.

Invalid UTF-8 in the input text is replaced with U+FFFD.

## What it checks

For each input:

1. Layout construction returns a layout or a typed rejection and is
   deterministic. A command tree the constructor rejects is rejected with the
   same message when it arrives as JSON. The one distinct outcome is a JSON
   form nesting 128 or more arrays and objects: serde_json refuses it with its
   own recursion-limit error before the constructor runs. That error is
   accepted only for a tree the constructor also rejects and whose JSON
   really nests that deep.
2. A validated layout stays within the validator's limits: 64 nested
   commands, 4,096 commands, 1 MiB of layout strings, 64 distinct switch
   regular expressions and 96 levels of serialized JSON nesting. It serializes
   to JSON that decodes to an equal layout.
3. Reading the input text returns either an instance or one of
   `FlexTextError::Data`, `TooManyRecords` or `TooManyNodes`. Any other error,
   or a panic, is a finding. Reading twice gives the same result. Unless the
   text already starts with a byte-order mark or the read reached the record
   or node limit, adding a leading byte-order mark does not change the
   result.
4. A parsed instance has the shape its layout describes: fields appear once,
   in command order, with the declared scalar types (null only for empty
   record fields); only `Ignore` and unmatched `Switch` outputs may be absent;
   a switch never holds both arm and default output, and a first-match switch
   holds at most one arm. It has at most 1,000,000 nodes and records, depth at
   most `2 * 64 + 1`, and no value longer than its input.
5. Writing the instance back returns output within the 8 MiB limit (plus an
   optional BOM) with the layout's line endings (see the carriage-return
   limit below), or one of
   `FlexTextError::Data`, `OutputTooLarge` or `InstanceTooDeep`, and is
   deterministic. Output small enough is read again under rule 3. Writing need
   not reproduce the original input.

## Per-input budget

- **Input size:** the target ignores inputs larger than 64 KiB
  (`MAX_INPUT_BYTES`). The smoke run below also passes `-max_len=4096`.
- **Program size:** each program decodes at most 1,024 commands, 70 chained
  wrappers per chain, 4,200 generated switch arms in total (66 of them regular
  expressions) and string operands totalling 1 MiB + 256 bytes. Once that
  budget leaves no room, a selector for a string at or over a size limit
  decodes as the pool's first entry instead.
- **Time and memory:** smoke and long runs pass `-timeout=10` (seconds per
  input) and `-rss_limit_mb=2048`. Inputs that reach the 1,000,000 record or
  node limit take a few seconds each under sanitizers. libFuzzer reports a slow
  input or excess memory as a crash artifact.

## Requirements

- cargo-fuzz 0.13 (`cargo install cargo-fuzz`).
- The nightly toolchain pinned in `rust-toolchain.toml`
  (`nightly-2026-09-07`). Commands run from this directory pick it up
  automatically; from the repository root pass `+nightly-2026-09-07`.

Every command below runs from this directory. `--fuzz-dir .` tells cargo-fuzz
that this directory is the fuzz project.

## Build and smoke run

```sh
cargo fuzz build --fuzz-dir . layout
mkdir -p corpus
cargo fuzz run --fuzz-dir . layout corpus seeds -- \
  -max_total_time=120 -max_len=4096 -timeout=10 -rss_limit_mb=2048
```

`corpus/` is the writable working corpus; `seeds/` is read-only input. Both
`corpus/` and `artifacts/` are ignored by Git. Raise `-max_total_time` for a
longer campaign; keep the other bounds unchanged.

## Reproduce and minimize a finding

libFuzzer writes a failing input to `artifacts/layout/crash-*` (or `timeout-*`,
`oom-*`).

```sh
# Replay one input; the panic message names the invariant and path.
cargo fuzz run --fuzz-dir . layout artifacts/layout/crash-<hash>

# Shrink it to a minimal reproduction.
cargo fuzz tmin --fuzz-dir . layout artifacts/layout/crash-<hash>
```

Copy the minimized input to `regressions/<short-name>.bin`; `cargo test`
replays every file there. Report the defect as a separate issue with that
reproduction rather than fixing the validator or runtime in the harness change.

To shrink a grown working corpus: `cargo fuzz cmin --fuzz-dir . layout corpus`.

## Seeds and tests

`seeds/` is generated by `src/seeds.rs`, which writes layout programs with the
program encoder and a few JSON layouts. They cover every splitter, store
trimming, fixed-width and delimited records (quoting, escapes, multi-character
separators, Unicode fill and short final fields), first-match and all-possible
switches with literal and regular-expression arms, CRLF and BOM input, Unicode
column and delimiter boundaries, layouts at and one past the depth, node,
string-size, regular-expression and serialized-JSON-depth limits, inputs that
reach the record and node limits, and each layout rejection. Regenerate after changing the
generator:

```sh
cargo run --example write_seeds
```

`cargo test` checks that `seeds/` matches the generator, that each seed reaches
its intended layout, read and write outcome, that every prefix of every short
seed satisfies the invariants, and that each saved regression still passes.
These tests do not need sanitizer instrumentation.
