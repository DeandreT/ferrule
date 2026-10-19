# Runtime-only R recipes — no public Project claims

These remain the separate component lane from contracts/CONTRACT-CORRECTED.md §7.
This packet implements and executes **zero** R tests. Runtime implementers must
author them under a separate grant; a public Project cannot manufacture private
cohort/witness coverage. Reuse the authenticated fixtures in Rust
`crates/codegen-runtime/src/context/basic_group_adjacent_by_tests.rs` and C#
`runtime/csharp/Ferrule.Runtime.SmokeTests/BasicOwnershipGroupAdjacentByTests.cs`.

| ID | Independent literal frontier |
|---|---|
| R01 | Authentic empty original, two bounds: metadata1,bounds[w0,w1],semantic/target0,empty output. |
| R02 | Foreign-registry empty and cloned-parent empty: RegistryMismatch / InvalidCohortWitness(input0), before metadata/semantics. |
| R03 | Duplicate IDs, incomplete/reordered original and corrupt terminal separately; exact original fields,semantic0. Preserve Rust terminal-after-metadata and C# Seed terminal-before-metadata distinction. |
| R04 | IDs0/1/2, boundary F/T/F,k2,h2 First3/Last3,identity sort,filter/post true: metadata1,filter3,sort6,bounds2,boundary3,post3,target2. Log s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,b1,p1,f2,b2,p2,t0,t1. |
| R05 | FilterThenSort,keep IDs0/2: f0,f1,f2,s00,s01,s20,s21,w0,w1,b0,p0,b2,p2,t0; one run IDs0/2. |
| R06 | SortThenFilter,keep IDs0/2: s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,f2,b2,p2,t0. |
| R07 | All boundaries true: three distinct immutable C# publications; earliest remains ID0/member1/group1. Inspect retained unused Null n-key backing and all fixed capacities. |
| R08 | First0 with R04: same candidate/boundary/post work,target0,storage still admitted for all candidates. |
| R09 | Rust Rows singleton known text12+template20=32: B31 used12/request20/max31 before semantics; B32 boundary1/post1. Two rows F/F:24+40=64; B63 used24/request40/max63,B64 boundary2/post2/target1. No Boolean key bytes. |
| R10 | Rust root R1/F1/P0/B0/W1,capacity4,k2,h48,nonstring sort keys: Seed38 + fixed/template/selection28 + windows144 =210/max210. Subtotal28 is k2+nk2+8n+3+T7+U6. Capacity5: MetadataSlots used39/request172/max210,semantic0. |
| R11 | Rust Rows singleton B31/W1,h118,capacity4 versus5: at425slots known-text used12/request20/max31; at426 MetadataSlots used48/request378/max425 wins. |
| R12 | C# Ω/巣/行 R2/F2/P2/W2,n2,k2,h2,s1,a2: Seed90slots/64bytes +footprint177/142=267/206,ceiling3637,T36/40,U31/31. k1126 used90/request3549/max3637,direction CountReads1/IndexReads0,semantic0. Setup2 fits;limit1 used0/request2/max1. |
| R13 | Same C# fixture,four String(é) sort returns,F/F boundaries:206+8=214,not222. B213 used212/request2/max213 at fourth return before bounds/boundary/post. B214 log s0,s1,s0,s1,w,b,p,b,p,t. |
| R14 | Original thrown sort error at late frontier wins; nonexistent return charges no text. Boundary throw after sort admission wins before post,not a group-key text error. |
| R15 | Checked h+implicit/product/element/array representability;W/F/P exact+1;full overlong raw/canonical selectors even empty/missing/null. Distinguish consumed Alias raw segment from canonical endpoint-local bound. |
| R16 | Old By/Adjacent duplicate String(é)/String(💠) thresholds unchanged; every returned key still charges before post. |

Preserve checked intermediate order, retained spare capacity and exactly two C#
setup records. Never infer expected counts from production Footprint/metadata,
introduce a boundary vector/heap adapter under the unchanged ledger claim, or
label numeric resource errors cross-engine functional parity. Native retains its
own WorkBudget oracle. Grouped-observer/forged ordinary-context refusals are also
component-only. R10 remains **210**, including the accepted corrected subtotal28.
