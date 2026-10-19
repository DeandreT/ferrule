# #443 owned starting-boundary grouping — finite engineering contract

**Status: source-only draft for independent review, not implementation authorization or qualification.** The parent owns review and sequencing after #442 merges. All unqualified requirements below describe future work, not completed behavior. See `MINIMAL-CONTROLS.md` for concrete public signatures/private dispatch, `IMPLEMENTATION-SCOPE.md` for file ownership and conservation, and `DEPENDENCY-SOURCE-FENCE.md` for exact inspected inputs.

## 1. Authority and prerequisite boundary

- Governing issue: https://github.com/DeandreT/ferrule/issues/443, “Add owned starting-boundary grouping”, dependency #442. Its body says to open **only group_starting_with with existing first-item and boundary semantics**. Thus `_with_basic_owned_group_starting_with` follows both the existing field spelling and the established #441/#442 companion family. The issue does not prescribe a public runtime controls type.
- Observed checkout HEAD `7769fce8009bc17be091980cb7de12f11f7722c4`, branch `feat/basic-owned-group-adjacent-by`, contains preexisting modified/untracked #442 implementation and evidence sources. #442 was OPEN and no PR for that branch was returned. Parent reports component qualification only; this session did not audit or promote it to full three-engine acceptance. The reviewed #442 contract is an input, **not proof that its current implementation is accepted or merged**.
- #473 was CLOSED at read. Its four-producer ItemAt consumer permission is independently specified in `docs/design/owned-sequence-item-at.md` and composed orthogonally by the current #442 companion. Bind future implementation to the exact merged, independently accepted #442 source/evidence and rebase this draft's anchors/hashes before coding. Do not pretend this dirty working tree is an accepted base.
- Only new owned control: StartingWith, alongside previously qualified By/AdjacentBy and existing Basic scalar/filter/sort/window/primary-row-failure behavior. No new Project/owner/path/GroupingPlan serialization fields. `Scope.group_starting_with`, `GroupingPlan::StartingWith { predicate }`, and `GroupingExpressionRole::StartingPredicate` already exist.
- Owned EndingWith/IntoBlocks and all unqualified producer/consumer/specialized contexts remain closed. Ordinary legacy-only grouping islands retain their existing routes. This draft neither changes nor claims unrelated authoring, export, adapter, observer or product capabilities.

## 2. Existing ordinary semantics are the specification

### 2.1 Boundary membership, including the initial false prefix

Evidence, not an invented choice:

- Native ordinary `crates/engine/src/eval_scope.rs:472-585`: for each selected candidate, evaluate Starting predicate with `passes_scope_filter(... GroupStarting ...)`; evaluate independent post predicate; append to last constructed group only if `starts_group == false`; otherwise start a new group. With no last group, false also starts the first group.
- Rust ordinary `crates/codegen-runtime/src/context/grouping.rs:133-182`: identical `!starts_group && last_mut()` rule, separately OR-ing retention.
- C# ordinary `runtime/csharp/Ferrule.Runtime/FerruleGrouping.cs:77-107`: `groups.Count == 0 || starts` starts a group, after boundary then post callbacks.
- Existing native `tests/group_starting.rs:140-178` fixes filtered L/H/L/H/H/L to `[A,C] [D] [E,F]`, excluding a filtered-out H. `:194-233` fixes independent any-member post retention, including a false first member.

Required owned rule: every true candidate **belongs to the new group it starts**, never the old group; no empty group is emitted before an initial true. Consecutive trues create consecutive singleton starts. The leading false prefix is one real group; all-false nonempty input is one group; empty input is zero groups. The first predicate must still be evaluated even though the first candidate necessarily starts a group. No “skip predicate for first item” optimization.

After selection, partition by boundaries, not equality of Bool keys. `[false,false,true,true,false]` means `[1,2] [3] [4,5]`, not two Boolean buckets or adjacent Bool runs. There is no grouping key, comparison, hashing, numeric/string coercion or key text to charge for a successful Boolean boundary.

### 2.2 Post retention and preserved run boundaries

Post is candidate/member retention, not a group-context predicate. Evaluate after the boundary, once for every reached selected member. OR results into the **constructed** run; retain that whole run if any succeeds, including its false first member and all false followers. An earlier true never suppresses later post callbacks/errors. An all-false run is removed only after complete partitioning. Never join its neighboring runs afterward, even if the removed run was a boundary-only singleton. Selection filtering happens earlier and can remove a would-be boundary; that legitimately changes partitioning.

### 2.3 Exact phase order

Preserve the paired #442 pipeline; ordinary helpers are semantic references, not owned bridges:

1. Preserve exact target/declaration/input validation and global failure precedence. Select/walk the complete original authenticated cohort. Grouped preflight validates registry, receiving-parent identity, complete original witness/IDs, terminal provenance, full canonical/raw selectors, known storage and metadata before **semantic** callbacks. Empty/missing/null selection is not an admission bypass.
2. With actual sorting and FilterThenSort, run **all** selection predicates in structural order, then sort keys for surviving candidates, candidate-major/key-major. Otherwise all sort keys precede the remaining selection phase. Without sorting, FilterThenSort does **not** create an eager all-predicates phase.
3. Stable-sort with the existing comparator. Sort callbacks see structural terminal positions. Ranked candidate views change only the selected terminal position to sorted rank, retaining ancestor/foreign-owner positions and payload identity.
4. Evaluate every explicit window bound once in stored order in the **receiving parent**, after sorting and before any boundary. FromTo evaluates first then last. This includes empty/all-filtered input and an earlier empty window. Append existing implicit First(1) without a callback.
5. Visit candidates in selected order: remaining selection predicate; if true, **boundary predicate**, then post predicate; append/start last constructed run; advance. Boundary/post see structural positions without sorting or selected ranks with sorting, including filter-created gaps, never final group ordinals. An earlier boundary/post error beats a later candidate's selection error. All FilterThenSort predicates instead precede sort/bounds/boundary/post.
6. Drop all-false runs, then apply already evaluated windows sequentially to the **retained groups**, not source rows or members. Compact final group ordinals from one. Only included groups evaluate bindings/children. Nested member iteration enumerates just the run, in selected order, with independent one-based member positions.

