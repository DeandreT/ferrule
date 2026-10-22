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

The [resource profile](pdf-worker-resource-profile.md) repeats worker and
in-process extraction with the corrected verification path on a separate
self-authored corpus, recording the build profile, brackets, complete outcomes,
own-reap usage and allocation failures. It does not reuse the inputs above, so it
makes no improvement or parser-memory claim against these five observations.

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
