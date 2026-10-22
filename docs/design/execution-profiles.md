# Trusted and restricted execution profiles

Ferrule's execution host chooses one of two profiles for each run:
**trusted**, the default, or **restricted**, where the host supplies a
`RestrictedPolicy`. This contract states what authority each profile grants,
which acquisition routes the restricted policy governs, how its budgets are
counted, and the typed refusal for each case.

A restricted profile **enforces a policy inside the ferrule process. It is not a
sandbox.** Mapping evaluation, format readers and writers, and the HTTP client
keep running in the host's process with the host's operating-system privileges.
A host that needs to contain hostile mappings or inputs must also run ferrule
under an operating-system isolation mechanism, such as a container, a separate
user account or a seccomp/AppContainer profile.

## Profiles

| | Trusted (default) | Restricted |
| --- | --- | --- |
| Filesystem | Any path the process can open, including dynamic absolute paths | Only beneath host-listed roots, by access mode |
| HTTP GET | Any `http`/`https` origin; up to five redirects, never downgrading HTTPS | Only host-listed origins; redirects refused |
| Execution budgets | None beyond the existing per-feature bounds | Explicit work, depth and join-row budgets |
| Typed failures | Unchanged | Plus `PolicyRefusal` and `ExecutionBudgetExceeded` |

Trusted is `ExecutionPolicy::Trusted`. It is what every API, CLI command and the
GUI use when no policy is supplied. A trusted run behaves exactly as it did
before profiles existed: it performs no policy checks and keeps no budget state,
and it produces the same values, errors, trace events and debug callbacks.

Restricted is `ExecutionPolicy::Restricted(RestrictedPolicy)`. Its access lists
deny by default: a policy without roots allows no filesystem access, and one
without origins allows no HTTP request. All three budgets must be set
explicitly; none is inferred.

### Neighbouring mechanisms that are not execution profiles

- **Imported-resource confinement.** Package roots, `ferrule-package.json`
  manifests and EDI/JSON catalog roots confine where `import-mfd` and its
  importers *look up* design resources. That is import-time resource lookup. It
  does not restrict what a mapping reads or writes when it runs, and confining
  imports does not make a run sandboxed.
- **Per-feature bounds.** Format byte limits, generated-sequence and regex caps,
  filter/map item and work ceilings, user-function nesting and the recursive
  filter/tree depth limits bound individual features. They stay in force in
  both profiles, but they are not universal limits. A small graph whose nodes
  share inputs can still require exponentially many evaluations without
  touching any of them, because the interpreter re-evaluates a shared node for
  each consumer instead of memoizing it. The restricted budgets close that gap.

## Acquisition rights governed by the restricted policy

The policy governs every location a run reads or writes for the mapping. That
includes locations stored in the project, locations the mapping graph computes,
and locations supplied as run overrides such as `--input` and `--output`. A host
that wants to grant such a path lists a root that contains it.

| Route | Right checked | Refusal |
| --- | --- | --- |
| Primary input file (`source_path` or override) | read | `PathOutsideRoots`, `UnresolvablePath` |
| Static named source file | read | as above |
| Dynamic named source (graph-computed path per driver item) | read, after resolving relative paths against the project or pipeline directory | `EngineError::DynamicSourceRefused` wrapping the refusal |
| Local XML file set | read, for the set's base directory; the reader already confines members to the canonical base | as above |
| HTTP(S) GET XML source (stored, override or dynamic) | origin | `HttpOriginDenied`, `InvalidHttpUrl` |
| HTTP redirect response | unsupported in restricted runs | `UnsupportedCapability(HttpRedirect)` |
| Static primary or named target file, including SQLite and update-existing XLSX targets | write | `PathOutsideRoots`, `UnresolvablePath` |
| Dynamic per-document target files | write, for every final document path | as above |
| Pipeline host inputs, stage dynamic sources and selected publications | read, read, write | as above |
| Standard-stream run (`ferrule run PROJECT - -`) | read, for a primary input file and every named static input file | as above |

Rights not listed here are not granted. A restricted run therefore refuses with
`UnsupportedCapability(UnconfinedDynamicSourceLoader)` if it reaches a dynamic
source through a library host loader that does not implement
`DynamicSourceLoader::load_restricted`. The engine cannot check where such a
loader reads, so it refuses before calling it.