“Bounds before grouping” is **evaluation**, not application. First(0), implicit First and a first successful post do not hide later boundary/post errors. Bounds/controls finish before group targets. Dropped/window-excluded target bindings remain lazy. Preserve original callback error objects/nodes/messages and NotABool/ItemCount errors, not generic grouping errors. No partial result tree is returned; callback side effects are not transactional.

Source distinction to preserve: current Rust original-witness validation precedes metadata sampling, but private malformed-terminal validation in `prepare` follows it (`basic_group_adjacent_by_tests.rs:193-243` expects metadata1 for that corruption). Current C# validates terminals in Seed before metadata. Both refuse before semantic callbacks; do not invent identical accessor counts for their different private frontiers.

## 3. Owner, root, prefix and member contexts

The private container carries authenticated exact owner O and absolute endpoint-local collection C, independent of names, equal payloads or wrapper spelling. Preserve incoming ancestor/foreign-owner frames and actual observed prefix cuts. Only the paired source-walk observation establishes provenance, including identity/nonrepeating roots and legacy aliases. No `GroupedItems`/ordinary `ScopeContext` conversion, representative pointer guessing or alias inference is an owned bridge.

- Unpinned or C-pinned scalar reads of the group use its **first surviving selected member**, whether its boundary or post was false. Selecting C again enumerates the run's complete members; ordinary active Item selection retains its existing identity shortcut. A new ordinary member occurrence shadows its enclosing group at O/C. Foreign owners remain distinct.
- Preserve the first member's **intermediate source prefix**, not a separate original parent stack for each later member. A flattened run spanning Batches X and Y retains first Batch X; nested members from Y still see X for the pinned Batch context. This limitation is already explicit in `docs/design/source-ownership.md:24-29`, `owned-group-by.md:36-42`, and #442's reviewed contract.
- Raw legacy wrapper identity and canonical owned C stay separate. Legacy `[Alias]` over a named repeated endpoint can create an Alias wrapper with canonical C=`[]`. Owned references never prepend Alias. Actual wrapper redirects survive nested legacy member iteration.
- **Explicit `frame=[]`:** choose the latest exact-owner occurrence at exact collection `[]`. If it is the authenticated synthetic member container at C=`[]`, read the immutable endpoint root, not the representative. A repeated root therefore raises strict uniterated-repetition, even with a nonrepeating schema declaration. A newer ordinary Item at `[]` instead has ordinary active-member pin behavior. Another owner's newer Item cannot shadow this owner's root group.
- A group at nonempty C alone grants no root-pin exception. Existing declared-nonrepeating **and actual nonrepeated** root bypass stays independently valid. Without that bypass or an exact active `[]` frame, empty pin stays inactive. Resolve exact owner availability first; never substitute colliding Primary/foreign data. Ordinary descendant repetitions below a representative stay strict.

The current private name `KeyedGroup` is accurate for #442 but not StartingWith. Rename only private markers/origins to honest **GroupMembers** (and GroupWrapper) and restrict minting to the explicit three admitted operations. Do not introduce “any grouping” permission. Detailed rename/copy/minting inventory is in `MINIMAL-CONTROLS.md`; root/representative/member rules must survive every copy/rank/bridge path. There is no need for a new public context-kind field or a serialized mode.

## 4. Narrow profile and orthogonal consumer policy

Add the six public companion functions listed in `MINIMAL-CONTROLS.md`. The new profile admits static Source/OwnedSource Group/Scalar scopes using **By OR AdjacentBy OR StartingWith** plus existing controls; Repeated or existing Group-only First output. Default, Basic/filter/sort/window/row-failure, By, ItemAt and AdjacentBy companions keep their existing typed gates. In particular old AdjacentBy and old ItemAt remain closed to owned StartingWith; old By remains closed to owned ItemAt and AdjacentBy. Neither runtime primitive presence nor a renamed private marker widens an old emitter.

Pair the new target-control profile with existing `SequenceItemAtPolicy::BasicScalarProducers` and the separately unchanged primary-row-failure subset. ItemAt supports **Generate, Tokenize, TokenizeByLength, TokenizeRegex only**, including nested eligible ItemAt in parent/candidate/group/member scalar contexts. Producer ordered arguments finish/materialize first, then index once in the same receiving context even if production is empty/null-suppressed. Producer errors suppress index. Generate is inclusive unit-step `from..=to`, omitted start defaults to one; no count/start/step fields. ItemAt creates no owned generated-item frame. Its lexical item declaration is not endpoint provenance.

Inherited closed contexts dominate both permissions: generated-source iteration, RecursiveCollect/FilterMap consumer producers, SequenceExists/SequenceAggregate, aggregate/lookup/find, isolated function, join, dynamic owner/loader, document/XML/tree, concatenation, CopyCurrentSource/dynamic construction and unqualified grouped-observer snapshots do not become available. An eligible ItemAt in one argument does not open a closed sibling argument. Owned failure predicates/messages still do **not** gain ItemAt, boundary iteration or other target permissions. The independently qualified root/primary-row global failure subset and pre-target ordering remain unchanged.

