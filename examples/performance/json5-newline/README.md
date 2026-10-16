# JSON5 final-newline controls

These opt-in controls supplement the unchanged
[six-recipe allocation study](../json5-allocations/README.md). They do not replace
its original 184 small calls or its 96 physical calls. Run that study to closure
first, with the baseline and candidate generated libraries retained separately.

`UnicodeLimitHost.cs` maps an empty input group to a string containing exactly
22,369,616 `雪` characters followed by two ASCII `a` characters. The independently
specified output is `b'{\n  "n": "' + b'\xe9\x9b\xaa' * 22_369_616 + b'aa"\n}\n'`,
exactly 67,108,864 UTF8 bytes including the final newline. A third ASCII character
must produce the existing typed output-resource refusal requesting 67,108,865
bytes, with no partial returned value. Both text and byte APIs are checked for
both cases, before and after the change: eight separate calls.

The host retains each complete original returned value or exception chain before
checking the full literal prefix, every Unicode triplet, suffix, length and SHA256.
The runner computes the expected hash from the literal byte recipe independently
of either runtime. Each variant references its generated library, whose C# source
closure must differ in exactly the two intended runtime files. The baseline and
candidate use the same host source and Release settings.

From the repository root, supply absolute paths to the two generated library
directories, the successfully closed candidate campaign, a fresh owned evidence
directory, and the selected .NET executable:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 examples/performance/json5-newline/unicode_controls.py \
  --repo "$PWD" \
  --baseline-library "$BASELINE/prepared/output-exact/library" \
  --candidate-library "$CANDIDATE/prepared/output-exact/library" \
  --prerequisite "$CANDIDATE" \
  --root "$UNICODE_EVIDENCE" \
  --dotnet /usr/bin/dotnet
```

Use task-specific variables for the retained baseline/candidate roots and the new
evidence root. The runner refuses an existing destination and requires Linux. It
reuses the finite study's 4 GiB available-memory and 20 GiB free-disk admission
thresholds, 8 GiB logical evidence budget, one-hour deadline, 180-second command
deadline, owned process groups and child reaping. It builds two hosts serially
with package sources cleared, then runs one fresh CLR per call without profiling,
forced collections or GC overrides. These extra controls establish output/error
semantics; their execution is separate from the allocation/RSS comparison.

Optimized Python execution (`-O`, `-OO`, or `PYTHONOPTIMIZE`) is refused before
argument parsing or preparation so that prerequisite, source-closure and result
assertions cannot be skipped. Inherited CLR/profiler overrides are refused before
evidence creation. The constructed child environment explicitly sets
`DOTNET_ADD_GLOBAL_TOOLS_TO_PATH=false` for its isolated .NET CLI home, overriding
an inherited setting. Other inherited `DOTNET_` settings remain forbidden.

Portable launcher regression tests do not launch .NET, the allocation campaign,
or native workloads:

```sh
python -B examples/performance/json5-newline/test_unicode_controls.py
```

`RESULT.json` must report no failure and ten completed commands (two builds and
eight calls). Each observation must have `COMPLETE.json` with zero failures;
successful calls retain their entire `ORIGINAL-RETURNED-UTF8.bin`. The binding
files retain source and built-artifact identities. Original environments and
failures stay local alongside the evidence and should be reviewed before sharing.
