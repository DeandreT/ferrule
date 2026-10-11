# PDF worker memory observations

These five retained trials measure process lifetime high-water marks, including
setup and executable verification. They do not measure parser-only memory,
provide a resident-memory ceiling, or establish an improvement after the bounded
executable-read correction.

## Original observations

Input sizes come from retained file identities. RSS values are in **KiB (1,024
bytes)**. Host columns are `getrusage(RUSAGE_SELF).ru_maxrss` observations before
and after the request; the worker column is the raw usage returned by the owned
child's sole `wait4` reap.

| Original sequence / input | Input bytes | Host SELF maximum RSS before / after, KiB | Worker own-wait4 maximum RSS, KiB | Retained outcome |
| --- | ---: | ---: | ---: | --- |
| 1 / F01, direct baseline | 431 | 115,676 / 115,676 | — | Baseline success |
| 2 / F01, worker | 431 | 113,988 / 453,364 | 452,516 | Worker success |
| 19 / F10, worker | 211,812 | 116,460 / 452,888 | 452,116 | Worker success |
| 20 / F11, worker | 1,691,201 | 115,140 / 456,988 | 453,828 | `WorkerExit`; raw wait status 134 |
| 25 / F14, worker | 99,448 | 116,660 / 453,056 | 452,328 | `Pdf`; exit 2 |

Sequence 20's retained stderr reports a failed **268,435,456-byte allocation**.
The allocating component and the cause of failure remain unknown. Neither the
raw wait status nor that message proves that the address-space limit caused it.
The failure remains part of the report.

## Measurement limits