Named admission follows actual phases: bounds in parent; filter/sort/boundary/post in candidate; bindings/children in group/member; ItemAt arguments/index in their receiver. Preserve full namespace/context/ancestry/closure/node visit keys when the same NodeId is used in different phases. Owned-only named reads remain Optional until reached. Potential legacy aliases remain RequiredLegacy, including both lazy If branches; unavailable dynamic declarations remain ReservedUnavailable. Retain original ordinals and complete All/Primary/exact-named catalogues; global failure dependencies occur in all of them. Invalid exact target selection still precedes supplied-input/policy work.

## 5. Storage, exact resource formulas and error frontiers

**Selected minimal layout:** keep #442's actual fixed arrays/templates exactly, including n Value group-key cells preinitialized to Null but unused for Starting. There is no boundary vector, stringification, new map/list/ticket, adapter heap object or per-run budget. A Boolean boundary lives only in a local scalar; a private three-case dispatch selects the operation. The n-key reservation is real retained backing, not a fabricated Boolean text charge. Removing it is a different layout requiring a new proof/thresholds; this contract does not do that.

Proof: n original candidates yield at most n nonempty runs; their disjoint member chains sum to at most n; each output claims its first candidate's unique one-shot prefix template. Thus existing n buckets/keys/links/plans/templates/outputs and worst-case singleton member/wrapper storage suffice. Reuse only if the implementation receipt verifies unchanged backing/record layouts, capacities, known path copies and setup records. A new retained C# adapter object, field array or replacement backing invalidates the proof and must be enumerated/reviewed before acceptance. C# must publish distinct immutable contexts/member arrays; a retained earlier callback context never changes when a later run starts. Rust retains the higher-ranked borrowed-local-payload target returning an owned Instance.

All arithmetic below is checked in the existing evaluation order before corresponding allocation. Element-size/native Vec and C# Array.MaxLength representability checks remain additional prerequisites. Policy: roots R, frames F, canonical path segments P, UTF-8 metadata B, walk contexts W. Derived ceilings remain **unchanged**, representation-specific:

```text
Q=P+1; L=2F+2P+4; J=FP+FQ+LP
H_R=2+F+2L+J; H_C=7+3F+2L+J
M_R=E_R+2W*H_R+9W+3
M_C=E_C+W*(H_C+(H_C-1))+14W+5 = E_C+2W*H_C+13W+5
```

For completeness the existing retained-phase ceilings E are closed, not new free parameters. At K=3W, A=F+P+1, N=K+1, D=2P+2, T=2(F+1), AF=KF+2F:

```text
C=2R+1+AF+AFP+T+K+D
m=2NL+NLP+NF+NF(P+1)+5R+2A(L+F+2)(P+2)+2N
E_R=C+m+D+W+5
E_C=(C+2K+F+FP+T)+3m+D+(K+1+4N+A+4)+2W+5
```

These are the algebraic forms of Rust `paired_bounds` at 3W plus W+5 (`basic.rs:91-120,1564-1573`, `mod.rs:778-818`), and C# `BasicSourceAccounting.Maximum(limits,true)` (`ScopeContext.cs:2222-2243`). Preserve their checked intermediate products, not only an algebraically equivalent unchecked final sum.

For actual n<=W, k sort keys, h windows including implicit First, s=1 iff sorted, a wrapped templates:

```text
fixed_R = k+nk+3h+(7+s)n+a+3 + sum(T_R) + s*sum(U_R)
fixed_C = k+nk+3h+(9+s)n+4a+3 + sum(T_C) + s*sum(U_C)
T_R=2+(o+1)+(l+1+w)+(q+1+w)+j
T_C=7+(o+1)+(l+1+w)+(q+1+w)+2(t+1)+j
U_R=2+o'+l'+q'+j'
U_C=6+o'+l'+q'+2t'+j'
```

o/l/q/t are prefix owned/legacy/origin/collection counts; w is wrapper flag; j counts every copied path cell. Primed counts are the entire candidate used for a sorted view, not just its prefix. T includes its template/context identity records; C# collection associations cost both references and objects. Add existing root/parent/full-cohort seed, **retained spare capacity** and C# two separately admitted ledger setup records exactly once. Direction snapshot k, sort cache nk, windows3h, order/next/bucket/key/output5n, merge sn, plans n, payload R n+a / C# 3n+4a, templates/sorted views are the exhaustive retained inventory. No reset per group.

Known metadata text = registry/parent/cohort seed text + every actual path/name copy in templates/sorted views. Preflight slot refusal wins over known-text refusal, both before semantics. Sort string return frontier remains callback success -> count UTF-8 -> cumulative admit -> cache/compare. **Starting boundary returns bool, adds zero group-key UTF-8 bytes and performs no group-key comparison/storage.** Its internal expression may produce temporary text or fail RequireBoolean, but that is not a successful returned string key. Preserve callback/NotABool errors before post; do not assign a fictitious group-key resource frontier.

Existing By/Adjacent calls retain every-returned-string-key charges including duplicates and their original before-post frontier. Native has its own WorkBudget and retained-key accounting; Starting adds no key string there either. Metadata caps do not bound source/output payload copies, sequence materialization, RSS or CPU. Functional errors compare all three with nonbinding metadata limits; numeric resource refusals use independent per-runtime counts.

## 6. Independent finite genuine-Project catalogue

These are **literal unexecuted authoring recipes**, not generated test results. Build each as a real Project with valid schemas/nodes/declarations, save its complete input/expected tree or typed error before invoking any engine. Expected constructors may construct only the explicitly listed fields/members; they may not partition input, call any evaluator, copy an actual, infer expected groups with another mode, or strip diagnostic information. Compare lossless complete trees (ordered fields, scalar tags/float bits, Unknown group XML origin) and complete native errors before a closed portable diagnostic projection. Each listed variant is a separate finite case.

