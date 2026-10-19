# #443 authored finite source inventory

All counts below are source accounting, **not observed preparation or execution**.
`source/conformance/basic-owned-group-starting-with/projects.rs::PROFILES`
enumerates 105 unique model keys. The TSV lists every key and applicable routes.
Model ordinals are assigned by first case occurrence during future preparation;
the TSV is not a claim about that resulting order.

| Definitions | Models | Cases | Mapping calls per engine |
|---|---:|---:|---:|
| S01–S24, including three S23 variants | 26 | 27 | 102 |
| X01–X17, including all route/declaration variants | 29 | 30 | 88 |
| E01–E20/E23 functional, plus E22 selection control | 34 | 35 | 129 |
| I01–I08 | 11 | 11 | 38 |
| C02/C03-global/C03-row/C05, plus absent C01 | 4 | 5 | 20 |
| Q02 ordinary islands inside independent siblings | 1 | 1 | 4 |
| Core | **105** | **109** | **381** |
| Separate saved ordinary Starting baseline | 1 | 1 | 2 |

The four additional cases share existing models: `S20-empty` uses S20;
`X10-grouped-shadow` uses X08-P; `C01-absent-optional` uses S01;
`E22-target-before-input-and-policy` uses S01. E22 has only selection ` audit `,
absent named inputs and deliberately invalid limits. C01 has all four routes
with literal distinct expected products, including failure of All at audit.

There are 26 two-call cases (All/Primary), 82 four-call cases
(All/Primary/exact `audit`/exact `unicode`) and the one-call E22 control:
`26*2 + 82*4 + 1 = 381`. The ordinary baseline adds All/Primary, for 383 per
engine and a future target of 1,149 calls across native/Rust/C#. This is not a
passed-call count and does not include runtime probes or refusal calls.

Named targets are intentionally absent for isolated S23, all root-only
X07/X08/X09/X10-ordinary/X11, X14, E17/E18 and I07. The root models have no
RootMarker output. S23 has only H products and no inherited owned target read.
X04 has a noniterating After child after Members. X10 ordinary descendants
inherit positions1..4; grouped members reset1;1;1,2.

E21's three exact-owner failures are structural negative Projects and direct
Programs, not mapping executions. C01 uses S01 catalogues; C04 is covered by
S23-boundary/post and X08/X09-Alias. `catalogues.rs` spells every original
declaration/name/availability entry and all applicable named-target entries.

## Refusals (not mapping calls)

| Source | Negative Projects | Direct Programs |
|---|---:|---:|
| closed.rs | 18 | 21 |
| closed_specialized.rs | 22 | 22 |
| closed_matrix.rs | 25 | 18 |
| Total | **65** | **61** |

The matrix expands each of Aggregate, Lookup, CollectionFind, SequenceExists
and SequenceAggregate from target into boundary/post/under-ItemAt/mixed-sibling
Project forms; direct Programs have target/under-ItemAt in closed.rs and the
other three phases in closed_matrix.rs. The supplement provides the 21 paired
specialized recipes, Project-only owned document output and direct-only uncalled
isolated body. Complete expected diagnostic objects are literal Rust constructors.
Structural contradictory-group/post-without-group/source-free states are Project
controls; IR's single grouping discriminant cannot encode conflicting modes.

The additional public Projects cover a generated member, exact dynamic owner,
FilterMap ItemAt and owned-failure predicate/message. Their direct counterparts
remain in closed.rs. No negative definition is disguised as a successful model.

Q01 is a separate **50-surface** old-family matrix: 8 lowerers, 16 common
validator/emission-validator calls, 8 catalogues and 9 emitters per language
(default/Basic have distinct emitter entries but share default lowering).
`old_profiles.rs` includes the old AdjacentBy family explicitly. Compare every
product to the complete typed Grouping diagnostic returned by grouping_error().
New companion direct negatives require both validators, catalogue and both
emitters, retaining all originals and testing input immutability.

R01–R16 stay separate component recipes in `RUNTIME-PROBES.md`; zero runtime probes
are implemented or executed by this packet. Prior qualified By/Adjacent/ItemAt
corpora and their historical manifests are immutable external conservation lanes,
not cases silently counted here.
