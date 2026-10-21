# Owned scalar target construction corpus

This corpus qualifies owned field and position reads in scalar target
construction, which every Basic owned-source generation companion admits as
described in [the code generation guide](../../docs/code-generation.md#owned-reads-in-scalar-targets).

`projects.rs` defines genuine Projects. Primary and the named `Ref`,
` Ref 雪 `, `ref`, `primary` and `Unused` endpoints share one schema, so every
owned path also exists under the other owners and under a colliding legacy
`Ref/Value` prefix. Every Project except the scalar-primary control has an
`audit` named scalar target reading `Ref`; the Primary target covers root
scalar reads, colliding and literal owner names, an unsupplied owner, lazy
branches, argument order, exact numeric adaptation, and nested scalar fields
inside and beside owned and legacy repetitions. `S09` reads a scalar Primary
endpoint at `[]` without named declarations.

`cases.rs` authors the inputs and the complete expected outputs or typed
failures for the all-output, Primary and `audit` calls. Some calls supply
explicit generated limits for each of the five policy fields; they record the
interpreter's own outcome separately because the interpreter has no
owned-source limits. Walk contexts are owned metadata in generated Rust and a
paired bridge budget in generated C#, so that one refusal records each
backend's exact accounting. `closed.rs` lists every Basic lowering,
validation, catalogue and emission route, the specialized contexts that keep
their refusals under scalar targets, and directly constructed Programs with
structural or dynamic-owner causes.

The CLI `basic_owned_scalar_construction` tests check that every companion
admits, catalogues and emits each Project while the ordinary emitters keep
refusing owned reads, that every interpreter call matches, that refusals hold,
and that legacy scalar construction keeps its ordinary route. The ignored host
test runs the same calls through generated Rust and C# libraries.