### 6.1 Literal data and expected-tree notation

Owners: Primary P; exact static N=`Rows`; exact static U=` Røws💠 `; unused unavailable dynamic D. Every endpoint has its own immutable Group root with fields Marker:String, Limit:Int, Rows:Repeated(row) in that order. P.Marker=`P.root`, Limit2; N.Marker=`N.root`, Limit1; U.Marker=`U.root`, Limit2. P also has actual root Rows, colliding with named owner N's spelling. Rows have ordered fields Start:Bool, Label:String, Keep:Bool, Rank:Int, Limit:Int0, Key:String. Data:

```text
P: label  start keep  rank key
   p0     F     F      30  A
   p1     F     T      10  B
   p2     T     F      20  A
   p3     T     F      25  A
   p4     F     T      40  C
   p5     T     F      50  A
   p6     F     T      60  A
N: n0 T F 300 A; n1 F T 100 B; n2 F F 200 A;
   n3 T F 250 A; n4 F T 400 C; n5 F F 500 A; n6 T T 600 A
U: u0 F T 7 A
```

F/T in input means Bool false/true, not strings. Each row label is unique. Sort ties variants set listed Rank values explicitly. In fresh-data cases, shorthand `x:F` (or `x:T`) expands to the complete ordered row `{Start:Bool(false),Label:String(x),Keep:Bool(true),Rank:Int0,Limit:Int0,Key:String(A)}`, changing only Start for T. Thus fresh labels a/b/c/d/x1/y1 do not inherit unspecified values from an actual result. Repeated-root cases explicitly omit RootMarker targets; their root schema/data are the stated row fields rather than the base Marker/Limit/Rows wrapper.

`M(p,x)` = Group ordered `{Label:String(x), MemberPosition:Int(p), LegacyPosition:Int(p)}`. `G(o,f,m,[x,y])` = Group ordered `{First:String(f), GroupPosition:Int(o), RootMarker:String(m), Members:Repeated([M(1,x),M(2,y)])}`; longer explicit member lists use the literally listed sequential positions. `O([G...])` = Group `{Groups:Repeated([...])}`. This finite constructor is not a grouping algorithm. RootMarker uses explicit empty-frame pin at the named/primary nonrepeating root; First uses owned Rows/Label. `G*` is G without RootMarker, used for repeated endpoint roots. Every Group has Unknown XML origin. `[]` is a real empty Repeated; no omitted/null conflation.

```text
P4=[G(1,p0,P.root,[p0,p1]),G(2,p2,P.root,[p2]),
    G(3,p3,P.root,[p3,p4]),G(4,p5,P.root,[p5,p6])]
N3=[G(1,n0,N.root,[n0,n1,n2]),G(2,n3,N.root,[n3,n4,n5]),
    G(3,n6,N.root,[n6])]
U1=[G(1,u0,U.root,[u0])]
```

### 6.2 Boundary/order/window/post cases

Each expected list is inside O unless stated otherwise.

| ID | Genuine Project variation | Literal expected list |
|---|---|---|
| S01 | Owned P Rows, Starting=Start | P4. Initial false prefix is preserved. |
| S02 | Exact N Rows, same-spelling P.Rows | N3, never P/U labels. |
| S03 | Exact U Rows, preserve whitespace/Unicode | U1. |
| S04 | Starting always false | `[G(1,p0,P.root,[p0,p1,p2,p3,p4,p5,p6])]`. |
| S05 | Starting always true | `[G(1,p0,P.root,[p0]),G(2,p1,P.root,[p1]),G(3,p2,P.root,[p2]),G(4,p3,P.root,[p3]),G(5,p4,P.root,[p4]),G(6,p5,P.root,[p5]),G(7,p6,P.root,[p6])]`. |
| S06 | P rows restricted to first true p2 followed by true p3 then false p4 | `[G(1,p2,P.root,[p2]),G(2,p3,P.root,[p3,p4])]`; no leading empty group. |
| S07 | Selection excludes only p2,p3,p5 | `[G(1,p0,P.root,[p0,p1,p4,p6])]`; removed boundaries no longer split. |
| S08 | Post=Keep | `[G(1,p0,P.root,[p0,p1]),G(2,p3,P.root,[p3,p4]),G(3,p5,P.root,[p5,p6])]`; dropping p2 never merges runs. |
| S09 | Post always false | `[]`; all seven boundaries/posts still evaluate, no target. |
| S10 | Stable sort Rank ascending | `[G(1,p1,P.root,[p1]),G(2,p2,P.root,[p2]),G(3,p3,P.root,[p3,p0,p4]),G(4,p5,P.root,[p5,p6])]`. |
| S11 | S10 + Position>2, FilterThenSort | `[G(1,p2,P.root,[p2]),G(2,p3,P.root,[p3,p4]),G(3,p5,P.root,[p5,p6])]`; filter sees raw3..7. |
| S12 | S10 + Position>2, SortThenFilter | `[G(1,p3,P.root,[p3,p0,p4]),G(2,p5,P.root,[p5,p6])]`; selected ranks3..7. |
| S13 | S12 + post Position==4 | `[G(1,p3,P.root,[p3,p0,p4])]`; false first p3 retained, selected3/4/5 become member1/2/3. |
| S14 | Sorted Rank ascending, Starting=(Position<=3) | `[G(1,p1,P.root,[p1]),G(2,p2,P.root,[p2]),G(3,p3,P.root,[p3,p0,p4,p5,p6])]`; consecutive true is NOT adjacent Bool grouping. |
| S15 | Sort key owned Position(Rows) descending | `[G(1,p6,P.root,[p6]),G(2,p5,P.root,[p5,p4]),G(3,p3,P.root,[p3]),G(4,p2,P.root,[p2,p1,p0])]`. |
| S16 | S10 + First(parent P Limit2), each row Limit0 | First two S10 groups, ordinals1/2, members `[p1] [p2]`; no bound from row. |
| S17 | P4 + SkipFirst(1),Last(2) | `[G(1,p3,P.root,[p3,p4]),G(2,p5,P.root,[p5,p6])]`. |
| S18 | P4 + Last(2),SkipFirst(1) | `[G(1,p5,P.root,[p5,p6])]`. |
| S19 | P4 + FromTo(2,3) | `[G(1,p2,P.root,[p2]),G(2,p3,P.root,[p3,p4])]`. |
| S20 | Group-only implicit First, nonrepeating Group target | Bare `G(1,p0,P.root,[p0,p1])`, not Repeated. Empty source variant: bare empty Group. |
| S21 | First(0), target Raise | O([]); successful candidate controls still all run. |
| S22 | Null-pruned owned Rows `[a:F,Null,b:F,JsonNull,c:T,d:F]` | `[G(1,a,P.root,[a,b]),G(2,c,P.root,[c,d])]`; structural boundary positions1,3,5,6. |
| S23 | Legacy Source(Rows), with exactly one control introducing owned demand; use the isolated legacy-only targets defined below | The four explicitly listed legacy products below for both variants; named Rows remains RequiredLegacy; descendants remain paired. |
| S24 | Tie variant all P Rank0, stable ascending | P4 exactly. |

