# rust_xlsxwriter 0.96.0 — Ferrule patch

This is the library source from the crates.io `rust_xlsxwriter` 0.96.0 package
(`https://crates.io/crates/rust_xlsxwriter/0.96.0`), upstream
`https://github.com/jmcnamara/rust_xlsxwriter`. The package's VCS metadata identifies
upstream revision `9134de25afadaee955d0f821862338e3d046a338`.
It was imported from the matching native Cargo registry source, without changing
the registry cache. Upstream's MIT and Apache notices are retained alongside it.
The published package SHA-256 is
`dd1746025420e17b5d62528b930e550e016e857038794d74e169018126ef3d14`.
`LICENSE_MIT` and `LICENSE_Apache2.0` are retained verbatim as `LICENSE-MIT`
and `LICENSE-APACHE`.

Only the library source and the `CHANGELOG.md` that its documentation module
includes are kept. Examples, benchmarks, tests, media and registry bookkeeping are
omitted. The manifest keeps the package identity and its required `zip` dependency
and offers no optional features (Ferrule uses none).
Ferrule selects this copy through its workspace dependency, so crates that depend
on Ferrule's libraries from Git use it too. No machine-local source paths are used
at build time.

The local patch is one change in `src/packager.rs`: worksheets are assembled one
after another on every target, as upstream already does for `wasm32`, instead of
on scoped threads. Saving a workbook therefore never creates a thread, which hosts
that forbid thread creation require. The assembled XML and the written package are
produced by the same per-worksheet assembly and package-writing code, in the same
worksheet order.

To refresh, compare the library source and manifest against the same published
version, reapply the packager change and keep the thread-free save test passing.