On Linux, `ru_maxrss` is maximum resident set size in KiB, and resource usage
persists across `execve`. A process lifetime high-water value cannot be reset by
replacing the executable. [getrusage manual](https://man7.org/linux/man-pages/man2/getrusage.2.html)

`wait4` can select a particular child and return that child's resource usage.
The values above are own-child observations, not cumulative `RUSAGE_CHILDREN`
measurements or sampled current RSS. [wait4 manual](https://man7.org/linux/man-pages/man2/wait4.2.html)

Do not subtract the host's before value from its after value to obtain a parser
peak, or add host and worker peaks to estimate simultaneous RAM use. These
lifetime maxima do not identify which component allocated memory. Similar host
and worker values do not establish the actual launch mechanism or an inherited
amount.

The original Rust harness buffered both executable images in full before worker
launch and repeated those reads before the after observation. Setup and
validation are therefore inside the host measurement. The Python handoff also
read the complete **93,623,912-byte test harness** before replacing itself with
that executable. These are identified verification costs; the table does not
separate them from extraction, startup or other process costs.

Executable verification has since been changed to bounded streaming, with
separate control checks. Those checks are not memory trials. A fresh resource
campaign and report remain pending; no post-correction RSS result or reduction
is claimed here.

## Follow-up

- [ ] Repeat the same inputs and output checks with the corrected verification
  path; record the build profile, measurement bracket and complete outcomes.
- [ ] Retain each trial's host lifetime observations and exact child reap usage,
  including allocation failures; keep sampled values distinct from peaks.
- [ ] Compare repeated trials before making an improvement or parser-memory
  claim; preserve the original five observations.

Track resource qualification in [#328](https://github.com/DeandreT/ferrule/issues/328)
and bounded executable verification/reporting in
[#394](https://github.com/DeandreT/ferrule/issues/394). The twelve filesystem-route
checks in [#388](https://github.com/DeandreT/ferrule/issues/388) and load-request
observations in [#391](https://github.com/DeandreT/ferrule/issues/391) have separate
functional scopes. This table does not qualify those routes or callback counts.

## Real loader-request counts

A separate functional check ran twelve fresh filesystem-route requests, using
the same fixtures and expectations in two repetitions. It covered ordinary
`Hello` and typed `Null` outputs through primary, selected static, and demanded
dynamic sources. Actual schema 5 traces gave these counts:

| Filesystem route | Requests | Attempted loader callbacks per request | Total attempts |
| --- | ---: | ---: | ---: |
| Primary | 4 | 0 | 0 |
| Selected static | 4 | 0 | 0 |
| Demanded dynamic | 4 | 1 | 4 |

The four attempts came from 84 complete trace event records. All twelve whole
XML and worker-wire outputs and all 25 normal owner closures were accepted.
Earlier twelve-route counts inferred from outputs or job records remain
unqualified as loader callback counts.

These are attempted loader callbacks, not a count of physical PDF acquisitions
or evidence of cache reuse. Each dynamic request made one demand; repeated
demands, cache behavior, and unused routes were outside this check. It adds no
RSS, parser-only memory, native, or full-parity claim. Load-request observation
is tracked in [#391](https://github.com/DeandreT/ferrule/issues/391).

## Fresh 70-request campaign (2026-10-11)

The later campaign completed seventy serial requests against twenty frozen
fixtures in two repetitions, reversing fixture and route order in the second
repetition. It used thirty direct-library baselines and forty filesystem-worker
attempts. All 140 request and decoder owners closed normally without rescue;
36 PDF workers launched and were reaped by their sole owner.

The compiled test harness and reused default production worker pair were bound
to the frozen 2,675-file source cohort. The pair was retained from the preceding
qualified build under verified production-source equality; no fresh default
rebuild or current-main measurement is claimed. Executable verification used
bounded streaming, with complete retained image and input identities. This
completes the fresh-campaign reporting item above; the original five trials,
their earlier pending status, and the separate loader-count results stay intact.

### Repeated process-lifetime observations

Each range below is the minimum and maximum of two retained observations for
that fixture and route; a single value means both observations were equal.
Host values are `getrusage(RUSAGE_SELF).ru_maxrss` **through the after snapshot**,
including handoff, setup and validation. They are not after-exit or API-only
measurements. Child values are the separately retained own-child `wait4`
`ru_maxrss`. All values are KiB (1,024 bytes). A dash means that route was not
run or no worker launched; it does not mean zero memory use.

| Fixture | Input bytes | Library-route host, KiB | Worker-route host, KiB | Worker own-wait4, KiB |
| --- | ---: | ---: | ---: | ---: |
| F01 | 431 | 25,088–25,496 | 25,328–25,556 | 8,764–8,980 |
| F02 | 578 | 25,340–25,508 | 25,588–25,692 | 8,860–9,252 |
| F03 | 1,354 | 25,340 | 25,216–25,412 | 9,104–9,156 |
| F04 | 53,024 | 25,240–25,724 | 25,608–25,820 | 9,280–9,728 |
| F05 | 53,231 | 25,408–25,652 | 25,424–25,460 | 8,780–8,980 |
| F06 | 8,388,608 | 26,172–26,480 | 35,444–35,532 | 24,972–25,020 |
| F07 | 8,388,609 | 25,556–25,588 | 25,152–25,636 | — |
| F08 | 8,388,609 | 25,288–25,648 | 25,148–25,348 | — |
| F09 | 7,073 | 25,396–25,692 | 25,320–25,648 | 11,044–11,300 |
| F10 | 211,812 | — | 25,556–25,836 | 77,700–77,944 |
| F11 | 1,691,201 | — | 25,076–25,548 | 280,844–282,064 |
| F12 | 6,339 | 25,176–25,820 | 25,152–25,496 | 8,808–8,980 |
| F13 | 6,433 | 25,400–25,452 | 25,628–25,636 | 8,648–8,700 |
| F14 | 99,448 | — | 25,564–25,644 | 10,348–10,736 |
| F15 | 442 | 25,384–25,660 | 25,260–25,476 | 8,532–8,844 |
| F16 | 407 | 25,316–25,480 | 24,924–25,592 | 8,704–8,892 |
| F17 | 431 | 25,464–25,628 | 25,296–25,420 | 8,500–8,732 |
| F18 | 740 | 25,320–25,456 | 25,020–25,604 | 8,564–8,908 |
| F19 | 89 | — | 25,240–25,636 | 8,836–8,876 |
| F20 | 7,073 | — | 25,240–25,548 | 10,888–10,952 |

F07 exceeded the 8-MiB input cap before launch: the host retained a closed
`InputTooLarge` failure and receipt, with no child, Ready/Start, wait status or
worker usage. F08's repair-dependent layout was refused without a child, request
or receipt. Their host observations remain recorded; neither row supplies a
worker measurement.

The frozen sixty exact-class requests yielded **56 complete semantic matches
and four prelaunch boundary matches**. Six resource observations came from
F10, F11 and F14: F10 returned typed `Null`, F14 returned the complete PDF
inheritance error with exit 2, and both F11 workers ended with raw wait status
134 (`SIGABRT`) and 122-byte allocation-failure stderr. The allocating component
and cause remain unknown. The four F19/F20 malformed-input outcomes remain
exploratory and receive no exact-pass credit. Full outputs and errors were
retained before comparison; an owner exiting normally does not turn a resource
termination into semantic success.

All 36 launched workers reported the admitted `linux-x86_64-v1` profile:
512 MiB address space, CPU soft/hard limits of 30/31 seconds, a 64-MiB file-size
limit and no core output, with 30-second work and 5-second stop allowances.
These are route limits, not a hard RSS ceiling.

The table reports dedicated process lifetimes. It does not isolate parser
allocations, subtract high-water marks, add host and child peaks, establish a
causal improvement over the original trials, or qualify current main. The
separate twelve-route functional and loader-count checks remain separate; this
campaign supplies no all-82, native or full-parity claim.

### Remaining qualification

- [ ] Reconcile and qualify a current-main build with explicit artifact and
  source bindings before reporting current-main resource results.
- [ ] Separate setup, executable validation, startup, extraction and publication
  costs in a controlled attribution study; preserve both cohorts and failures.
- [ ] Compose route coverage only through explicit compatible-source admission;
  keep broader PDF workloads, native coverage and full parity separately gated.