S23 uses a separate target recipe, not G or P4: ordered Group `{Groups:Repeated([H...])}`, where H is ordered Group `{First:String, GroupPosition:Int, Members:Repeated([Group{Label:String,MemberPosition:Int}...])}`. Every target expression is legacy: First/Label are SourceField(path=[Label],frame=[Rows]); GroupPosition/MemberPosition are Position(collection=[Rows]); the member child uses legacy inherited Source([]). There are no OwnedSourceField/OwnedPosition nodes in target bindings, member bindings, selectors, sort/filter/windows, failure rules or called functions. All Groups retain Unknown XML origin.

The boundary-only variant has only owned P Rows/Start as its Starting predicate and no post predicate. The post-only variant instead reads legacy SourceField(path=[Start],frame=[Rows]) for Starting; its post predicate is `equal(owned P Rows/Keep, same owned node)`, which returns true for every literal Boolean input while retaining that node as the sole ownership demand. Neither variant uses the base G constructor's owned First or RootMarker fields. The complete H products are `(p0,1,[(p0,1),(p1,2)])`, `(p2,2,[(p2,1)])`, `(p3,3,[(p3,1),(p4,2)])`, and `(p5,4,[(p5,1),(p6,2)])`. The corresponding control-free legacy variant is a separate ordinary-route conservation control; it cannot be used as evidence that ownership discovery reached a boundary/post dependency.

### 6.3 Nesting, owners, roots and first-prefix cases

For X01/X02 output is Group `{Outer:Repeated([Group{OuterFirst,OuterPosition,Inner},...])}` in that field order; Inner is the complete O tree. Other rows use only the explicitly listed fields, not an implicit RootMarker on a repeated root.

| ID | Literal model/control | Literal expected output |
|---|---|---|
| X01 | Outer P Starting First(2); child exact N Starting | Outer `(p0,1,O(N3)),(p2,2,O(N3))`. |
| X02 | Outer N Starting First(2); child exact P Starting | Outer `(n0,1,O(P4)),(n3,2,O(P4))`. |
| X03 | P Starting FromTo(3,3), nested same-C Starting=false | Outer First p3,pos1; inner O(`[G(1,p3,P.root,[p3,p4])]`). No p0/p1/p2/p5/p6. |
| X04 | Same outer as X03; owned member child | Members `[M(1,p3),M(2,p4)]`; separate binding after child reads outer group position1, not member2. |
| X05 | Flatten Batches X rows x1:F,x2:T; Y rows y1:F,y2:T | Runs `[x1] [x2,y1] [y2]`, positions1/2/3. Emit ordered `{First,GroupPosition,BatchName,Members}` with BatchName X,X,Y. Each member emits `{Label,MemberPosition,BatchName}`; middle members `(x2,1,X),(y1,2,X)`, not Y for y1. |
| X06 | Active Batch X Limit1 rows x1:F,x2:T,x3:F; Y Limit2 rows y1:F,y2:T,y3:T; root Limit0/row Limit0; First(pinned Batch Limit) | X groups `[x1]` ordinal1; Y `[y1] [y2]` ordinals1/2; windows receive each Batch parent. |
| X07 | Nonrepeating Group endpoint `[]` with Label single,Start=false | One G* `(single,1,[single])`; nested member1; separate explicit root-pin Label single. Author P and N independently. |
| X08 | Repeated root `[]` rows a:F,b:T,c:T,d:F | `[G*(1,a,[a]),G*(2,b,[b]),G*(3,c,[c,d])]`. Nested ordinary member empty-pin Label reads a;b;c,d at member1;1;1,2. Three routes: owned P[], owned N[], legacy named `[Alias]` over N root with owned binding demand. |
| X09 | X08 group binding explicit `frame=[]` Label | Typed uniterated repetition at binding node. Both declared repeating flags true/false, separately for all three routes. |
| X10 | X08 group then newer ordinary same-owner [] member; explicit [] Label | Labels a;b;c,d succeed. Plain ordinary repeated-root iteration without grouping is separate conservation control, labels a,b,c,d with positions1..4. |
| X11 | P repeated-root group, child ordinary N[] member, binding explicit P[] Label | Still P uniterated-repetition; foreign Item never shadows P group. |
| X12 | Group at C=Rows, root declared repeating but actual Group, no active exact [] | Explicit [] Marker -> InactiveSourceFrame(collection=[]). Separate nonrepeating declared+actual Group variant -> Marker P.root; [] Rows/Label still strict uniterated Rows. |
| X13 | Representative ordinary descendant Details repetition, owned scalar Rows/Details/Value without iterating Details | Typed uniterated-repetition, not first Detail. |
| X14 | Repeated Bool scalar root `[false,true,true,false]`, boundary=owned scalar [] | Repeated Scalar reps `[Bool(false),Bool(true),Bool(true)]`; nested member scalar payloads `[false] [true] [true,false]`, per-run positions1;1;1,2. Use Repeated scalar construction, never scalar First. |
| X15 | Outer Starting=false over all P; inner By Key, and separate AdjacentBy Key | By inner groups `[p0,p2,p3,p5,p6] [p1] [p4]`, firsts p0,p1,p4, ordinals1..3. Adjacent inner `[p0] [p1] [p2,p3] [p4] [p5,p6]`, ordinals1..5. Output outer First p0,pos1, Inner complete O of those literal Gs. |
| X16 | Outer P By Key; inner Starting Start | Outer firsts p0,p1,p4, positions1/2/3. Their inner lists respectively `[p0] [p2] [p3] [p5,p6]`; `[p1]`; `[p4]`, all with independently compact inner positions. |
| X17 | Outer P AdjacentBy Key; inner Starting Start | Outer firsts p0,p1,p2,p4,p5,pos1..5. Inner lists respectively `[p0]`;`[p1]`;`[p2] [p3]`;`[p4]`;`[p5,p6]`. |

