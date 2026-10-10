# Mapping Model and Workspace Architecture

## System Architecture

Solid arrows show data and emitted-artifact flow. Dotted arrows show crate
dependencies or design-time relationships. An [editable draw.io version](architecture-parity.drawio)
contains the architecture and qualification-gate views.

```mermaid
flowchart TB
    subgraph interfaces["Interfaces"]
        direction TB
        gui["Native GUI<br/>gui + editor-ui"]
        web["Web demo<br/>WASM + real engine"]
        cli["CLI and host API<br/>filesystem or payload execution"]
        host["Embedding application"]
    end

    subgraph design["Mapping design"]
        direction TB
        projectFile["Ferrule project JSON"]
        mfdFile[".mfd design"]
        mfd["mfd importer/exporter<br/>confined resource resolution<br/>and actionable warnings"]
        project["mapping::Project<br/>schemas, expression graph, scopes,<br/>endpoints, options, UDFs, and failures"]
    end

    subgraph model["Shared typed model"]
        direction TB
        schema["SchemaNode<br/>format-independent document shape"]
        instance["Instance and Value<br/>scalar, group, repetition,<br/>mapped sequence, and document set"]
        functions["functions<br/>typed scalar built-ins"]
    end

    subgraph boundary["Format boundaries"]
        direction TB
        inputs["Primary and named inputs<br/>paths, URLs, or bounded payloads"]
        readers["Readers and schema importers<br/>XML, JSON, CSV, XLSX, SQLite,<br/>EDI, FlexText, Protobuf, PDF, and XBRL"]
        writers["Writers and validators<br/>supported output adapters"]
        artifacts["Serialized artifacts<br/>filesystem paths or host-owned bytes"]
    end

    subgraph runtime["engine interpreter"]
        direction TB
        validate["Validate project<br/>before execution"]
        context["Build execution context<br/>sources, parameters, paths,<br/>stable time, and tracing"]
        iterate["Evaluate scope pipeline<br/>iterate, join, filter, group,<br/>sort, and window"]
        evaluate["Evaluate graph<br/>fields, calls, branches,<br/>UDFs, lookups, and aggregates"]
        construct["Construct ordered targets<br/>primary plus named outputs"]
    end

    subgraph generation["Static code generation"]
        direction TB
        lower["codegen<br/>validate and lower the<br/>reachable portable subset"]
        rust["codegen-rust<br/>generated Rust library"]
        csharp["codegen-csharp<br/>generated .NET library"]
        runtimes["Generated runtimes<br/>Rust crate or vendored C# sources"]
    end

    projectFile --> project
    mfdFile --> mfd --> project

    gui -.-> project
    web -.-> project
    cli -.-> project
    host --> cli

    project -.-> schema
    project -.-> instance
    readers -.-> schema
    readers -.-> instance
    inputs --> readers

    project --> validate
    readers --> context
    validate --> context --> iterate --> evaluate --> construct
    functions -.-> evaluate

    construct -.-> instance
    construct --> writers --> artifacts

    project --> lower
    lower --> rust
    lower --> csharp
    runtimes -.-> rust
    runtimes -.-> csharp

    gui -.-> cli
    web -.-> validate
    cli -.-> readers
    cli -.-> writers
    cli -.-> validate
    mfd -.-> readers

    classDef interface fill:#dbeafe,stroke:#2563eb,color:#172554
    classDef design fill:#dcfce7,stroke:#16a34a,color:#14532d
    classDef format fill:#fef3c7,stroke:#d97706,color:#451a03
    classDef engine fill:#fee2e2,stroke:#dc2626,color:#450a0a
    classDef generated fill:#f3e8ff,stroke:#9333ea,color:#3b0764
    classDef artifact fill:#ccfbf1,stroke:#0f766e,color:#042f2e

    class gui,web,cli,host interface
    class projectFile,mfdFile,mfd,project,schema,instance,functions design
    class inputs,readers,writers format
    class validate,context,iterate,evaluate,construct engine
    class lower,rust,csharp,runtimes generated
    class artifacts artifact
```

## Project Model

A ferrule project is plain JSON built from four main concepts:

1. **Schemas** describe source and target trees. Nodes are named scalar or group
   values and may be repeating, attributes, nullable, fixed, or dynamically
   named where the target format permits it.
2. **Graph nodes** compute scalar values. Nodes read source fields and positions,
   hold constants, call built-in functions, branch lazily, translate value maps,
   perform lookups and joins, reduce collections, and access selected runtime
   values, or raise a typed error when their expression is reached.
3. **Scopes** construct target groups. A scope can iterate source collections,
   generated scalar sequences, document sets, or validated joins, then filter,
   group, sort, window, bind fields, and construct child scopes. The default
   `Constructed` constructor requires a group target schema; scalar targets use
   explicit `Scalar` construction. Project validation checks this shape for
   primary and named targets before lowering or executable design admission.
4. **Endpoints** identify the primary input and output plus optional named
   sources and targets. Stored paths can be overridden by the host or CLI.

