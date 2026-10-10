# Parallel Work

The [coordination issue](https://github.com/DeandreT/ferrule/issues/89) tracks
bounded work from the [roadmap](../ROADMAP.md). Issues define the problem,
in/out of scope, likely files and acceptance checks; this page helps contributors
choose a lane and avoid conflicting changes.

## Claim a lane

1. Read the issue and its linked contracts. Check current assignees and comments;
   the table below is a snapshot, not a replacement for the live issue.
2. Assign the issue to yourself before starting. Maintainer work is assigned to
   `DeandreT`; an unassigned `help wanted` issue is available. If you cannot set
   an assignee, ask the maintainer to assign you in the issue before proceeding.
3. State the files or modules you plan to own and any shared verification slot
   you will need. Agree on overlapping edits before changing the same source.
4. Keep one bounded result per PR and link its issue. For a newly discovered
   gap, open a scoped issue before implementing it; do not silently widen the
   issue you claimed.
5. Leave concise updates in first person (I/me): result, evidence link, blocker,
   and next step. Release ownership when pausing or handing off so another
   contributor can take it.

## Ownership and readiness

Snapshot: 2026-10-10 04:20:16 UTC, against merged main
[`fef16c3`](https://github.com/DeandreT/ferrule/commit/fef16c329591708d8437ac9d97e27bb7259cb438).
Completed rows record merged increments; active maintainer lanes are assigned to
`DeandreT`. Unassigned `help wanted` lanes remain available. Available does not
mean the implementation contract has already been agreed; design-first issues
settle their contract before code changes. Check the live issue for later updates.
The table contains 100 work lanes plus the #89 coordination row. The current
refresh is [#271](https://github.com/DeandreT/ferrule/issues/271); #252/#255 is merged.
#207/#249, #217/#251, #218/#262 and #250/#265 are merged, each with three successful
hosted jobs. All 229 selected JSON calls and #248's eight-measurement/eight-verifier/
four-decoder campaign are independently accepted. #248's report source/data and
one full rendered diagram are reviewed; report publication is still pending.
#203's fixture publication #266 is merged, with all three hosted jobs successful;
24 interpreter / 51 export-refusal checks, 237 library tests and fresh combined-
main 24+51 / strict / format checks pass. #264 is closed; faithful-form admission
remains open. #261 is closed and #270 is merged in main fef16c3, with all three
hosted jobs and fresh combined-main strict/format checks passing. These states
were fetched individually, not simultaneously.
#263's earlier 41 simulated controller-policy subcases match; its real native cycle
is unrun. #268's oracle/driver source is reviewed, but its new 68 helper, 17 policy
and 41 compatibility controls are unrun. #253's three tests / 11 internal controls,
344 engine tests, ordinary engine build and corrected strict/format checks pass
locally. Its #277 test-helper prerequisite and #253 publication share open #278;
hosted jobs are pending. #201's 67 native calls and desktop duplication workflows
remain unrun; the GUI lane needs disk admission.

All three Mermaid fence bodies remain byte-exact historical views from #252 and
earlier refreshes. The table and prose below record newer lanes and dependencies;
the old graph labels are not a current status census. Earlier bounded SVG/PNG
renders retain their cleanup and zoom limits; no fresh render is claimed here.
Source review, local tests, generated calls and physical workflows retain separate
gates. Earlier zero-step billing failures remain failures; hosted execution has
resumed, and pending jobs are not passes.

| Issue | Owner | Next gate | Primary ownership / coordination |
| --- | --- | --- | --- |
| [#89 Coordination](https://github.com/DeandreT/ferrule/issues/89) | Complete; #109 merged | Live issue map; dated documentation refresh in #271 | Docs; coordinate all overlaps |
| [#90 Computed JSON properties](https://github.com/DeandreT/ferrule/issues/90) | Complete; #110 merged | Local GUI checks complete; desktop separate | Scope inspector, workspace and GUI tests |
| [#91 Isolated Value map conversion](https://github.com/DeandreT/ferrule/issues/91) | Complete; [#123](https://github.com/DeandreT/ferrule/pull/123) merged | Local conversion gates complete; ordinary #113 remains a distinct context | Runtime helpers and both emitters |
| [#92 Workbook desktop persistence](https://github.com/DeandreT/ferrule/issues/92) | Complete; [#132](https://github.com/DeandreT/ferrule/pull/132) merged | Flat fixture create/save/reopen complete; other workflows separate | Desktop lane; #98 extends primary source setup |
| [#93 XML root annotations](https://github.com/DeandreT/ferrule/issues/93) | DeandreT; active | Metadata observation pending | XML/.mfd metadata; shared display |
| [#94 JSON object conformance cells](https://github.com/DeandreT/ferrule/issues/94) | Complete; [#129](https://github.com/DeandreT/ferrule/pull/129) merged | Proposal/validator checks complete; no Supported cells | Conformance docs and validator fixtures; survey unchanged |
| [#95 Generated XML limit coverage](https://github.com/DeandreT/ferrule/issues/95) | Complete; [#121](https://github.com/DeandreT/ferrule/pull/121) merged | Source/link review complete; rendering unverified; cohorts recorded in #133 | XML coverage index and host guide |
| [#96 Generated JSON5 adapters](https://github.com/DeandreT/ferrule/issues/96) | Complete; initial scope closed | All initial language/public/resource increments merged | Parent contract; broader identifiers/native metadata remain separate |
| [#97 Typed Value map cells](https://github.com/DeandreT/ferrule/issues/97) | Complete; [#128](https://github.com/DeandreT/ferrule/pull/128) merged | Local editor gates complete after #91/#114; desktop separate | Cell editor; no coercion changes |
| [#98 Transposed workbook source setup](https://github.com/DeandreT/ferrule/issues/98) | Complete; [#150](https://github.com/DeandreT/ferrule/pull/150) merged | Local and scoped desktop setup/save/reopen checks complete; workbook execution checked locally | Primary-source workbook wizard; hierarchical/named setup excluded |
| [#99 Browser edit history](https://github.com/DeandreT/ferrule/issues/99) | Complete; [#178](https://github.com/DeandreT/ferrule/pull/178) merged | 43 web tests, fresh WASM and finite 132-command/34-download browser review; original oracle/cleanup limits below | Web app/history/canvas; preserve #165; #177 is separate net-zero correction |
| [#100 Required-only reference siblings](https://github.com/DeandreT/ferrule/issues/100) | Complete; native [#170](https://github.com/DeandreT/ferrule/pull/170) and generated [#174](https://github.com/DeandreT/ferrule/pull/174) merged | Exact native/generated presence subset qualified; broader composition separate | JSON Schema presence subset only; coordinate #101; no general intersection |
| [#101 String-or-Int range constraints](https://github.com/DeandreT/ferrule/issues/101) | Complete; [#183](https://github.com/DeandreT/ferrule/pull/183) merged | Exact native/direct/generated contract; all 152 compiled calls and scoped checks pass | Exact String-or-Int IntegerRange only; IR/JSON constraints and both validators; broader unions separate |
| [#102 Scalar-sequence composition](https://github.com/DeandreT/ferrule/issues/102) | Complete; [#188](https://github.com/DeandreT/ferrule/pull/188) merged | Reviewed design plus 37 proposed feature literals and two legacy controls; execution in separate lanes | [Design](design/scalar-sequence-composition.md) and [full corpus](design/fixtures/scalar-filter-map-v1.json); no implementation in #102 |
| [#103 Typed SQLite query boundary](https://github.com/DeandreT/ferrule/issues/103) | Unassigned | Read-only query contract first | Boundary model, SQLite and CLI input |
| [#104 PDF extraction isolation](https://github.com/DeandreT/ferrule/issues/104) | Unassigned | Portable worker/limit contract first | PDF, CLI and applicable preview worker |
| [#105 Offline mapping bundle export](https://github.com/DeandreT/ferrule/issues/105) | Unassigned | Bundle contract first | CLI staging, resources and path codecs |
| [#106 JSON5 interchange metadata](https://github.com/DeandreT/ferrule/issues/106) | Unassigned | Observe saved metadata before implementation | Metadata lane; coordinate display with #93 |
| [#107 JSON output-set memory trials](https://github.com/DeandreT/ferrule/issues/107) | Complete; [#185](https://github.com/DeandreT/ferrule/pull/185) merged | Eight normal-dev measurements, eight separate verifiers and twelve complete documents pass; [report](performance/json-output-set-memory.md) retains finite limits | Fixture/host and filesystem JSON performance report only; #184 uses this baseline; no optimization |
| [#108 Mapping-path desktop workflow](https://github.com/DeandreT/ferrule/issues/108) | Unassigned | Existing implementation; desktop gate open | Desktop scenario; coordinate with #92/#93 |
| [#111 Project Float precision](https://github.com/DeandreT/ferrule/issues/111) | Complete; [#120](https://github.com/DeandreT/ferrule/pull/120) merged | Local precision gates complete; other contexts qualify separately | JSON configuration and persistence fixtures |
| [#112 C# JSON Float rounding](https://github.com/DeandreT/ferrule/issues/112) | Complete; [#120](https://github.com/DeandreT/ferrule/pull/120) merged | Local rounding gates complete in the #111 increment | C# number parser and boundary fixtures |
| [#113 Ordinary Value map JSON null](https://github.com/DeandreT/ferrule/issues/113) | Complete; [#126](https://github.com/DeandreT/ferrule/pull/126) merged | Ordinary direct-helper/public-host checks complete; isolated marker semantics preserved | Ordinary helpers; distinct from #91 |
| [#114 Enabled absent defaults](https://github.com/DeandreT/ferrule/issues/114) | Complete; [#127](https://github.com/DeandreT/ferrule/pull/127) merged | Persistence/history checks complete; legacy missing/null unchanged | Field-local model serialization and history tests |
| [#115 Status refresh](https://github.com/DeandreT/ferrule/issues/115) | Complete; [#125](https://github.com/DeandreT/ferrule/pull/125) merged | Earlier snapshot retained; superseded by #149 | Roadmap and contributor map only |
| [#116 XML input byte boundary](https://github.com/DeandreT/ferrule/issues/116) | Complete; [#131](https://github.com/DeandreT/ferrule/pull/131) merged | Small and opt-in compiled-host checks complete | Static input child; declared census/error precedence retained |
| [#117 XML output byte boundary](https://github.com/DeandreT/ferrule/issues/117) | Complete; [#137](https://github.com/DeandreT/ferrule/pull/137) merged | Small and physical exact/+1 checks complete | Primary document-list child; no partial returned results |
| [#118 XML loader count boundary](https://github.com/DeandreT/ferrule/issues/118) | Complete; [#138](https://github.com/DeandreT/ferrule/pull/138) merged | Real callback and next-reservation checks complete | Dynamic input child; no counter priming |
| [#119 XML mapping/count priority](https://github.com/DeandreT/ferrule/issues/119) | Complete; [#139](https://github.com/DeandreT/ferrule/pull/139) merged | Lazy last-row Raise before mixed count checked | Mixed output child; no pre-target global rule substitute |
| [#122 Numeric codec documentation](https://github.com/DeandreT/ferrule/issues/122) | Complete; [#124](https://github.com/DeandreT/ferrule/pull/124) merged | Source/link/Rustdoc checks complete; rendering unverified | Project-files guide, host-guide paragraph and opening Rustdoc |
| [#130 Native JSON5 nonfinite tokens](https://github.com/DeandreT/ferrule/issues/130) | Complete; [#164](https://github.com/DeandreT/ferrule/pull/164) merged | 190 native controls, 58 CLI calls and full affected suite pass; BOM accounting separate #163 | Native pre-projection admission; generated adapters and metadata excluded |
| [#133 XML qualification documentation](https://github.com/DeandreT/ferrule/issues/133) | Complete; [#147](https://github.com/DeandreT/ferrule/pull/147) merged | Source/link review of four completed cohorts and measurements | XML coverage index and memory guide; no new measurements |
| [#134 Rust JSON5 syntax](https://github.com/DeandreT/ferrule/issues/134) | Complete; [#140](https://github.com/DeandreT/ferrule/pull/140) merged | Shared syntax/scaled-limit checks complete; public projection separate | Pure normalizer and syntax controls |
| [#135 JSON5 Unicode identifiers](https://github.com/DeandreT/ferrule/issues/135) | Unassigned | Shared frozen membership tables and both-language contract first | Optional Unicode extension; initial ASCII subset stays explicit |
| [#136 C# JSON5 syntax](https://github.com/DeandreT/ferrule/issues/136) | Complete; [#146](https://github.com/DeandreT/ferrule/pull/146) merged | Shared syntax/scaled-limit checks complete; public projection separate | Package-free pure normalizer; ordinary source set unchanged |
| [#141 JSON5 closed-object eligibility](https://github.com/DeandreT/ferrule/issues/141) | Complete; [#151](https://github.com/DeandreT/ferrule/pull/151) merged | Policy checks complete with #148; public adapters separate | Shared schema/profile predicate and contract |
| [#142 Rust JSON5 adapters](https://github.com/DeandreT/ferrule/issues/142) | Complete; [#153](https://github.com/DeandreT/ferrule/pull/153) merged | Local optional codec/emitter checks complete; physical gate separate | Optional Rust APIs only; ordinary strict defaults preserved |
| [#143 C# JSON5 adapters](https://github.com/DeandreT/ferrule/issues/143) | Complete; [#155](https://github.com/DeandreT/ferrule/pull/155) merged | Local smoke/emitter/public checks complete; physical gate separate | Optional package-free C# APIs only; ordinary source set preserved |
| [#144 JSON5 generation and small hosts](https://github.com/DeandreT/ferrule/issues/144) | Complete; [#156](https://github.com/DeandreT/ferrule/pull/156) merged | All 184 small compiled public calls and CLI checks complete | Explicit CLI opt-in; 23 literal cases across four routes per language |
| [#145 JSON5 byte boundaries](https://github.com/DeandreT/ferrule/issues/145) | Complete; [#159](https://github.com/DeandreT/ferrule/pull/159) merged | All 48 physical calls pass after all 184 small prerequisites | Serial resource fixtures; whole-host RSS is not an API-only peak or RAM ceiling |
| [#148 JSON5 descriptor duplicates](https://github.com/DeandreT/ferrule/issues/148) | Complete; [#151](https://github.com/DeandreT/ferrule/pull/151) merged | Duplicate/error-priority controls complete with #141 | Descriptor decoder guard; ordinary JSON codec unchanged |
| [#149 Current status refresh](https://github.com/DeandreT/ferrule/issues/149) | Complete; [#152](https://github.com/DeandreT/ferrule/pull/152) merged | Source/link/dependency checks complete; rendering unverified; superseded by #158 | Roadmap and this map; no implementation |
| [#154 JSON5 guide refresh](https://github.com/DeandreT/ferrule/issues/154) | Complete; [#161](https://github.com/DeandreT/ferrule/pull/161) merged | Public routes, all 184 small/all 48 physical calls and whole-host RSS documented | JSON5 contract guide only; rendering unverified |
| [#157 Direct-Program Rust emission](https://github.com/DeandreT/ferrule/issues/157) | Complete; [#168](https://github.com/DeandreT/ferrule/pull/168) merged | 110 tests, three warnings-denied generated hosts and 11 public calls pass | Ordinary Rust expression retention after validation; lowering and #145 fixtures excluded |
| [#158 JSON5 progress and lanes](https://github.com/DeandreT/ferrule/issues/158) | Complete; [#162](https://github.com/DeandreT/ferrule/pull/162) merged | Earlier two-doc source/link/dependency review; rendering unverified; superseded by #173 | Roadmap and this map; separate #154 guide retained |
| [#160 C# JSON5 allocation attribution](https://github.com/DeandreT/ferrule/issues/160) | Unassigned | Matched profiling/retention plan before attribution conclusions | Generated C# benchmark/report only; exclude production optimization and filesystem #107 |
| [#163 Native JSON5 original-byte accounting](https://github.com/DeandreT/ferrule/issues/163) | Complete; [#167](https://github.com/DeandreT/ferrule/pull/167) merged | 45 small controls and eight physical calls pass; original UTF-8 bytes include the BOM | Native reader seam and corresponding memory row; no streaming/RAM claim |
| [#165 Missing browser target bindings](https://github.com/DeandreT/ferrule/issues/165) | Complete; [#166](https://github.com/DeandreT/ferrule/pull/166) merged | Eight layout cases, 38 browser tests and WASM pass; #99 must retain them | Canvas target-wire construction only; history and admission excluded |
| [#169 Generated required-reference qualification](https://github.com/DeandreT/ferrule/issues/169) | Complete; [#174](https://github.com/DeandreT/ferrule/pull/174) merged | 183 native observations, all 594 compiled calls, 23 CLI checks and capture controls pass; closes #100 generated gate | CLI qualification leaf, corpus and Rust/C# hosts; no importer or runtime fix |
| [#171 JSON required-property error precedence](https://github.com/DeandreT/ferrule/issues/171) | Complete; [#172](https://github.com/DeandreT/ferrule/pull/172) merged | 30 direct observations and native/Rust/C# suites pass; #169 compiled mapping witness passes separately | Narrow C# runtime check order and direct tests; no importer/browser edits |
| [#173 Roadmap and lane refresh](https://github.com/DeandreT/ferrule/issues/173) | Complete; [#175](https://github.com/DeandreT/ferrule/pull/175) merged | Earlier two-doc source/link/diagram review; rendering unverified; superseded by #182 | ROADMAP.md and this map only; separate from implementation |
| [#176 Applied browser target names](https://github.com/DeandreT/ferrule/issues/176) | Complete; [#181](https://github.com/DeandreT/ferrule/pull/181) merged | 45 web tests, fresh WASM, 24 actual native browser commands and two typed/byte-checked downloads | Borrowed applied schema title and existing eight layout cases; ordinary canvas/model/wires preserved |
| [#177 Coalesced browser baseline return](https://github.com/DeandreT/ferrule/issues/177) | Complete; [#179](https://github.com/DeandreT/ferrule/pull/179) merged | Exact retained baseline collapse; 45 web tests, scoped strict lint and formatting pass; no fresh browser campaign | Browser history and state-machine regressions; strict byte ledger, discarded redo and distinct focus sessions |
| [#180 Browser Fit view transform](https://github.com/DeandreT/ferrule/issues/180) | Complete; [#195](https://github.com/DeandreT/ferrule/pull/195) merged | 54 unique web tests, 12 Fit checkpoints, 66 browser commands and two complete downloads; owned cleanup required | Whole-node view transform; Project/node layout preserved; broader authoring separate |
| [#182 Browser progress and coordinated lanes](https://github.com/DeandreT/ferrule/issues/182) | Complete; [#187](https://github.com/DeandreT/ferrule/pull/187) merged | Earlier two-doc source/link/diagram review; rendering unverified; superseded by #212 | ROADMAP.md and this map only; dated main302823 snapshot retained |
| [#184 Borrowed ordinary JSON serialization](https://github.com/DeandreT/ferrule/issues/184) | Complete; [#200](https://github.com/DeandreT/ferrule/pull/200) merged | 398 scoped regressions and 126-call byte equality; eight matched fixture peaks 8.7–40.4 MiB lower | Ordinary writer/private view and [report](performance/borrowed-json-output-memory.md); full String, validation and publication preserved; no RAM cap |
| [#186 Generated scalar-UDF argument error order](https://github.com/DeandreT/ferrule/issues/186) | Complete; [#196](https://github.com/DeandreT/ferrule/pull/196) merged | All 18 compiled calls per language match independent expectations; original four mismatches/language retained | Ordinary/nested Rust/C# call rendering only; interpreter/coercion/frame/sequence/GUI policy unchanged |
| [#189 Scalar sequence model and guards](https://github.com/DeandreT/ferrule/issues/189) | Complete; [#197](https://github.com/DeandreT/ferrule/pull/197) merged | 1,759 distinct scoped regressions; model/codec/guard qualification | Two private owners, ordered captures, signatures and legacy serialization; execution separate |
| [#190 Native scalar sequence execution](https://github.com/DeandreT/ferrule/issues/190) | Complete; [#202](https://github.com/DeandreT/ferrule/pull/202) merged | All 37 frozen feature cases, two legacy controls and 1,011 distinct regressions; five local gates | Native eager construction, lazy selected mapper, positions, shared feature limits/cancellation; no global legacy budget |
| [#191 Common scalar sequence lowering](https://github.com/DeandreT/ferrule/issues/191) | Complete; [#204](https://github.com/DeandreT/ferrule/pull/204) merged | 62 complete comparisons, 497 distinct library tests and six local gates | Stage UDF roots, transitive callees and direct Program validation; backend execution separate |
| [#192 Rust scalar sequence execution](https://github.com/DeandreT/ferrule/issues/192) | Complete; [#206](https://github.com/DeandreT/ferrule/pull/206) merged | 188 public calls, 30 separate direct controls, 365 library tests and seven local gates | Rust runtime/emitter; 40 compiled plus seven static/decode profiles; preserve ordinary APIs |
| [#193 C# scalar sequence execution](https://github.com/DeandreT/ferrule/issues/193) | Complete; [#208](https://github.com/DeandreT/ferrule/pull/208) merged | 172 public calls, 35 separate direct controls, 432 library tests, 179 smoke groups and eight local steps | C# runtime/emitter and 43 frozen profiles; full errors/causes retained |
| [#194 Compact scalar sequence editor](https://github.com/DeandreT/ferrule/issues/194) | Complete; [#214](https://github.com/DeandreT/ferrule/pull/214) merged | Final fresh strict/fmt/affected-unit/build pass; historical 20 focused/742 GUI/31 web-demo crate tests; finite desktop inspection, identity Rust/C# generation and two 172-byte public String JSON host calls qualified | Descriptor controls, icons, palette/scopes/history; 16 new tests and 13 local native/Preview recipes; no all-13 physical or all-39 claim; GUI cleanup and normal host closure separate |
| [#198 Generated typed target selection](https://github.com/DeandreT/ferrule/issues/198) | Complete; [#219](https://github.com/DeandreT/ferrule/pull/219) merged | 90 native/Rust/C# comparisons, 435 owning library tests, three compiled regressions and strict/fmt pass | Typed selection preserves all-output APIs, complete static admission, failure order and lazy selected loaders; selected JSON is #218 |
| [#199 Legacy Rust aggregate predicate generation](https://github.com/DeandreT/ferrule/issues/199) | Complete; [#209](https://github.com/DeandreT/ferrule/pull/209) merged | 15 native controls, 45 compiled typed/text/byte calls, 114 Rust tests and six local steps | Existing predicate helper order only; old compiler mismatch retained; frozen new sequence behavior unchanged |
| [#201 Sequence-consumer duplication](https://github.com/DeandreT/ferrule/issues/201) | DeandreT; source proposal accepted | 28 frozen controls, 87 snapshots and 56 canvas tables; all 67 native calls and desktop checks unrun; GUI disk admission and fresh source binding pending | Atomic private-owner remapping; preserve #213 Fit hooks and history/layout; general clipboard/arbitrary graph copy excluded |
| [#203 Scalar sequence saved-design interchange](https://github.com/DeandreT/ferrule/issues/203) | DeandreT; fixture published; saved-form design open | 24 interpreter comparisons, 51 typed export refusals and 237 library tests pass; [#266](https://github.com/DeandreT/ferrule/pull/266) merged with three hosted successes; fresh combined-main 24+51 / strict / fmt pass | Frozen synthetic design and public observations admit no faithful new saved form; #264 fixture envelope only; #253 internal and #263 native cycle separate |
| [#205 Generated test artifact retention](https://github.com/DeandreT/ferrule/issues/205) | Complete; [#216](https://github.com/DeandreT/ferrule/pull/216) merged | Five matched tests, two retained failing-comparison controls, portable fallback, strict/fmt pass; full emitted bytes and exposed errors conserved | Rust/C# JSON5 tests and test-only helper; measured log/file bytes do not establish memory saving or unrecorded historical error details |
| [#207 Eager scalar sequence memory study](https://github.com/DeandreT/ferrule/issues/207) | Complete; [#249](https://github.com/DeandreT/ferrule/pull/249) merged | All 72 measured runs, 72 separate complete verifiers and three independent backend reviews pass; [report](performance/scalar-sequence-memory.md) retains both repetitions and 36 matched pairs | Bounded native/Rust/C# fixtures and report; allocation profiling is #248; no production optimization, policy change or RAM guarantee |
| [#217 Older generated CLI host policy](https://github.com/DeandreT/ferrule/issues/217) | Complete; [#251](https://github.com/DeandreT/ferrule/pull/251) merged | Nine helper controls, three paired Rust/C# regressions and three default-cleanup repeats pass; three deliberate environment/configuration refusals retained; all three hosted jobs succeed | Shared host-cache configuration and complete original retention for three older test wrappers; [usage](development.md#reuse-generated-cli-host-builds); production behavior excluded |
| [#210 Sequence editor generation guidance](https://github.com/DeandreT/ferrule/issues/210) | Complete; [#214](https://github.com/DeandreT/ferrule/pull/214) merged | Reviewed one-sentence guidance and cohesive #194 local/finite desktop/generation qualification complete | Same editor source and cohesive GUI increment; no backend/save behavior change |
| [#211 Compact GUI frame diagnostics](https://github.com/DeandreT/ferrule/issues/211) | Complete; [#232](https://github.com/DeandreT/ferrule/pull/232) merged | Same 20 tests retain complete outcomes; logs 538.6 MB to 23.6 MB (95.6%); strict/fmt pass | Test-only frame evidence; log bytes separate from process memory and generated-artifact #205 |
| [#212 Earlier roadmap and lane refresh](https://github.com/DeandreT/ferrule/issues/212) | Complete; [#215](https://github.com/DeandreT/ferrule/pull/215) merged | Dated source/link/checklist and bounded render review; owned cleanup and map zoom limits retained | ROADMAP.md and this map only; latest refresh is #271; no product gate promotion |
| [#213 Desktop Fit node bounds](https://github.com/DeandreT/ferrule/issues/213) | Complete; [#238](https://github.com/DeandreT/ferrule/pull/238) merged | Nine Fit tests plus popup regression, full 749 GUI, strict/fmt/build and four finite alternate-display parts pass | Desktop view transforms on main/named/single/empty; saved Project/layout invariant; bus-helper cleanup distinct from GUI closure; browser/authoring excluded |
| [#218 Selected-target JSON APIs](https://github.com/DeandreT/ferrule/issues/218) | Complete; [#262](https://github.com/DeandreT/ferrule/pull/262) merged | All 229 independent calls pass: 76 native, 76 Rust, 77 C#; three hosted jobs succeed; original 15 C# comparator mismatches and #257 correction retained | Additive text/UTF-8 APIs; public JSON results and separate decoded-wire observations; exact names, static admission, lazy loaders and limits preserved; guide #261 separate |
| [#220 Hosted nightly lint repair](https://github.com/DeandreT/ferrule/issues/220) | Complete; [#222](https://github.com/DeandreT/ferrule/pull/222) merged | 299 local tests, strict/fmt and hosted lint steps pass; later mapping inventory failures retained separately | Four mechanical warning repairs; mapping behavior and toolchain policy unchanged |
| [#221 Generated C# raw X12 004010](https://github.com/DeandreT/ferrule/issues/221) | Complete; [#226](https://github.com/DeandreT/ferrule/pull/226) merged | Bounded package-free raw text/byte/context APIs and synthetic native/compiled controls; later mapping-job failure remains distinct | Opt-in schema-guided X12 reader/writer; ordinary typed/JSON APIs unchanged; external certification excluded |
| [#223 Exact generated test inventories](https://github.com/DeandreT/ferrule/issues/223) | Complete; [#228](https://github.com/DeandreT/ferrule/pull/228) merged | 12 affected mappings, manifest, cleanup controls and strict/fmt pass; all three hosted jobs successful | Exact ordinary 77 / optional 78 filenames; test-only common helper and five wrappers; preserve semantic assertions |
| [#224 Native fixed-width ISA output](https://github.com/DeandreT/ferrule/issues/224) | Complete; [#230](https://github.com/DeandreT/ferrule/pull/230) merged | Independent physical output/refusal controls; ordinary/crate hosted jobs pass; later mapping failure separate | Complete 16-field ISA encoding, fixed widths and caller controls; partial schemas/other dialects preserved |
| [#225 Successful typed XML artifact retention](https://github.com/DeandreT/ferrule/issues/225) | Complete; [#229](https://github.com/DeandreT/ferrule/pull/229) merged | Affected mapping retention and separate default cleanup controls pass; all three hosted jobs successful | Three Drop guards only; coordinate #223 wrappers; commands/assertions and production unchanged |
| [#227 Atomic publication diagnostics](https://github.com/DeandreT/ferrule/issues/227) | Complete; [#231](https://github.com/DeandreT/ferrule/pull/231) merged | Actionable unsupported no-replace diagnostic; original OS failure and paths retained | CLI publication boundary; no non-atomic fallback or general cache cleanup |
| [#233 Read-only SQLite metadata](https://github.com/DeandreT/ferrule/issues/233) | Complete; [#236](https://github.com/DeandreT/ferrule/pull/236) merged | Literal filename/read-only refusal controls and three hosted jobs pass | No CREATE/URI/write fallback or recovery; MFD fallback exact; WAL sidecars remain possible; query model is #103 |
| [#234 Confined MFD resource reads](https://github.com/DeandreT/ferrule/issues/234) | Complete; [#243](https://github.com/DeandreT/ferrule/pull/243) merged | Nested schema/module containment controls and three hosted jobs pass | Canonical authorizing root through transitive reads; accepted contained paths and fallback diagnostics preserved; runtime module execution excluded |
| [#235 CLI executable MFD admission](https://github.com/DeandreT/ferrule/issues/235) | Complete; [#240](https://github.com/DeandreT/ferrule/pull/240) merged | Single-project static admission/refusal before publication and three hosted jobs pass | Explicit --require-executable; ordinary repair imports and ordered rules unchanged; pipeline combination refused |
| [#237 Variadic Boolean logical calls](https://github.com/DeandreT/ferrule/issues/237) | Complete; [#245](https://github.com/DeandreT/ferrule/pull/245) merged | Native/generated ordered Boolean and arity controls; three hosted jobs pass | and/or admit at least two strict Bool operands; eager evaluation; nullable external equivalence separate |
| [#239 Nested target clone ownership](https://github.com/DeandreT/ferrule/issues/239) | Complete; [#244](https://github.com/DeandreT/ferrule/pull/244) merged | Unequal/equal declared branches, ordered outputs and refusal controls; three hosted jobs pass | Declared ancestor distribution and source anchors; no ownership inferred from counts or competing singular-feed winner |
| [#241 Singular-target multiple feeds](https://github.com/DeandreT/ferrule/issues/241) | DeandreT; contract preparation | Independent cardinality/order/null/error contract before admission or execution | Same-scope competing feeds remain refused; distinct expressions preserved; #239 ancestor branches are separate |
| [#242 Linux memory-study guard](https://github.com/DeandreT/ferrule/issues/242) | Complete; merged in [#249](https://github.com/DeandreT/ferrule/pull/249) | Linux host warning-denied build and six recorder controls pass; publication complete | Source guards exclude Linux observations elsewhere; no actual other-platform compilation or generated portability change |
| [#246 Memory-study Rust workspace packaging](https://github.com/DeandreT/ferrule/issues/246) | Complete; merged in [#249](https://github.com/DeandreT/ferrule/pull/249) | Corrected offline composition/build and full #207 campaign pass; initial lock failure retained | Only composed member manifests lose standalone workspace markers; raw emitted artifact sets unchanged |
| [#247 Pages deployment HTTP 404](https://github.com/DeandreT/ferrule/issues/247) | Unassigned; available | Actual cause/configuration repair and successful main deployment pending | Repository Pages/workflow only; successful site build is separate; no dependency on mapping or memory work |
| [#248 C# large-string allocation attribution](https://github.com/DeandreT/ferrule/issues/248) | DeandreT; finite campaign/report source accepted | Eight measurements, eight independent output verifiers and four trace decoders accepted; report source/data and one full diagram reviewed; publication pending | #207 fixtures; sampled allocations/stacks and trace perturbation separate from RSS; owned helper cleanup/viewport scroll retained; no exact ledger, phase-clock, capture-cause, optimization or new budget claim |
| [#250 Generated C# X12 format options](https://github.com/DeandreT/ferrule/issues/250) | Complete; [#265](https://github.com/DeandreT/ferrule/pull/265) merged | Bounded directional 004010 option qualification and all three hosted jobs pass | Parsing/numeric/lexical/completion contracts preserve supplied contexts, strict defaults and typed/JSON APIs; external certification and other versions excluded |
| [#252 Earlier roadmap and lane refresh](https://github.com/DeandreT/ferrule/issues/252) | Complete; [#255](https://github.com/DeandreT/ferrule/pull/255) merged | Dated two-doc source/link/render review, strict/fmt and three hosted jobs pass; diagrams retain zoom/cleanup limits | Only ROADMAP.md and this map; historical graphs preserved; new dated refresh is #271 |
| [#253 Internal sequence observations](https://github.com/DeandreT/ferrule/issues/253) | DeandreT; local gates accepted; [#278](https://github.com/DeandreT/ferrule/pull/278) open | Three tests / 11 internal controls, 344 engine tests, ordinary engine build and corrected workspace strict/fmt pass; independent complete observations accepted; hosted/publication pending | Engine unit leaf, lib.rs registration and borrowed cfg(test) sink; #203 public observations remain separate; #277 publication prerequisite; construction reach is not allocator telemetry |
| [#257 Selected JSON error comparator](https://github.com/DeandreT/ferrule/issues/257) | Complete; merged in [#262](https://github.com/DeandreT/ferrule/pull/262) | All 77 corrected C# calls pass; original 11 small and four size mismatches remain retained failures | Sole test comparator uses the frozen JSON-boundary Detail; production/runtime/emitter/oracles unchanged; qualification prerequisite of #261 |
| [#261 Selected JSON API guide](https://github.com/DeandreT/ferrule/issues/261) | Complete; [#270](https://github.com/DeandreT/ferrule/pull/270) merged | Independent source and rendered-diagram review, all three hosted jobs and fresh combined-main strict/fmt pass; issue closed | docs/code-generation.md only; merged #218 and corrected #257 remain prerequisites; no implementation/status-report changes |
| [#263 Native saved-file reopen checkpoint](https://github.com/DeandreT/ferrule/issues/263) | DeandreT; simulated policy controls accepted | 13 groups / 41 subcases match; individually admitted design/oracles/bindings and real native reopen/execute/save2 cycle unrun | Neutral checkpoint protocol/report; preserve exact saved1, separate save2, complete outputs/errors and structural identities; #268 oracle prerequisite; no faithful filter/map seed inferred |
| [#264 Public parent-position fixture envelope](https://github.com/DeandreT/ferrule/issues/264) | Complete; [#266](https://github.com/DeandreT/ferrule/pull/266) merged | Corrected public 24+51 cohort passes with frozen fixtures; original four case-8 failures retained; issue closed | Test-only ordinary Parents/Driver source frames preserve parent7/terminal1; prerequisite of #203 public qualification; no production position change |
| [#268 Complete XML checkpoint oracles](https://github.com/DeandreT/ferrule/issues/268) | DeandreT; oracle/driver source accepted | Complete independent XML trees/path-metadata contract and drivers reviewed; new 68 helper, 17 policy and 41 compatibility controls all unrun | Coordinate #263 comparison role; earlier #263 simulated 41 evidence remains separate; preserve byte mode/original outputs; real native cycle unrun; no observed-output oracle or new saved-design form |
| [#271 Current roadmap and lane refresh](https://github.com/DeandreT/ferrule/issues/271) | DeandreT; dated source preparation | Bind main fef16c3 and individually fetched statuses; V2 source review accepted, fresh two-doc source/link review and publication pending | ROADMAP.md and this map only; preserve historical fences; #248 report publication, #201 GUI, #253/#277 publication and #263/#268 actuals remain separate |
| [#277 Capture-observation helper let-chain](https://github.com/DeandreT/ferrule/issues/277) | DeandreT; local prerequisite accepted; [#278](https://github.com/DeandreT/ferrule/pull/278) open | Single test-only condition independently reviewed; corrected same three tests / 11 controls, 344 engine tests and strict/fmt pass; original strict failure retained; hosted/publication pending | Prerequisite of #253 publication in the same focused PR; preserve evaluation order/complete observations and production code; no additional distinct control cohort |


Computed JSON properties retain their #110 local qualification; normal desktop
authoring remains separate. The #111/#112 precision increment merged in #120,
followed by #91 in #123, ordinary declared JSON-null matching #113 in #126, enabled
absent-default persistence #114 in #127, and typed-cell authoring #97 in #128.
Those increments preserve their distinct ordinary/isolated and persistence/UI
contracts. The original input-conversion and precision cohorts are not a claim
that every later editor workflow has desktop coverage.

The #92 ordinary flat-workbook desktop fixture is qualified in #132. The #98
primary transposed-source wizard is merged in #150 after local checks and a
separate six-phase desktop create/save/reopen workflow at 1200 × 800 / 1200 × 760.
Complete Project comparisons and saved/reopened body equality passed; workbook
execution was checked separately in local tests. Earlier collector-protocol
failures remain distinct from product behavior. Desktop workbook execution and
broader hierarchical/named setup are not claimed. Mapping-path desktop #108
remains open. Metadata lanes #93/#106 must observe
settings before changing interchange admission. #130/#163 now separately
qualify native nonfinite admission and original-byte accounting; neither observes
interchange metadata.

The #94 proposal merged in #129 with 224 independent cells: 48 Unverified,
176 Unassessed and no Supported promotion. The protected survey remains unchanged.
The source/link-reviewed #95 index and #122 numeric guide retain their earlier
rendering limitations. The four XML public-host cohorts #116–#119 are now merged;
#133/#147 records their exact gate ownership and workload-specific memory
observations in the [coverage index](generated-xml-qualification-index.md) and
[memory guide](memory-and-limits.md#compiled-xml-host-observations).
These cohorts do not establish external execution, general design coverage or a
universal RAM limit. Source reviews and emission passes remain separate from
compiled public calls.

Pure JSON5 syntax is merged in #140/#146, and the shared policy #141 with its
descriptor duplicate guard #148 is merged in #151. Explicit Rust and C#
companions are merged in #153/#155; CLI opt-in and the complete 184 small compiled
public calls are merged in #156. The 23 public cases comprise 17 representation
and six mapping/context cases, each across four routes in both languages. This
cohort is distinct from the 75-case pure syntax inventory. Ordinary strict JSON
APIs and default artifact sets stay unchanged. The complete 48 serial physical
calls in #145 are merged in #159 after a fresh complete small prerequisite
cohort: original input, normalized input and strict output exact/+1 workloads
across all eight public language/API routes. Full results/typed causes, source
and library guards, and normal process closure are retained. Whole-host RSS
includes startup, input handling, public mapping, output retention and host
checks; it establishes neither an API-only peak, a RAM ceiling nor streaming.

The [companion contract](generated-json5-contract.md) separates policy, syntax,
projection, mapping and strict output. Its #154 refresh is merged in
[#161](https://github.com/DeandreT/ferrule/pull/161), including all physical
boundary results and measured whole-host memory ranges. The unassigned
#160 benchmark/report lane will attribute generated C# allocation peaks through
matched profiling and uninstrumented controls, excluding production optimization
and the filesystem multi-output trials in #107. Direct-Program Rust retention
#157 is now merged in #168 after 110 emitter tests, three warnings-denied hosts
and 11 public calls; full validation order is preserved. Native nonfinite
admission #130 is merged in #164 after 190 controls and 58 CLI calls, preserving
the original nullable-reader failure and corrected outcomes. Original-byte/BOM
accounting #163 is merged in #167 after 45 small controls and eight physical
calls. These input-boundary checks make no streaming or total-memory promise.
Unicode identifiers #135 and native metadata #106 remain independent and
available. The initial #96 companion scope is complete and closed. Broader
identifier membership and native/desktop contracts remain separate.

The native #100 increment is merged in #170: seven focused groups and 388 JSON
tests pass, with one existing physical-resource opt-in test ignored. It admits
only the proved required-only object/object-null profile, retains each physical
resource's dialect and ordered presence, and bounds required lists and merged
sets to 256 within that subset. Unsupported shapes/assertions and undeclared new
names still refuse. Generated qualification #169 is merged in
[#174](https://github.com/DeandreT/ferrule/pull/174), completing #100's exact
native/generated acceptance. The finite cohort passes 183 native observations
and all 594 compiled calls: 297 each in Rust and C#, across nine profiles,
16 mappings and 66 cases. Each language passes its full [131, 117, 30, 16, 3]
call vector for generated JSON mappings, independent readers, ordinary typed
execution, typed writers and admission controls. The strict compiled error-order
witness passes on the native #100/#170 and runtime #171/#172 prerequisites.
All 23 owning CLI checks, scoped strict lint and formatting pass. Fourteen
harness controls retain complete originals, including 13 intentional mismatches
and a later valid control. Earlier compiler/nullability and authored expected-text
failures remain preserved separately from the final successful cohort.
The separate #171 direct-runtime correction passed 30 observations and the
affected native/Rust/C# suites. All three hosted #174 jobs completed as failures
without executing any validation step because of the account billing/spending-limit
restriction, as in #170/#172. Local verification, source review and compiled calls
remain distinct from hosted execution.

The #99 browser-history increment is merged in
[#178](https://github.com/DeandreT/ferrule/pull/178) after 43 web tests, fresh
WebAssembly and a separate finite actual browser review: 132 native commands,
34 native GUID-completed downloads and 32 public-helper matches. The two
original helper refusals are preserved; their compact draft originals were
separately qualified by complete-byte comparison. No campaign exit-0 or helper
pass is claimed for those two. The browser/server close calls fulfilled, then
remaining owned descendants needed cleanup before final closure was proven.
Earlier failures remain distinct from this bounded product acceptance. The
#165/#166 missing-binding controls remain prerequisites, without widening the
browser history claim to arbitrary editor or native-desktop parity.

The separate #177 exact coalesced baseline correction is merged in
[#179](https://github.com/DeandreT/ferrule/pull/179), with all 45 web tests, scoped
strict lint and formatting passing. It preserves the strict retained-byte ledger,
transient redo discard and distinct focus sessions; no fresh browser campaign
is claimed. Applied target titles #176 are merged in
[#181](https://github.com/DeandreT/ferrule/pull/181) after 45 web tests, fresh
WebAssembly and 24 actual native browser commands in wide/compact views, with
two complete typed and whole-byte-checked downloads. That owned browser run also
needed descendant cleanup before final closure was proven. Whole-node Fit #180
is merged in [#195](https://github.com/DeandreT/ferrule/pull/195): 54 unique web tests,
12 wide/compact Fit checkpoints, 66 browser commands and two complete verified
downloads pass. It changes only the view transform. Owned descendant cleanup
was required; normal closure was false and final closure was proven. Original
lint, startup and fixture failures remain retained.

The #101 exact String-or-Int IntegerRange contract is merged in
[#183](https://github.com/DeandreT/ferrule/pull/183). Three IR and four native
focused groups, 202 direct calls per language, all 179 C# smoke groups and all
152 compiled generated text/byte calls pass across seven mappings. Full typed,
serialized and refusal oracles cover primary and named boundaries. All 797
affected Rust checks, scoped strict warnings and formatting pass, with one
existing resource test ignored. The original native error-text oracle failure
and its source-reviewed correction remain retained. The optional full generated
catalog was explicitly stopped after early checks and owned cleanup; it is not
claimed passed. Three hosted jobs executed zero steps under the account billing
restriction. String assertions, enum/format behavior and strict Contains remain
separate from integer branch intervals; broader scalar unions are not admitted.
The #107 ordinary filesystem JSON study is merged in
[#185](https://github.com/DeandreT/ferrule/pull/185): eight normal-development-profile
measurements, eight separate verifiers and twelve complete output documents pass.
The [individual-run report](performance/json-output-set-memory.md) records
120.3–194.6 MiB whole-process peaks and all four matched second-output increases:
31.2–39.4 MiB. Both repetitions, full typed/serialized expectations, original
phase/process observations and normal closure remain recorded. The earlier
numeric-parser source finding and its bounded correction are preserved; all 45 numeric
controls pass. Three hosted jobs executed zero validation steps because of the
account billing/spending restriction. Two workloads and two repetitions do not
establish statistical significance, streaming, allocation-exclusive peaks or a
universal RAM cap. The separate #160 generated allocation scope is unchanged.

The ordinary JSON writer #184 is merged in
[#200](https://github.com/DeandreT/ferrule/pull/200). Its ordered private view borrows
ordinary target text and member names rather than constructing a complete owned
`serde_json::Value` tree before pretty rendering. The returned String, APIs,
order, null behavior, coercion, validation/errors and publication stay intact;
container metadata, arbitrary JSON and validation snapshots can still allocate.
All 398 scoped regressions and 126 complete writer-comparison calls pass.
The [matched report](performance/borrowed-json-output-memory.md) preserves the
historical #107 baseline and eight fresh original/candidate fixture comparisons,
with 16 separate verifiers and 24 complete documents. Whole-process peaks are
8.7–40.4 MiB lower for these fixtures only; source/layout/startup/cache differences
limit attribution. No streaming, exclusive allocation saving or RAM cap follows.

The reviewed [scalar sequence design](design/scalar-sequence-composition.md) and
[all 39 proposed literals](design/fixtures/scalar-filter-map-v1.json) are merged
in #102/[#188](https://github.com/DeandreT/ferrule/pull/188). These are 37 feature
cases and two legacy controls; approving a design does not execute them. Model,
codec and private-owner admission #189/#197 pass 1,759 distinct scoped regressions.
Native execution #190/#202 separately passes all 37 feature cases, two legacy
controls and 1,011 distinct regressions. Common lowering #191/#204 passes 62
complete comparisons and 497 library tests, retaining predicate/mapper roots and
transitive UDF callees. Broader nested/higher-order shapes and failure-rule
transform authoring remain outside the initial contract.

Rust #192/#206 passes 188 whole public calls and 30 separately labeled direct
controls across 40 compiled profiles plus seven static/decode controls, with 365
library tests and seven local gates. C# #193/#208 passes 172 public calls across
43 profiles, 35 direct controls, 432 library tests, 179 C# smoke groups and eight
local steps. Complete values, positions, limits, checker boundaries and typed
causes retain their finite public/direct observability scopes. A failed public
mapping does not expose a partial output. Original host/compiler/lint and fixture
failures remain distinct from the corrected cohorts. These results are not GUI,
external-interchange or whole-product qualification.

Ordinary/nested UDF argument ordering #186 is merged in
[#196](https://github.com/DeandreT/ferrule/pull/196): evaluate all raw arguments
left to right before adaptation; all 18 compiled calls per language match frozen
expectations after the original four mismatches per language. Interpreter,
coercion, frame and legacy sequence policy remain unchanged. The separate legacy
Rust aggregate helper-order fix #199 is merged in
[#209](https://github.com/DeandreT/ferrule/pull/209), with 15 independent native
controls, 45 compiled typed/text/byte calls, 114 Rust tests and six local steps.
Its original generated compiler mismatch was observed before the correction.
Earlier hosted sequence jobs did not start under the account billing/spending
restriction; no local result is a hosted pass. Hosted execution later resumed.
All three jobs succeed for #228/#229/#236/#238/#240/#243/#244/#245/#249/#251,
#255/#262/#265/#266/#270 at this snapshot. Open #278 has three running jobs;
local #253/#277 checks are not hosted success. #222/#226/#230/#231 retain reached mapping-job failures. #249 merged
at 00:07:26 UTC and #251 at 00:16:28 UTC on 2026-10-10. Successful site builds do not close the Pages
HTTP 404 tracked independently in [#247](https://github.com/DeandreT/ferrule/issues/247).

Compact GUI #194 and guidance #210 are merged in
[#214](https://github.com/DeandreT/ferrule/pull/214). The final 21-role source and
bounded fixture/style corrections are independently accepted. Earlier checks pass
20 focused tests, the complete 742-test GUI suite and 31 web-demo crate tests;
those crate checks are distinct from actual browser UI. Final source passes fresh
strict warnings after 454 required source timestamp refreshes, formatting, one
affected unit check already included in the 742-test suite, and the fresh production build.
Repeated checks do not increase the distinct totals. Earlier compiler, fixture,
strict-lint, timeout and chooser/registration failures remain preserved.

Finite desktop parts observe current-item properties at 1200 × 900 and scope
selection from 900 × 700 to 1200 × 900. Both exit 0 with owned helper cleanup,
which is separate from normal cleanup-free closure. Ordinary editor-driven
identity Rust and C# library generation also exits 0 with owned helper cleanup;
One public String JSON call from each editor-generated identity library matches
the complete independently authored 172-byte golden. The five host commands exit 0
with normal owned closure and no cleanup; this cannot replace the earlier GUI
cleanup observations. All three hosted #214 jobs executed zero validation steps
because of the account billing/spending restriction. These finite observations
do not qualify all 13 physical workflows or all 39 sequence design cases. Constant-7
controls do not prove a real parent frame. Independent document copies are in
scope; same-project duplication is #201.

Desktop Fit #213 is merged in [#238](https://github.com/DeandreT/ferrule/pull/238).
Nine focused Fit tests plus the popup regression, the full 749 GUI tests and fresh
strict/fmt/production build checks pass. Four finite alternate-display parts cover
main/named/single/empty canvases at 900 × 700 and 1200 × 900, preserving complete
saved Project/layout files. GUI process closure is normal; owned private bus
helpers need cleanup. These parts do not qualify every desktop authoring workflow
or browser Fit. Source/model/history and factories/icons remain unchanged.

The earlier two-document refreshes #158/#162, #173/#175, #182/#187 and #212/#215
remain dated records. #212's revised two fences have two SVGs and four PNGs under
the fixed bounds, with owned helper cleanup; its complete map needs zoom. The
unchanged roadmap fence retains its earlier rendered evidence. #252/#255 also
completed its bounded review of the two changed parallel fences. This #271 refresh
preserves every fence body and its historical rendering/zoom/cleanup limits.
The newer ownership and prerequisite relationships are stated in the table and
prose; no fresh or complete-label visual inspection is claimed. No source/link/
diagram check establishes additional product execution.

## Next acceptance gates

- [x] Complete #194's earlier 20 focused/742 GUI/31 web-demo crate checks and final fresh
  strict warnings, formatting, affected unit and production build gates; retain
  original failures and do not double-count repeated checks.
- [x] Observe finite desktop current-item properties and responsive scope selection,
  and generate identity Rust/C# libraries through the editor; owned helper cleanup
  was needed and the broad physical workflow scope is not complete.
- [x] Qualify the two editor-generated identity String JSON host calls against the
  complete 172-byte independent golden, with five normal exit-0 commands and
  cleanup-free owned host closure. Broader saved-input pointer/keyboard, Preview,
  save/reload and history coverage remains separate from this finite increment.
- [x] Qualify #198's 90 native/Rust/C# typed-selection comparisons and owning
  regressions, with complete static admission, global rules once, lazy selected
  loaders and counters; #205's shared C# test-source release is preserved.
- [ ] Qualify #201's accepted atomic private-owner proposal: all 67 native calls and
  alternate-display duplication/save/history checks remain unrun. Its 28-control
  contract and complete canvas snapshots are frozen; preserve #213's Fit hooks.
  Disk admission for the sole shared GUI verification lane is still pending.
- [x] Materialize and qualify #203's 24 public interpreter observations and 51
  separate typed export refusals; all 237 interchange library tests, strict
  warnings and formatting pass on the fixture candidate. #264's test-only parent
  envelope correction preserves frozen oracles and the original case-8 failure.
- [x] Merge fixture publication #266 after fresh combined-main 24+51 / strict /
  format checks; all three hosted jobs succeed and #264 is closed. External
  save/reopen and faithful-form admission
  remain separate from public Ferrule observations and typed export refusal.
- [x] Qualify #263's 41 simulated controller-policy controls, retaining complete
  exceptions, copies, refusal ordering and exact saved1-path branch. Simulation
  and external owner closure do not establish any real native application call.
- [ ] Qualify #268's source-reviewed complete XML oracles through its new 68 helper,
  17 controller-policy and 41 compatibility controls, then #263's individually
  admitted native cycle. All new calls remain unrun; earlier #263 simulated 41
  evidence stays separate, and no saved-form seed is inferred.
- [x] Qualify #253's focused engine unit controls against #203's published frozen
  fixture: three tests / 11 complete internal observations, all 344 engine tests,
  ordinary engine build and corrected workspace strict/format checks pass locally.
  Public-host observations and borrowed internal records stay separate; construction
  reach is not allocator telemetry and no production getter is introduced.
- [ ] Publish #253 with the single test-helper let-chain prerequisite #277 in open
  [#278](https://github.com/DeandreT/ferrule/pull/278); hosted jobs remain pending.
  The original strict-warning failure and both complete focused cohorts are retained;
  repeated runs do not add distinct cases.
- [x] Execute #205's five matched tests and two deliberate comparison-failure
  controls. Full exposed originals remain equal; raw logs are 137,823,450 versus
  124,980 bytes and retained files 876,481 versus 6,110,354 bytes. Two admission logs
  grow. No memory saving or unrecorded old diagnostic detail is inferred.
- [x] Freeze and execute #207's 72 measured native/Rust/C# runs and 72 separate
  complete verifiers. The [report](performance/scalar-sequence-memory.md) retains
  two repetitions, 36 matched pairs and raw/sparse whole-process counter limits.
  Linux guards #242 and composed-workspace packaging #246 are qualified in the
  same increment, merged in #249; all three hosted jobs succeed.
- [x] Complete #248's eight measurements, eight independent output verifiers and
  four trace decoders using #207's merged fixtures, retaining both repetitions.
- [ ] Publish #248's bounded report. Source/data and one full rendered diagram are
  independently reviewed; the 100% viewport requires scrolling. Browser-helper
  cleanup was needed; closure is proven, with normal cleanup-free closure false.
  Samples/stacks are not an exact object ledger, RSS is not allocation attribution,
  and no exact phase clock or capture cause is inferred.
- [x] Qualify #211's same 20 controls and full outcomes; logs change from 538.6 MB
  to 23.6 MB (95.6%). Production behavior and process memory are separate.
- [x] Publish #212's dated two-doc/link/diagram refresh in #215. Historical rendered
  diagrams retain owned cleanup and zoom limitations.
- [x] Qualify #213's complete-rectangle Fit in nine focused tests plus the popup
  regression, all 749 GUI tests and four finite alternate-display main/named/single/
  empty views at 900 × 700 / 1200 × 900; saved Project/layout files remain exact.
- [x] Qualify #217's nine helper controls, three paired regressions, three default
  cleanup repeats and three configuration refusals with one compatible cache.
  Merged in #251 after #249, with all three hosted jobs successful; this does not
  change production behavior.
- [x] Execute #218's frozen 40-recipe/229-call selected JSON contract: 76 native,
  76 Rust and 77 C# calls pass, merged in #262. Actual public returns precede
  separately decoded-wire observations. Preserve the original 15 comparator
  mismatches and the qualified test-only #257 correction.
- [x] Merge the independently source/render-reviewed #261 guide in #270; all three
  hosted jobs and fresh combined-main strict/format checks pass, and #261 is closed.
- [x] Qualify #250's bounded generated C# 004010 options, merged in #265; supplied
  contexts, strict defaults and ordinary typed/JSON APIs remain preserved.
- [ ] Settle #241's explicit competing-feed contract before admission and execution.
- [ ] Diagnose #247's actual Pages 404 and verify a successful deployed artifact;
  successful site compilation alone is insufficient.
- [x] Publish #252's dated two-doc/source/link/render review in #255, with three
  hosted jobs successful; historical diagram cleanup and zoom limits remain.
- [ ] Independently review and publish #271's main fef16c3 two-doc status refresh.
  Existing fences stay byte-exact; no new rendering or product qualification is inferred.

## Dependencies and shared areas

A solid arrow below is a prerequisite for the labeled checks. A dotted arrow
means coordinate shared source or verification resources; it does not block
independent design/source work. In particular, generated JSON5 adapters and
native JSON5 metadata are separate contracts, and XML annotations do not have to
finish before JSON5 observation can begin. #100/#170 and #171/#172 are
prerequisites for #169's compiled mapping checks. #99 is the retained history
baseline for #177; #176 precedes #180's applied-title Fit checks. #101 closes only
its exact native/generated profile; #107 supplies #184's matched measurement baseline.
#89 and dated status refreshes coordinate work rather than gate implementation.

The merged #102 contract precedes #189's buildable model/guards, then #190 native
execution and #191 common lowering. #192/#193 separately qualify their languages
on that baseline and the #186 UDF-order policy; #194 native authoring needs the model
and native engine, while backend actions need their respective qualified emitter.
#198's typed-selection qualification is merged independently of GUI implementation.
Its shared C# test-source handoff from #205 preserves complete artifact/error
capture; that release edge is not another backend qualification. #199's predicate
correction remains intact. #218 builds on #198's typed routes; #207/#217 are merged
and release their shared source. #218's 229-call qualification and publication
are complete. The test-only #257 correction is part of that qualification;
#218 → #257 and #218/#257 → #261 describe the guide's prerequisite chain.
#248 uses #207's published fixtures for its accepted finite analysis campaign;
report publication and any later C# runtime optimization remain separate.

#201's contract/source work is independent, but final GUI adoption preserves the
released #194 and #213 hooks; native/desktop qualification awaits disk admission.
#264 → #203 is the test-fixture prerequisite for the accepted public 24+51 cohort;
it does not alter production position semantics. #203's design admits no faithful
new saved form and does not edit execution/GUI files. #268 → #263 supplies complete
independent XML checkpoint oracles before any native cycle; #268's new 68 helper,
17 policy and 41 compatibility controls and the real #263 native cycle remain unrun. #263's accepted simulated
41 controls do not discharge either dependency or admit a filter/map seed. #211 and #213 have completed their separate test-only/view-only increments.
#253's internal engine-unit lane depends on #203's published frozen fixture and
keeps complete internal observations separate from public saved-design results.
Its narrow test-only sink changes no public API, budgets or execution policy.
The explicit #277 → #253 publication dependency is the single let-chain condition
in the capture test helper; both are locally qualified in open #278, with hosted
jobs and merge pending. It is a prerequisite, not shared-resource coordination.
#217's merged PR followed #207's publication because they share a parent/source handoff,
not because memory results supply a host-policy contract. #242/#246 are bounded
parts of #207. #250/#265 is merged on the admitted raw X12 #221 and native ISA #224 boundaries;
#241 distinguishes competing feeds from #239's declared ancestor branches.
#233 metadata inspection and #103's typed query contract remain separate.

#212/#215 and #252/#255 are dated documentation increments. #271 refreshes only
those two files against merged #218/#262, #250/#265, #203's fixture #266 and guide
#261/#270 on main fef16c3. It records open #278 and pending #248 report publication
without inferring later completion. The Pages
#247 workflow repair is independent of mapping implementation and these docs.
The existing diagrams are preserved historical dependency views from #252;
their solid arrows are prerequisites and dotted arrows coordinate shared files
or resources without preventing independent source work.

The compact view below remains the earlier #252 graph. The current table and
explicit prose dependencies above are authoritative for the additional lanes;
the historical graph is not a current completion map. #253 depends on
#203's frozen fixture, whose design lane is outside this compact view; that edge
is explicit in the complete map below.

```mermaid
flowchart LR
    G189["#189 Model"] --> G190["#190 Native"]
    G190 --> G191["#191 Lowering"]
    G191 --> G192["#192 Rust"]
    G191 --> G193["#193 C#"]
    G186["#186 UDF order"] --> G192
    G186 --> G193
    G189 --> G194["#194 Editor"]
    G190 --> G194
    G192 --> G194
    G193 --> G194
    G190 --> G198["#198 Selection"]
    G192 --> G198
    G193 --> G198
    G192 --> G199["#199 Rust predicate"]
    G192 --> G205["#205 Evidence"]
    G193 --> G205
    G205 --> G198
    G194 --> C212["#212 Docs"]
    G194 --> G213["#213 Desktop Fit"]
    G194 -.-> G198
    G194 -.-> G205
    G190 --> G207["#207 Memory"]
    G192 --> G207
    G193 --> G207
    G207 --> G217["#217 Host policy"]
    G198 --> G218["#218 Selected JSON"]
    G207 --> G218
    G217 --> G218
    G207 --> G248["#248 Allocation trace"]
    G218 -.-> G248
    G194 --> G201["#201 Duplication"]
    G213 --> G201
    G194 --> G211["#211 GUI diagnostics"]
    C212 -.-> C252["#252 Snapshot"]
    G207 --> C252
    G217 --> C252
    G253["#253 Internal observations"]
```

<details>
<summary>Complete issue dependency map (93 issues)</summary>

Labels in the complete map identify each issue and topic; status and ownership
remain in the table above. All prerequisite and coordination links are retained.

`needs` marks a prerequisite; `share` marks source or resource coordination.
The table and dependency prose above describe the scopes. Full original arrow
descriptions are retained beside their arrows in the diagram source.

```mermaid
flowchart TB
    C89["#89 Coordination"]
    C115["#115 Status"]
    C122["#122 Numeric docs"]
    C133["#133 XML docs"]
    C149["#149 Status"]
    C154["#154 JSON5 guide"]
    C158["#158 Status"]
    C173["#173 Docs"]
    C182["#182 Docs"]
    C212["#212 Snapshot docs"]
    C252["#252 Current docs"]
    subgraph gui["GUI and browser"]
        G90["#90 Properties"]
        G97["#97 Typed cells"]
        G98["#98 Transposed setup"]
        G99["#99 Browser history"]
        G165["#165 Missing bindings"]
        G176["#176 Target titles"]
        G177["#177 History baseline"]
        G180["#180 Browser Fit"]
        G194["#194 Sequence editor"]
        G201["#201 Duplication"]
        G210["#210 Generation help"]
        G211["#211 GUI diagnostics"]
        G213["#213 Desktop Fit"]
    end
    subgraph generated["Generated execution"]
        G91["#91 Conversion"]
        G111["#111 Float precision"]
        G112["#112 JSON rounding"]
        G113["#113 JSON null"]
        G114["#114 Absent defaults"]
        G95["#95 XML index"]
        G116["#116 XML input"]
        G117["#117 XML output"]
        G118["#118 XML load count"]
        G119["#119 XML errors"]
        G157["#157 Rust emission"]
        G169["#169 Compiled checks"]
        G171["#171 Presence errors"]
        G186["#186 UDF order"]
        G192["#192 Rust sequences"]
        G193["#193 C# sequences"]
        G198["#198 Typed selection"]
        G199["#199 Rust predicate"]
        G205["#205 Artifact evidence"]
        G217["#217 Host policy"]
        G218["#218 Selected JSON"]
        G220["#220 Nightly lint"]
        G221["#221 C# raw X12"]
        G223["#223 Test inventory"]
        G225["#225 Test retention"]
        G250["#250 X12 options"]
    end
    subgraph json5["Explicit generated JSON5 companions"]
        G96["#96 JSON5 scope"]
        G134["#134 Rust JSON5 syntax"]
        G136["#136 C# JSON5 syntax"]
        G148["#148 Duplicate guard"]
        G141["#141 Eligibility"]
        G142["#142 Rust JSON5 API"]
        G143["#143 C# JSON5 API"]
        G144["#144 Small calls"]
        G145["#145 Physical calls"]
        G135["#135 Unicode names"]
    end
    subgraph schema["Schema interoperability"]
        G94["#94 Object schemas"]
        G100["#100 Reference subset"]
        G101["#101 Union ranges"]
    end
    subgraph design["Model and host contracts"]
        G102["#102 Sequence contract"]
        G189["#189 Sequence model"]
        G190["#190 Native sequences"]
        G191["#191 Common lowering"]
        G203["#203 Saved designs"]
        G253["#253 Internal observations"]
        G207["#207 Sequence memory"]
        G103["#103 SQLite boundary"]
        G104["#104 PDF isolation"]
        G105["#105 Offline bundle"]
        G107["#107 Memory baseline"]
        G160["#160 C# allocation"]
        G184["#184 JSON writer"]
        G242["#242 Linux guard"]
        G246["#246 Study workspace"]
        G248["#248 Allocation trace"]
        G241["#241 Multiple feeds"]
        G237["#237 Variadic logic"]
        G239["#239 Clone owners"]
    end
    subgraph formats["Format and publication boundaries"]
        G224["#224 ISA encoding"]
        G227["#227 Publish diagnostics"]
        G233["#233 SQLite metadata"]
        G234["#234 Resource roots"]
        G235["#235 Import admission"]
        G247["#247 Pages deployment"]
    end
    subgraph observed["Desktop, native metadata and JSON5 input"]
        G92["#92 Workbook save"]
        G93["#93 XML annotations"]
        G106["#106 JSON5 metadata"]
        G108["#108 Mapping paths"]
        G130["#130 Token admission"]
        G163["#163 Original bytes"]
    end
    C89 -.-> G94
    C89 -.-> G90
    C89 -.-> G102
    C89 -.-> G107
    C89 -.-> G184
    C89 -.-> G186
    C89 -.-> G92
    C89 -.-> C115
    C89 -.-> C122
    C89 -.-> C149
    C89 -.-> C158
    C89 -.-> C173
    C89 -.-> C182
    C89 -.-> G180
    %% original:     C158 -.->|Same two docs; later snapshot| C173
    C158 -.->|share| C173
    %% original:     G169 -.->|Separate source; coordinate status| C173
    G169 -.->|share| C173
    %% original:     G99 -.->|Separate source; coordinate status| C173
    G99 -.->|share| C173
    %% original:     C173 -.->|Same two docs; later snapshot| C182
    C173 -.->|share| C182
    %% original:     G99 -.->|Finite browser evidence status| C182
    G99 -.->|share| C182
    %% original:     G176 -.->|Applied-title evidence status| C182
    G176 -.->|share| C182
    %% original:     G177 -.->|Local correction status only| C182
    G177 -.->|share| C182
    %% original:     G180 -.->|Earlier active snapshot retained| C182
    G180 -.->|share| C182
    %% original:     G101 -.->|Exact native/generated completion| C182
    G101 -.->|share| C182
    %% original:     G107 -.->|Qualified memory baseline status| C182
    G107 -.->|share| C182
    %% original:     G102 -.->|Earlier design-only snapshot retained| C182
    G102 -.->|share| C182
    %% original:     G184 -.->|Earlier available snapshot retained| C182
    G184 -.->|share| C182
    %% original:     G186 -.->|Earlier source-gap snapshot retained| C182
    G186 -.->|share| C182
    %% original:     C149 -.->|Same two docs| C158
    C149 -.->|share| C158
    %% original:     C154 -.->|Separate guide; coordinate status| C158
    C154 -.->|share| C158
    %% original:     C115 -.->|Same two docs| C149
    C115 -.->|share| C149
    %% original:     C122 -.->|Separate docs files| C149
    C122 -.->|share| C149
    %% original:     C133 -.->|Separate docs files| C149
    C133 -.->|share| C149
    %% original:     G95 -->|Host guide follows merged index| C122
    G95 -->|needs| C122
    %% original:     G111 -->|Merged precision contract| C122
    G111 -->|needs| C122
    G112 --> C122
    %% original:     G111 -.->|One precision increment| G112
    G111 -.->|share| G112
    G111 --> G91
    G112 --> G91
    G111 --> G114
    G112 --> G114
    G91 --> G113
    %% original:     G91 -->|Isolated conversion checks| G97
    G91 -->|needs| G97
    %% original:     G114 -->|Default persistence checks| G97
    G114 -->|needs| G97
    G116 --> C133
    G117 --> C133
    G118 --> C133
    G119 --> C133
    G92 -.-> G98
    %% original:     G165 -->|Preserve missing-binding controls| G99
    G165 -->|needs| G99
    %% original:     G99 -->|Retained history baseline| G177
    G99 -->|needs| G177
    %% original:     G99 -.->|Shared web app; separate label correction| G176
    G99 -.->|share| G176
    %% original:     G176 -->|Applied titles before Fit workflow| G180
    G176 -->|needs| G180
    %% original:     G177 -.->|Preserve history tests; shared display/build| G180
    G177 -.->|share| G180
    %% original:     G100 -->|Merged native subset before compiled checks| G169
    G100 -->|needs| G169
    %% original:     G171 -->|Merged presence-first runtime before compiled checks| G169
    G171 -->|needs| G169
    G100 -.-> G101
    %% original:     G101 -.->|Shared build lane; separate source| G107
    G101 -.->|share| G107
    %% original:     G107 -->|Retained baseline before matched trials| G184
    G107 -->|needs| G184
    %% original:     G101 -.->|Shared format-json crate; separate writer scope| G184
    G101 -.->|share| G184
    %% original:     G102 -.->|Separate contract; preserve qualified UDF policy| G186
    G102 -.->|share| G186
    G101 -.-> G96
    G95 -.-> G96
    G95 -.-> G116
    G95 -.-> G117
    G95 -.-> G118
    G95 -.-> G119
    G96 -.-> G141
    G134 --> G142
    G136 --> G143
    %% original:     G148 -->|Before policy qualification and merge| G141
    G148 -->|needs| G141
    %% original:     G141 -->|Before adapter qualification| G142
    G141 -->|needs| G142
    %% original:     G141 -->|Before adapter qualification| G143
    G141 -->|needs| G143
    %% original:     G142 -.->|Shared contract; separate language files| G143
    G142 -.->|share| G143
    G142 --> G144
    G143 --> G144
    %% original:     G144 -->|All small calls before any large call| G145
    G144 -->|needs| G145
    %% original:     G142 -->|Public API guide claims| C154
    G142 -->|needs| C154
    %% original:     G143 -->|Public API guide claims| C154
    G143 -->|needs| C154
    %% original:     G144 -->|Compiled small-cohort status| C154
    G144 -->|needs| C154
    %% original:     G145 -->|Resource status only| C154
    G145 -->|needs| C154
    %% original:     G145 -->|Resource row only| C158
    G145 -->|needs| C158
    %% original:     G142 -.->|Shared Rust emitter; separate scope| G157
    G142 -.->|share| G157
    %% original:     G145 -.->|Shared workload/evidence; no optimization| G160
    G145 -.->|share| G160
    %% original:     G143 -.->|Shared C# boundary; profiling only| G160
    G143 -.->|share| G160
    G135 -.-> G96
    %% original:     G134 -->|Frozen Rust syntax prerequisite| G135
    G134 -->|needs| G135
    %% original:     G136 -->|Same C# syntax contract| G135
    G136 -->|needs| G135
    G93 -.-> G106
    G92 -.-> G108
    G93 -.-> G108
    G106 -.-> G130
    %% original:     G130 -->|Finite admission before overlapping byte seam| G163
    G130 -->|needs| G163
    C89 -.-> C212
    %% original:     C182 -.->|Same two docs; later snapshot| C212
    C182 -.->|share| C212
    %% original:     G184 -.->|Finite writer report status| C212
    G184 -.->|share| C212
    %% original:     G186 -.->|Qualified UDF order status| C212
    G186 -.->|share| C212
    %% original:     G194 -->|Fresh verified snapshot before docs publication| C212
    G194 -->|needs| C212
    %% original:     G198 -.->|Source-only status| C212
    G198 -.->|share| C212
    %% original:     G205 -.->|Test-only source status| C212
    G205 -.->|share| C212
    %% original:     G102 -->|Approved contract before model| G189
    G102 -->|needs| G189
    %% original:     G189 -->|Model and ownership before native execution| G190
    G189 -->|needs| G190
    %% original:     G190 -->|Qualified native contract before lowering adoption| G191
    G190 -->|needs| G191
    %% original:     G191 -->|Common stage roots before Rust execution| G192
    G191 -->|needs| G192
    %% original:     G191 -->|Common stage roots before C# execution| G193
    G191 -->|needs| G193
    %% original:     G186 -->|Qualified ordinary UDF order| G192
    G186 -->|needs| G192
    %% original:     G186 -->|Qualified ordinary UDF order| G193
    G186 -->|needs| G193
    %% original:     G189 -->|Model ownership before authoring| G194
    G189 -->|needs| G194
    %% original:     G190 -->|Native Preview capability| G194
    G190 -->|needs| G194
    %% original:     G192 -->|Qualified Rust generation action| G194
    G192 -->|needs| G194
    %% original:     G193 -->|Qualified C# generation action| G194
    G193 -->|needs| G194
    %% original:     G192 -->|Typed Rust execution before selection| G198
    G192 -->|needs| G198
    %% original:     G193 -->|Typed C# execution before selection| G198
    G193 -->|needs| G198
    %% original:     G190 -->|Native selection controls| G198
    G190 -->|needs| G198
    %% original:     G194 -.->|Shared verification lane; source independent| G198
    G194 -.->|share| G198
    %% original:     G199 -.->|Shared Rust emitter; preserve correction| G198
    G199 -.->|share| G198
    %% original:     G192 -->|Qualified runtime before legacy predicate checks| G199
    G192 -->|needs| G199
    %% original:     G189 -->|Private-owner contract before duplication| G201
    G189 -->|needs| G201
    %% original:     G190 -->|Native counterpart before duplicate Preview| G201
    G190 -->|needs| G201
    %% original:     G194 -->|Shared GUI files released before adoption| G201
    G194 -->|needs| G201
    %% original:     G102 -->|Frozen sequence contract| G203
    G102 -->|needs| G203
    %% original:     G189 -->|Exact descriptors| G203
    G189 -->|needs| G203
    %% original:     G190 -->|Native semantics for interchange qualification| G203
    G190 -->|needs| G203
    %% original:     G203 -.->|Shared alternate display only| G93
    G203 -.->|share| G93
    %% original:     G192 -->|Qualified Rust test baseline| G205
    G192 -->|needs| G205
    %% original:     G193 -->|Qualified C# test baseline| G205
    G193 -->|needs| G205
    %% original:     G194 -.->|Shared build lane; separate test files| G205
    G194 -.->|share| G205
    %% original:     G205 -->|Release shared C# json5_output_tests.rs before selected-target test rebase| G198
    G205 -->|needs| G198
    %% original:     G190 -->|Native measurement capability| G207
    G190 -->|needs| G207
    %% original:     G192 -->|Rust measurement capability| G207
    G192 -->|needs| G207
    %% original:     G193 -->|C# measurement capability| G207
    G193 -->|needs| G207
    %% original:     G107 -.->|Separate fixtures; coordinate measured build lane| G207
    G107 -.->|share| G207
    %% original:     G192 -->|Qualified capability before guidance| G210
    G192 -->|needs| G210
    %% original:     G193 -->|Qualified capability before guidance| G210
    G193 -->|needs| G210
    %% original:     G210 -.->|Same editor; one cohesive increment| G194
    G210 -.->|share| G194
    %% original:     G194 -->|New frame helper released before adoption| G211
    G194 -->|needs| G211
    %% original:     G205 -.->|Distinct helpers; coordinate evidence policy| G211
    G205 -.->|share| G211
    %% original:     G194 -->|Shared GUI view-transform files released before adoption| G213
    G194 -->|needs| G213
    %% added:     G213 -->|Preserve released Fit hooks before GUI adoption| G201
    G213 -->|needs| G201
    %% added:     G207 -->|Publication parent/source release before PR251| G217
    G207 -->|needs| G217
    %% added:     G198 -->|Qualified typed selection APIs| G218
    G198 -->|needs| G218
    %% added:     G207 -->|Release shared emitter/build lane| G218
    G207 -->|needs| G218
    %% added:     G217 -->|Release shared CLI host helpers| G218
    G217 -->|needs| G218
    %% added:     G207 -->|Published matched fixtures before allocation analysis| G248
    G207 -->|needs| G248
    %% added:     G218 -.->|Coordinate later C# runtime edits and serial profiling lane| G248
    G218 -.->|share| G248
    %% added:     G207 -->|Study example owns platform guard| G242
    G207 -->|needs| G242
    %% added:     G207 -->|Study preparer owns workspace composition| G246
    G207 -->|needs| G246
    %% added:     G221 -->|Qualified bounded raw X12 baseline| G250
    G221 -->|needs| G250
    %% added:     G224 -->|Native ISA encoding baseline| G250
    G224 -->|needs| G250
    %% added:     G239 -.->|Distinct ancestor ownership versus competing singular feeds| G241
    G239 -.->|share| G241
    %% added:     G233 -.->|Read-only metadata versus typed query contract| G103
    G233 -.->|share| G103
    %% added:     G198 -->|TargetSelection filename in test inventory| G223
    G198 -->|needs| G223
    %% added:     G220 -->|Reached hosted inventory failure after lint repair| G223
    G220 -->|needs| G223
    %% added:     G223 -.->|Three shared XML wrapper files| G225
    G223 -.->|share| G225
    %% added:     G234 -.->|MFD resource-reader and executable-admission coordination| G235
    G234 -.->|share| G235
    %% added:     G237 -.->|Separate logical catalog and target-clone semantics| G239
    G237 -.->|share| G239
    %% added:     G198 -.->|Separate C# selected API and optional raw-X12 modules| G221
    G198 -.->|share| G221
    %% added:     C212 -.->|Same two docs; later dated snapshot| C252
    C212 -.->|share| C252
    %% added:     G207 -->|Bind final memory-study publication| C252
    G207 -->|needs| C252
    %% added:     G217 -->|Bind final host-policy publication| C252
    G217 -->|needs| C252
    %% added:     C89 -.->|Independent workflow lane; no mapping prerequisite| G247
    C89 -.->|share| G247
    %% added:     G203 -->|Frozen synthetic fixture before internal unit observations| G253
    G203 -->|needs| G253
```

</details>

Shared-file changes need explicit coordination even when the issues are otherwise
independent. Examples include app/workspace and test registrations, the workbook
draft, the JSON Schema facade, generated runtime/adapter modules, and CLI input
or artifact staging. Record a bounded file/function ownership split or sequence
the edits; preserve the other issue's complete behavior and tests.

## Verification resources

- Reserve one build lane when contributors share Cargo artifacts. Check disk
  space before and after substantial builds, reuse compatible caches, and use
  bounded jobs and incremental settings. See [Development](development.md#build-space-and-desktop-isolation).
- Reserve graphical or external-application sessions separately. Use an alternate
  display and an isolated chooser/session route; never drive another contributor's
  process or the user's active desktop. Bind the executable to the tested source
  and retain owned process closure, including failed attempts.
- Keep source review, focused local tests, generated public-host calls, normal
  desktop interactions, external execution and memory trials distinct. A source
  review or an emission pass does not close an execution gate.
- Measurements need their own finite input/profile/trial plan and full output
  checks. An observed peak is not a universal RAM limit or a streaming promise.

## Complete or hand off

A PR should describe the concrete resulting behavior, link the issue and record
which acceptance checks ran, with evidence or a precise blocker for checks still
open. Update only the corresponding roadmap/conformance gate after its required
evidence exists. Preserve earlier failures and narrower supported profiles.

The [conformance inventory](../conformance/README.md) remains incomplete. Do not
rewrite the protected survey as part of ordinary feature or coordination work,
mark unknown cells supported, or infer whole-product parity from a local cohort.
The coverage index in #95 keeps helper-only and compiled public-route evidence
separate; #133 records the completed #116–#119 slices without closing every XML
gap. Design-only #102 ends with an agreed follow-on plan rather than unreviewed
code.