### 6.4 Typed functional error and laziness catalogue

Control nodes B=101,F=102,S=103,W=104,P=105,T=106. Raise messages `boundary`,`filter`,`sort`,`bound`,`post`,`target`. Compare exact MappingException node/message and backend-native error fields/causes, not display strings. Ownership native association can be scope-based for a selector and node-based for a binding; retain both before portable normalization.

| ID | Literal variation | Expected first outcome / stopped work |
|---|---|---|
| E01 | First candidate p0 (stored Start=false), boundary expression Raise101; no prior group | MappingException(101,boundary), proving first predicate is evaluated; post/target0. |
| E02 | No sort; first F true, second F raises; B first raises | MappingException(101,boundary), before second F. |
| E03 | E02 B succeeds; first post raises | MappingException(105,post), before second F. |
| E04 | Sorting+FilterThenSort; second F raises; S/W/B/P armed | MappingException(102,filter), no S/W/B/P. |
| E05 | SortThenFilter; first S raises; W/F/B/P armed | MappingException(103,sort), no W/F/B/P. |
| E06 | Sort succeeds, W raises; B/P armed | MappingException(104,bound), no boundary/post. |
| E07 | First(0), B raises; separately implicit First | MappingException(101,boundary), not empty success. |
| E08 | First(0), B succeeds, P raises | MappingException(105,post), target0. |
| E09 | Starting false all; P first true, second raises | MappingException(105,post), no OR short circuit. |
| E10 | Boundary returns String("true") | NotABool(node101,found=string) before post; never coerced to true/key text. Separate Null,JsonNull,Int1,Float1,XmlNil tags each refuse their exact found kind. |
| E11 | Boundary Bool succeeds, post String("false") | NotABool(node105,found=string). |
| E12 | Empty source, W Bool(true) | NotAnItemCount(node104,found=bool); no candidate callbacks. |
| E13 | Windows First(0) then Raise104 | MappingException(104,bound); earlier empty does not skip later bound. |
| E14 | F always false; B/P/T armed | O([]); F7,B/P/T0, all bounds once. |
| E15 | P always false; T armed | O([]); B/P7,T0. |
| E16 | S08, Raise target only if representative p2 | Complete S08 output; p2 group target never reached, neighboring runs distinct. |
| E17 | Exact N absent; P/U Rows available | SourceUnavailable(exact Rows), before semantic callbacks, never P fallback. |
| E18 | Absent N read only by target binding, excluded First(0) or post-false | O([]); reached variant SourceUnavailable at that binding node. |
| E19 | Foreign exact owner position/pin with only P active | InactiveSourceFrame(exact owner,collection); foreign unpinned Rows/Label instead strict uniterated foreign repetition. |
| E20 | Earlier qualified primary-row global failure; target B raises | Existing failure rule/node/message wins for All/Primary/named selection; no target B. Root and ordinary failure controls remain unchanged. |
| E21 | Unknown/trimmed/case-changed owner | Original structural UnknownOwner; never normalize U/N. |
| E22 | Unknown exact target + absent input + invalid policy | Original UnknownTarget before supplied input/policy or grouping work. |
| E23 | FromTo first Raise at104,last Raise at107 | MappingException(104,bound); last0. Separate first succeeds/last raises107. |

For nonboolean found kinds use the current native/runtime variant's exact kind spelling and full cause; their cross-engine portable view may map explicitly but must not discard original products. Resource errors are separate below, not fabricated three-engine equivalents.

### 6.5 ItemAt combinations without permission leakage

Use actual four SequenceExpr shapes, valid isolated lexical item declarations and normal scalar target adaptation. Each row is independent; do not implement an ad-hoc producer.