The payload API (`run_project_value_payloads`, the GUI preview and pipeline
payload preview) acquires nothing itself: every document is host-owned bytes
identified by a logical path. Under a restricted policy only the budgets apply
there. Payload dynamic sources resolve logical identities among the documents
the host supplied.

The policy does not govern locations the host selects for itself rather than
for the mapping: the project or pipeline file, the policy file, the
`--trace-json` destination, the PDF worker executable and its evidence
directories, and stage mapping paths that are exposed only as runtime values.
`run-rest-json` keeps its separate explicit grant (`--allow-live-rest` and a
host-owned request file) and does not accept a restricted policy. Generated
Rust and C# mappings run in their own hosts, which own their loader boundaries.

## Filesystem rules

A root is an existing directory, canonicalized when the policy is built, with an
access mode:

- `read` admits reads beneath the root.
- `read_write` admits reads and writes beneath the root.

A location is beneath a root when its resolved form equals the root or extends
it by whole path components, so `/data/in` does not admit `/data/input`.

- **Reads.** The location is made absolute against the process working
  directory, then fully canonicalized, resolving every symlink, `.` and `..`.
  A location that cannot be canonicalized, including one that does not exist,
  fails with `UnresolvablePath` because the policy cannot prove where it
  points. A canonical location outside every root fails with
  `PathOutsideRoots`. An absolute dynamic path outside the roots, a relative
  path whose `..` components leave them, and a symlink inside a root that
  points outside it are all refused by this rule.
- **Writes.** The longest existing ancestor is canonicalized and the remaining,
  not-yet-existing components are appended. The remaining components must be
  plain names. A `..` after a missing directory fails with `UnresolvablePath`.
  The result must be beneath a `read_write` root. Existing protections still
  apply as well: dynamic documents stay confined beneath their output base,
  and symlinked destinations and components are still refused.

Checks run when the location is resolved, immediately before acquisition.
Locations known before evaluation are also checked while the route is
prepared, before the first input is acquired. A refused location is never
opened or created. Because this is policy
enforcement rather than isolation, a process that can change the filesystem
concurrently can still race a check against the later open (TOCTOU). Hosts that
face such writers must isolate the filesystem itself.

## HTTP rules

An origin is the triple *(scheme, host, port)*: the scheme is `http` or `https`
(case-insensitive), the host is compared in ASCII lowercase (IPv6 literals stay
bracketed) and the port is explicit or the scheme default (80 or 443). An
origin is admitted only when the policy lists that exact triple. There are no
wildcards and no subdomain matching, and `http://host` and `https://host` are
different origins. URLs with user-info, an empty host, an invalid port or
characters outside the URL authority grammar fail with `InvalidHttpUrl` before
any request is made. Policy entries use the form `scheme://host[:port]` with at
most a trailing `/`.

A restricted request follows no redirects. The first response comes from an
admitted origin, but a 3xx response would hand acquisition to a location the
policy has not checked. Such a response therefore fails with
`UnsupportedCapability(HttpRedirect)` without being parsed. The existing
timeout, header, body and UTF-8 bounds apply unchanged.

## Budgets

`ExecutionBudgets::new(work, depth, join_rows)` sets three independent limits.
Each limit admits exactly that many units, and the next unit fails with
`EngineError::ExecutionBudgetExceeded { kind, limit }`. A limit of zero admits
nothing of its kind. Counters belong to one engine evaluation: one project run,
counting failure rules, every evaluated target and dynamic-path expressions.
Each pipeline stage is a separate evaluation with fresh counters.

| Budget | Counting point | One unit is |
| --- | --- | --- |
| `work` | Entry of every graph-node evaluation in the main graph and in user-function bodies | One evaluation attempt of one node in one context |
| `depth` | The same entry points, released when the node returns | One level of simultaneously active node evaluations |
| `join_rows` | Each inner-join stage, for every left-row and right-row pairing it examines | One candidate tuple, whether or not its keys match |

- **Work** is charged before the node evaluates. A node reached again through
  another consumer, item or context is charged again, so the counter measures
  the interpreter's real work. A shared DAG of *n* levels in which each node
  reads the one below twice costs 2^(n+1) − 1 units, and the work budget cuts
  that recursion off at its limit. The interpreter never memoizes by `NodeId`,
  because a node's value depends on its frame, position and dynamic context and
  because memoizing would change trace multiplicity.