Library hosts execute through `cli::RunOptions`, which combines path overrides,
bounded typed runtime parameters, and optional tracing. A successful
`RunOutcome` retains every atomically published file in deterministic
primary-then-extra target order. `RunOptions::with_target` can instead evaluate
one primary or named target; unselected target scopes are not evaluated or
published, while the default all-target mode keeps its collision checks.

Hosts that own transport and persistence can instead use
`cli::PayloadRunOptions`. Each input carries bounded bytes plus a logical path
that selects its format and supplies dynamic-source identity. The runner accepts
named static and dynamic secondary documents and returns bounded serialized
`PayloadArtifact` values in the same target order without touching output
paths. It supports the same explicit target selection, including applying a
logical output-path override to the selected named target. SQLite and
update-existing XLSX stay on the filesystem runner because their behavior
depends on persistent prior state.

The CLI exposes that payload boundary as `ferrule run PROJECT - -`: stdin is
the primary document and stdout receives raw bytes only when exactly one
artifact is produced. The configured source and target paths remain the
logical format identities; named sources remain ordinary local paths. This
mode rejects per-item dynamic named sources and all persistent-state operations
before stdout is written.

The `run --trace-json PATH` option streams interpreter events into a private
sibling staging file and publishes it only after successful execution. Each
JSON Lines record has a versioned envelope, deterministic sequence number, and
a tagged node or scope/control event. Failed mappings leave an existing trace
untouched. See [Execution tracing](tracing.md) for the stable wire contract.

During execution, source contexts form a stack. Field resolution begins at the
innermost frame and falls outward, which allows parent values to broadcast into
nested target rows. Repeating source paths can cross several collection levels;
generated sequences and joins use their own typed iteration contexts. Absent
values remain explicit rather than terminating a run.

## Evaluation and Publication Order

Global failure rules and graph Raise nodes have different ownership and timing.
An ordered failure rule scans its collection before any target evaluation; its
message is evaluated only for the first selected item. Raise is an ordinary
expression node: an unselected lazy branch never raises, while a reached Raise
reports its node identity. Import/export must preserve that distinction rather
than infer equivalence from two similar filter branches.

```mermaid
flowchart TB
    I[Validate project and load inputs] --> G[Scan ordered global failure rules]
    G -->|No selected failure| E[Evaluate selected target scopes and graphs]
    E --> R[Reached Raise or another typed mapping error]
    E -->|Success| T[Complete mapped target instances]
    T --> V[Validate and serialize output boundaries]
    V --> F[Stage filesystem artifacts]
    V --> P[Return bounded payload artifacts]
    F --> C[Publish after the run succeeds]
    G --> X[First selected MappingFailure]
```

Selected-target execution skips unrelated targets, while global rules still
precede target evaluation. Preview returns in-memory results without output
publication. Filesystem and payload hosts have distinct stateful-operation and
budget contracts; this diagram does not promise a transaction over arbitrary
external services or databases. See [Pipelines](mapping-pipelines.md) for
stage ordering and [Memory use](memory-and-limits.md) for materialization.

## Authoring and Compatibility Boundaries

The native editor uses the same project model for primary, named-target, and
function canvases. Compact glyphs keep closed nodes small; full hover identities,
pin semantics, and property/details popovers retain the information needed to
edit them. Layout is adjacent metadata rather than mapping behavior.

Imported repair drafts, strict executable projects, native-compatible exports,
and generated libraries are distinct products of validation. Each has an
explicit admission boundary. The [roadmap](../ROADMAP.md) tracks the remaining
workflow and cross-runtime gates, and [Development](development.md) explains
what evidence closes each gate.

## Workspace Layout

### Core model and execution

- `crates/ir` - format-independent schema, scalar value, and instance trees
- `crates/mapping` - serialized project, graph, scope, join, and format-option
  model
- `crates/functions` - scalar built-in function library
- `crates/engine` - project validation and mapping interpreter

### Code generation

- `crates/codegen` - backend-neutral lowering, validation, and artifact model
- `crates/codegen-runtime` - runtime primitives linked by generated Rust mappings
- `crates/codegen-rust` - deterministic Rust library emitter
- `crates/codegen-csharp` - deterministic C# library emitter

### Format adapters

- `crates/format-xml`, `format-json`, `format-csv`, and `format-xlsx`
- `crates/format-db`
- `crates/format-edi` and `format-flextext`
- `crates/format-protobuf`, `format-pdf`, and `format-xbrl`

See [Supported formats](formats.md) for adapter direction and boundaries.

### Interfaces and interoperability

- `crates/mfd` - `.mfd` import and export
- `crates/cli` - headless validation, filesystem and raw-payload execution,
  host run options and ordered artifact reports, schema import, interop, and
  code generation
- `crates/editor-ui` - shared editor presentation and interaction logic
- `crates/gui` - native egui mapping editor
- `crates/web-demo` - WebAssembly playground built around the real mapping
  engine
- `site` - static project site and web-demo deployment shell

## Design Principles

- Format adapters depend on the shared instance model instead of one another.
- Unsupported imports should preserve useful work and emit an actionable warning.
- Runtime errors remain typed and identify the responsible mapping construct.
- Generated mappings contain static scope and expression functions rather than
  embedding the complete project interpreter.
- Project files remain open and inspectable; pre-1.0 public APIs may evolve as
  invalid states move into stronger types.