| ID | Receiver and literal producer | Literal outcome |
|---|---|---|
| I01 | Starting = equal(ItemAt(Tokenize("F|F|T|T|F|T|F","|"),owned candidate position),"T") | P4, including consecutive true starts p2,p3. |
| I02 | Group binding ItemAt(Generate(from10,to12),owned group ordinal), target has only Value | Repeated groups `{Value:Int10},{Value:Int11},{Value:Int12},{Value:Null}` for P4. Member receiver variant uses member position: prefix10/11; p2=10; p3/p4=10/11; p5/p6=10/11. |
| I03 | Parent First bound ItemAt(Tokenize("0|2","|"),exact N root index2), P root index1, row indices1 | Bound converts String("2") by existing ItemCount; first two P4 groups. Producer/index never read first candidate/group. Add root Index fields/schema only in this explicit variant. |
| I04 | Starting compare ItemAt(TokenizeByLength("FFTTFTF",1),candidate position) to "T" | P4. |
| I05 | Starting compare ItemAt(TokenizeRegex("F|F|T|T|F|T|F",pattern "\\|",flags absent),candidate position) to "T" | P4. |
| I06 | Starting = ItemAt(Generate(from1,to7),candidate position)>Int2 | `[G(1,p0,P.root,[p0,p1]),G(2,p2,P.root,[p2]),G(3,p3,P.root,[p3]),G(4,p4,P.root,[p4]),G(5,p5,P.root,[p5]),G(6,p6,P.root,[p6])]`. |
| I07 | Boundary producer raises201,index raises202; separately empty/null-suppressed producer,index raises202 | First variant MappingException201,index0. Second202 even though empty; later producer arguments retain their null-suppression rules. |
| I08 | Starting false; post equal(nested eligible ItemAt selecting String("T") at once-index1,"T") | One full run p0..p6. Construct inner Tokenize("1","|") index1 -> String1, used as outer Tokenize("T","|") index; retain exact receiving source context. |

Keep ordinary #473 producer limits/index conversion/null and shared-node reevaluation controls as immutable conservation inputs. Four-producer support does not imply RecursiveCollect/FilterMap/SequenceExists/Aggregate support or owned failure eligibility.

### 6.6 Complete named-input catalogues

Fixed declarations `[unusedStatic,Rows,U,unavailableDynamic]`, ordinals0..3. Define full entries `(ordinal,exactName,availability)` in every expected catalogue, not availability-only assertions. Primary target uses P Starting; exact named target `audit` uses N Starting; `unicode` uses U Starting. Preserve exact output names/order.

- C01: exclusively owned demand -> All,Primary,audit,unicode each `[Optional,Optional,Optional,ReservedUnavailable]`. Missing Rows/U inputs succeed for selected Primary; reached selected audit fails unavailable Rows. All reaches its output order normally.
- C02: add Primary-only potential legacy Rows alias read (including lazy branch) -> Primary/All ordinal1 RequiredLegacy, audit/unicode ordinal1 Optional; all other entries unchanged. An actual P.Rows field does not remove this conservative requirement.
- C03: ordinary global failure potential legacy Rows alias -> ordinal1 RequiredLegacy in **all four** catalogues. Qualified authenticated primary-row failure at collection Rows without a legacy alias expression is a separate variant and does not itself require named Rows.
- C04: legacy Source(Rows), only boundary or post owned demand -> ordinal1 RequiredLegacy regardless of the walk actually choosing Primary. Legacy Alias over named root keeps its consumed alias binding required and canonical C=[]; build a separately named declaration for that route rather than pretending Alias=Rows.
- C05: node700 is owned N root Limit with explicit frame=[] (Int1); reuse it as First bound, in boundary `equal(700,Int1)`, grouped BoundEcho and member Echo. Expected complete catalogue entries remain C01 in every selection; N is supplied because the bound is reached. The target for this variant is exactly Group `{Groups:Repeated([Group{First:String(p0),GroupPosition:Int1,BoundEcho:Int1,Members:Repeated([Group{Label:String(p0),MemberPosition:Int1,Echo:Int1}])}])}`. Retain independent contextual visits, not one global visited-ID cache. An owned-only name does not become eager merely because the same ID is reached in a group.

### 6.7 Closed typed controls and structural precedence

Each subvariation is separate; test Project lowering and directly built Program where representable, both emitters, input immutability on refusal, and complete diagnostics (location/request/reason/origin).

