# Filesystem Input Lifetime: 2026-10-07

Releasing evaluated inputs before output serialization reduced the process peak
for one wide CSV row by 7.9–8.3% in these two comparisons. The many-row workload
showed no peak reduction: the candidate peaks were 0.2–0.4% higher. These are
individual observations from a normal debug build, not a general memory bound
or a statistically established performance improvement.

## Change and workload

The sole production change explicitly drops the primary parsed input and the
dynamic-source loader after mapping and the before-publication check, before
the existing selected/all-target writers. The mapped target remains owned.
Both builds use the same current CSV encoder, complete dependency features,
compiler, Cargo lockfile and normal non-test profile: optimization level 0,
debug information level 2, incremental compilation disabled. The complete
paired source inventories differ only by the two comment/drop/drop insertions.

Both CSVs have `Id,Text` headers, comma separators, double-quote escaping, LF
records, UTF-8 and no BOM. Each Text contains an eight-digit row number, a colon,
enough `x` characters to reach the listed Text size, then a semicolon, quote and
apostrophe (1,012 `x` characters for many rows; 33,554,420 for the wide row).
The mapping copies Id and concatenates three ordered copies of Text.

| Shape | Rows | Text bytes per row | Input bytes | Output bytes | Selection |
| --- | ---: | ---: | ---: | ---: | --- |
| Many rows | 32,768 | 1,024 | 33,871,014 | 101,045,414 | All targets, primary only |
| Wide row | 1 | 33,554,432 | 33,554,446 | 100,663,312 | Explicit primary |

Inputs were prepared once and independently checked outside the measured
processes. Each matched pair used the same project/input files, working directory
and environment, with a fresh equal-length output path on the same filesystem.
The only argv differences were the executable and recorded output path.
Execution was serial; the second repeat reversed the role order. No warmup,
forced collection or cache flush was used; no other qualification compiler or
tested application overlapped the campaign.
Fixture preparation and output verification affect filesystem caches; ordinary
host activity was not isolated.

## Individual observations

Peak MiB is Linux `wait4` process-lifetime `ru_maxrss`, converted from KiB.
CPU is direct-child user plus system time. Wall time spans invocation through
genuine reap, including launch and sampling overhead.

| Ordinal | Shape | Repeat | Build | Peak MiB | CPU seconds | Wall seconds |
| ---: | --- | ---: | --- | ---: | ---: | ---: |
| 1 | Many rows | 1 | Baseline | 325.3 | 0.935 | 0.995 |
| 2 | Many rows | 1 | Candidate | 326.7 | 0.942 | 0.988 |
| 3 | Wide row | 1 | Baseline | 358.7 | 0.706 | 0.713 |
| 4 | Wide row | 1 | Candidate | 328.9 | 0.705 | 0.740 |
| 5 | Many rows | 2 | Candidate | 325.2 | 0.920 | 0.930 |
| 6 | Many rows | 2 | Baseline | 324.5 | 0.914 | 0.929 |
| 7 | Wide row | 2 | Candidate | 330.3 | 0.717 | 0.729 |
| 8 | Wide row | 2 | Baseline | 358.7 | 0.777 | 0.828 |

| Matched shape | Repeat | Candidate minus baseline peak |
| --- | ---: | ---: |
| Many rows | 1 | +1,490,944 bytes (+0.44%) |
| Many rows | 2 | +733,184 bytes (+0.22%) |
| Wide row | 1 | −31,289,344 bytes (−8.32%) |
| Wide row | 2 | −29,765,632 bytes (−7.91%) |

The retained 50 ms process snapshots contain 139 attempted and 129 accepted
identity checks across the eight runs. Sampled RSS and sampled high-water values
remain separate from `wait4`; they are sparse observations, and the kernel
accounting can differ. They do not measure an exclusive serialization peak.

## Correctness and scope

- [x] Pass 24 existing CLI regressions covering callbacks, selected/all targets,
  dynamic sources/documents, row release, and destination preservation.
- [x] Pass formatting, strict workspace checks and both normal CLI builds.
- [x] Run six small controls per build: full literals, BOM, alias refusal with
  an unchanged sentinel, primary/named selection and explicit output override.
- [x] Compare every byte, exact EOF and complete hashes for all eight large
  outputs against an independent streaming literal oracle.
- [x] Retain original streams, raw process snapshots, genuine status/resource
  records and complete source/executable identities; close all 20 child processes.

The workload has no named or dynamic input, so it does not measure loader-cache
release. Two repeats do not establish statistical significance. Earlier
parsing/mapping peaks and allocator-retained pages can mask shorter input
lifetimes. Full mapped rows and serialized buffers still exist; this change
does not add streaming, a total-RAM cap, or a universal memory/CPU saving.