- **Depth** bounds nesting across graph inputs, call arguments and user-function
  bodies. This bounds native stack use on long dependency chains, independently
  of total work. A check that would exceed both budgets reports `Work`.
- **Join rows** accumulate across every join execution in the evaluation: join
  scope iterations, repeated per-item joins and `JoinAggregate`. Counting
  candidates rather than matches bounds both key comparisons and materialized
  tuples.

Budget failures are never wrapped in the filter/map runtime envelope, so a
host always sees the typed budget error.

## Precedence, laziness and observation

- Checks and charges happen where the trusted interpreter already acquires or
  evaluates, in the same order. Lazy evaluation is unchanged: an unselected
  `If` branch or an unreached failure message is never charged, and a dynamic
  path that evaluates to Null acquires and checks nothing. An error raised earlier in evaluation order
  takes precedence over a later budget or policy failure.
- A node refused by a budget does not evaluate. It produces no value trace and
  no input or value debug callback. The debug failure callback reports the
  budget error at that node, as it does for any other node failure. Nodes that
  ran keep the trace multiplicity they have in a trusted run.
- Output destinations and input locations known before evaluation are checked
  before any input is read, in that order. Each input location is checked again
  immediately before it is read, and each dynamic source is checked per driver
  item immediately before loading. Dynamic
  documents are checked before any output is staged. A refused run publishes
  no outputs.

## Host API

Engine (`engine` crate):

- `ExecutionPolicy::{Trusted, Restricted}`, `RestrictedPolicy::new(budgets)`
  with `allow_root(path, RootAccess)` and `allow_origin(origin)`,
  `ExecutionBudgets`, `ExecutionBudgetKind`, `PathAccess`, `RootAccess`,
  `RestrictedCapability`, `PolicyRefusal` and `PolicyConfigError`.
- `ExecutionPolicy::authorize_read`, `authorize_write` and `authorize_url`, so
  other hosts can apply the same rules at their own acquisition points.
- `ExecutionContext::with_policy(&policy)`.
- `DynamicSourceLoader::load_restricted`, which policy-aware hosts implement.
  Its default refuses with `UnconfinedDynamicSourceLoader`. The engine calls
  its tagged form, `load_restricted_tagged`, whose default adapts it. A refusal
  becomes `EngineError::DynamicSourceRefused`, whose `DynamicSourceRefusal`
  holds the source name, logical path and typed refusal. `DynamicSourceDiagnostic` projects it as a host acquisition failure, as
  generated libraries see a confining host's refusal (see
  [interpreter host loaders](../code-generation.md#interpreter-host-loaders)).

CLI library (`cli` crate): the `execution_policy` field or builder on
`RunOptions`, `StandardIoRunOptions`, `PipelineRunOptions` (shared by pipeline
preview) and `PayloadRunOptions`, and `load_execution_policy_file`. CLI
failures keep the typed `PolicyRefusal` or `EngineError` as a downcastable
cause in the `anyhow` chain.

Command line: `ferrule run --restricted-policy FILE` (filesystem and
standard-stream forms) and `ferrule run-pipeline --restricted-policy FILE`.
Without the flag, both commands run trusted.

```json
{
  "version": 1,
  "filesystem": {
    "read": ["inputs"],
    "read_write": ["/srv/ferrule/out"]
  },
  "http_origins": ["https://data.example.com", "http://127.0.0.1:8080"],
  "budgets": { "work": 1000000, "depth": 512, "join_rows": 1000000 }
}
```

Unknown keys are refused. `filesystem` and `http_origins` may be omitted, which
denies those rights. `version` and all three budgets are required. Relative
roots resolve from the policy file's directory.

## Out of scope

- Process, memory, CPU or wall-time isolation, and syscall or network
  filtering.
- PDF worker sandboxing and its policy (#104, #326, #328). A restricted run
  still checks a PDF input's path before the in-process reader or the worker
  receives it.
- Offline bundle construction and verification (#105). Bundles package
  resources and are not an execution sandbox.
- Restricting the GUI, `run-rest-json`, code generation or generated
  mappings.