1. Q01: every old default/Basic/filter/sort/window/row-failure/By/ItemAt/**AdjacentBy** wrapper rejects owned Starting with Grouping capability, exact iteration owner/path, original scope route/origin. Old By/ItemAt/Adjacent profiles retain all their original positive/negative results; do not retrofit Starting into their validators.
2. Q02: new companion rejects owned EndingWith and IntoBlocks, plus Starting under every inherited closed context listed in §4. Ordinary islands of all five existing legacy grouping modes retain ordinary dispatch/results, including closed-mode islands with no owned demand/ancestry.
3. Q03: missing predicate node99 in Project -> existing validation `group-starting-with predicate references missing node 99`; direct Program -> `MissingGroupingExpression { role:StartingPredicate,expression:99,target_path:[Groups] }`. Missing post99 retains MissingPostGroupFilterExpression. Post-without-group, conflicting By+Starting/Adjacent+Starting, source-free Starting retain their original structural precedence/messages. No unsupported-control fallback may hide them.
4. Q04: Aggregate/Lookup/CollectionFind, SequenceExists/SequenceAggregate in paired group target/boundary/post; RecursiveCollect/FilterMap ItemAt; generated-source iteration; isolated function; join; dynamic owner/loader; documents/XML/tree; concatenation; special/dynamic target construction remain original typed refusals. Mixed ItemAt+closed operator in sibling argument remains closed.
5. Q05: ItemAt in owned failure predicate/message remains FailureRule refusal even though a target Starting has ItemAt permission; no new owned failure iteration representation. New target marker does not admit ordinary forged grouped contexts or grouped observer snapshots.

## 7. Runtime-only probes and literal budget controls

These are actual component/cohort tests, **not fake Project capability cases**. Keep them separate from complete-tree three-engine runs.

| ID | Probe | Required literal frontier/counts |
|---|---|---|
| R01 | Authentic empty original, two bounds, all candidate/target callbacks armed | metadata1,bounds `[w0,w1]`; all others0; empty output. |
| R02 | Foreign-registry empty; cloned-parent empty | RegistryMismatch / InvalidCohortWitness input0 before metadata/semantics. |
| R03 | Duplicate IDs, incomplete/reordered original, corrupt terminal separately | Original typed witness/duplicate fields; semantic0. Preserve Rust/C# terminal-vs-metadata distinction in §2.3. |
| R04 | IDs0/1/2, boundary F/T/F, k2,h2 windows First(3),Last(3),identity sort, filters/posts true | metadata1,filter3,sort6,bounds2,boundary3,post3,target2. Exact SortThenFilter log `s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,b1,p1,f2,b2,p2,t0,t1`. |
| R05 | R04 FilterThenSort; select IDs0/2 | `f0,f1,f2,s00,s01,s20,s21,w0,w1,b0,p0,b2,p2,t0`; one false-prefix run IDs0/2. |
| R06 | R04 SortThenFilter; select IDs0/2 | `s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,f2,b2,p2,t0`. |
| R07 | All boundaries true on three IDs, post true; C# retain every target context | Three distinct immutable payload/context publications, first stays ID0/member1/group1 after later groups. Inspect full fixed capacities including unused n-key storage. |
| R08 | First(0) with R04 controls | Same candidate/boundary/post counts, target0; storage still preadmitted for all candidates. |
| R09 | Rust one Rows candidate, no sort, bool boundary | Known original text12 + template20 =32. B31 -> MetadataBytes used12/request20/max31 before semantics; B32 succeeds boundary1/post1. Separate two-row F/F variant: original24 + templates40 =64, B63 refuses used24/request40/max63 before semantics, B64 succeeds boundary2/post2/target1. Unlike keyed #442, Bool boundary adds **no** 2-byte or 4-byte key charge. |
| R10 | Rust root singleton R1/F1/P0/B0/W1, retained capacity4,k2,h48, nonstring sort keys | Seed38 + fixed/template/selection28 (`k2 + nk2 + 8n + 3 + T7 + U6`) + windows144 =210/maximum210. Capacity5 -> MetadataSlots used39/request172/max210, semantics0. No new control/adapter retained cells allowed. |
| R11 | Rust Rows singleton B31,W1,h118,retained capacity4 vs5 | At425 slots, known text refusal used12/request20/max31; at426, MetadataSlots used48/request378/max425 wins. |
| R12 | C# existing Ω/巣/行 fixture R2/F2/P2/W2,n2,k2,h2,s1,a2 with new Boolean boundary controls | Independently retained Seed90/64 + footprint177/142 =267 slots/206 bytes, ceiling3637. Each T36/40,U31/31. k1126 -> slots used90/request3549/max3637, directions CountReads1,IndexReads0,semantic0. LedgerSetup2 fits; limit1 -> used0/request2/max1. |
| R13 | R12, four returned sort keys String(é), boundaries F/F, post true | Known206 + sort8 =214; **not222**. B213 -> MetadataBytes used212/request2/max213 at fourth sort key, before any bound/boundary/post. B214 -> `s0,s1,s0,s1,w,b,p,b,p,t`, one target; Boolean callbacks add zero bytes. |
| R14 | Sort callback throws at exact R13 late frontier | Original callback error object wins; no text charge for nonexistent return, no later callbacks. Boundary callback throws after sort admission -> original error before post, not GroupKey text refusal. |
| R15 | Checked h+implicit overflow, slot products, element/array representability; W/F/P exact+1; full selector overlong even empty/missing/null | Original typed arithmetic/limit fields before allocation/semantics. Include consumed named alias raw extra segment and canonical endpoint-local limit separately. |
| R16 | Original By/Adjacent calls with duplicate String(é)/String(💠) keys under exact old thresholds | Their existing #442 every-key/before-post charges and exact errors remain unchanged; new bool operation never changes keyed accounting. |

The numeric controls above are independent source-counted recipes based on the existing #442 fixtures, not executed #443 evidence. R13 is a **different byte frontier** because Starting has no string grouping key, even though its retained slot layout is deliberately identical. Do not copy #442's B221/222 group-key threshold and call it Starting parity. Native tests must use its own WorkBudget oracle.

## 8. Qualification contract, not a claimed campaign

After independent draft review and exact #442 accepted merge: first freeze literal Projects/inputs/complete expected products, then implement owned paths, run focused native/common/runtime/emitter checks, then execute every functional case using the interpreter and **compiled public Rust/C# typed consumers** at All/Primary/exact named selections as applicable. Retain raw lossless outputs before comparison. Run each applicable selected/All expected product independently; do not turn a private runtime probe into a public Project success. Capture build-time source bindings and complete generator/runtime manifests before artifact generation. Compilation or source diff alone is not execution qualification.

Use one fresh qualification root per source freeze and keep observed products independent of oracles. Re-run against the exact head proposed for merge; include all prior By/Adjacent/ItemAt positive/negative and ordinary legacy conservation lanes. Preserve historical inputs and manifests. Required project checks remain a future parent-owned step; no build, compiler, runtime, native/vendor invocation, installation, repository modification or issue/PR write was performed in this source-only session.
