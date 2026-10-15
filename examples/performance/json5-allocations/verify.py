import csv, hashlib, itertools, json, sys
from pathlib import Path

root = Path(sys.argv[1]).resolve()
destination = Path(sys.argv[2])
destination.mkdir()
load = lambda p: json.loads(p.read_text())
M = 67_108_864

def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda: f.read(1024*1024), b''): h.update(b)
    return h.hexdigest()

def guards(rows):
    for v in rows:
        p = Path(v['path'])
        assert p.stat().st_size == v['bytes'] and digest(p) == v['sha256'], p

def literal(path, pieces):
    with path.open('rb') as f:
        for byte, count in pieces:
            while count:
                n = min(count, 65536)
                assert f.read(n) == byte*n, path
                count -= n
        assert f.read(1) == b'', path

result = load(root/'CAMPAIGN-RESULT.json')
assert result['failure'] is None
assert load(root/'SOURCE-TOOL-GUARD-AFTER.json')['unchanged']
binding = load(root/'binding.json')
guards(binding['sources'] + binding['tools'])
guards(load(root/'generated-and-artifact-binding.json'))
receipt_paths = list((root/'small').rglob('ALL184_SMALL_PUBLIC_CALLS_COMPLETE.json'))
assert len(receipt_paths) == 1
receipt = load(receipt_paths[0])
assert (receipt['rust'], receipt['csharp'], receipt['total'], receipt['failures']) == (92,92,184,0)
assert len(receipt['call_results']) == 276
guards(receipt['sources'] + receipt['libraries'] + receipt['evidence'] + receipt['call_results'])
commands = list((root/'commands').glob('*/result.json'))
assert len(commands) == 154
for p in commands:
    x = load(p)
    assert x['exit'] == 0 and x['original_failure'] is None
    assert x['remaining_members'] == x['members_at_cleanup'] == [], p
cases = load(root/'prepared/CASES.json')
assert len(cases) == 6
Q, R, S = 11_184_809, 2, 33_554_425
for case in cases:
    p = Path(case['input']['path']); identifier = case['id']
    assert digest(p) == case['input']['sha256']
    if identifier.startswith('original'):
        pieces = [(bytes([x]),1) for x in b'{n:7}/*'] + [(b'x',M-9),(b'*',1),(b'/',1)]
        if identifier.endswith('plus1'): pieces.append((b' ',1))
    elif identifier.startswith('normalized'):
        pieces = [(bytes([x]),1) for x in b"{n:'"] + [(b'\0',Q),(b'a',R+identifier.endswith('plus1')),(b"'",1),(b'}',1)]
    else:
        pieces = [(bytes([x]),1) for x in b"{n:'"] + [(b'x',S),(b"'",1),(b'}',1)]
    literal(p,pieces)
rows = load(root/'rows.json')
assert len(rows) == 96
expected = set(itertools.product(range(2), [x['id'] for x in cases], range(4), ['control','gc-verbose']))
assert {(r['repetition'],r['id'],r['route'],r['mode']) for r in rows} == expected
phases = ['host-or-framework','normalization','schema-or-json-parse','typed-projection','mapping','serialization','boundary-encoding','other-runtime','unresolved']
csv_rows = []
total_ticks = missing = total_reported = missing_bytes = heap_events = successes = failures = 0
for row in rows:
    p = Path(row['observation']); case = next(c for c in cases if c['id'] == row['id'])
    call = load(p/'CALL-COMPLETE.json'); actual = load(p/'ORIGINAL-PUBLIC-RESULT.json')
    assert call == {'calls':1,'failures':0,'route':row['route'],'id':row['id'],'input_after_exact':True,'case_after_exact':True}
    assert load(p/'PRIMARY-AND-GUARD-RESULT.json') == {'primary':None,'guard':None,'afterExact':True}
    measurement = load(p/'whole-host.json'); assert measurement['exit'] == 0
    output = p/'ORIGINAL-RETURNED-UTF8.bin'
    if case['outcome'] == 'success':
        successes += 1
        assert actual['variant'] == 'Returned' and actual['original'] is None
        assert actual['actualType'] == ('System.String' if row['route'] < 2 else 'System.Byte[]')
        assert output.stat().st_size == case['output_bytes']
        assert digest(output) == actual['sha256'] == case['output_sha256']
        if row['id'] == 'output-exact':
            literal(output,[(bytes([x]),1) for x in b'{\n  "n": "'] + [(b'x',2*S)] + [(bytes([x]),1) for x in b'"\n}\n'])
        else: assert output.read_bytes() == b'{\n  "n": 7\n}\n'
    else:
        failures += 1
        assert actual['variant'] == 'Thrown' and not output.exists() and actual['actualType'] is None
        e = actual['original']; assert e['type'] == 'Ferrule.Runtime.FerruleJson5BoundaryException'
        stage,resource = {'original-limit':('Encoding','OriginalDocumentBytes'),'normalized-limit':('Syntax','NormalizedDocumentBytes'),'output-limit':('Output','OutputDocumentBytes')}[case['outcome']]
        assert e['fields']['Stage'] == stage and e['fields']['Resource'] == resource
        assert (e['fields']['Requested'],e['fields']['Maximum']) == (str(M+1),str(M))
        inner = e['inner']; assert inner['inner'] is None
        syntax = case['outcome']=='normalized-limit' or (case['outcome']=='original-limit' and row['route'] < 2)
        assert inner['type'] == 'Ferrule.Runtime.' + ('FerruleJson5SyntaxException' if syntax else 'FerruleJson5ResourceException')
        assert inner['fields']['Resource'] == resource
        assert (inner['fields']['Requested'],inner['fields']['Maximum']) == (str(M+1),str(M))
        if syntax:
            assert inner['fields']['Failure'] == 'Limit'
            assert inner['fields']['Offset'] == ('11184818' if case['outcome']=='normalized-limit' else '0')
    metrics = dict(repetition=row['repetition'],case=row['id'],route=row['route'],mode=row['mode'],
                   peak_rss_kib=measurement['whole_host_peak_rss_kib'],wall_seconds=measurement['wall_seconds'],
                   ticks='',missing_stacks='',events_lost='',post_gc_max_generation_sum_bytes='',trace_sha256='',
                   output_sha256=actual['sha256'] or '')
    metrics.update({phase+'_reported_bytes':'' for phase in phases})
    if row['mode'] == 'gc-verbose':
        a = load(p/'analysis/summary.json'); assert a['eventsLost'] == 0 and a['ticks'] > 0
        allocations = [json.loads(l) for l in (p/'analysis/allocation-ticks.jsonl').read_text().splitlines()]
        assert len(allocations) == a['ticks'] and {x['pid'] for x in allocations} == {measurement['pid']}
        assert sum(not x['frames'] for x in allocations) == a['noStack']
        for phase in phases:
            amount = sum(x['reportedAllocationBytes'] for x in allocations if x['phase'] == phase)
            assert amount == a['reportedAllocationBytes'].get(phase,0)
            metrics[phase+'_reported_bytes'] = amount
        heaps = [json.loads(l) for l in (p/'analysis/gc-heap-stats.jsonl').read_text().splitlines()]
        assert len(heaps) == a['heapEvents']
        assert all(x['pid'] == measurement['pid'] for x in heaps)
        metrics.update(ticks=a['ticks'],missing_stacks=a['noStack'],events_lost=a['eventsLost'],
                       post_gc_max_generation_sum_bytes=max((sum(h[f'size{i}'] for i in range(5)) for h in heaps),default=''),
                       trace_sha256=digest(p/'original.nettrace'))
        total_ticks += a['ticks']; missing += a['noStack']; heap_events += a['heapEvents']
        total_reported += sum(a['reportedAllocationBytes'].values()); missing_bytes += a['reportedAllocationBytes'].get('unresolved',0)
    csv_rows.append(metrics)
assert (successes,failures) == (48,48)
with (destination/'observations.csv').open('x',newline='') as f:
    w=csv.DictWriter(f,fieldnames=list(csv_rows[0]),lineterminator="\n");w.writeheader();w.writerows(csv_rows)
headroom = [json.loads(l) for l in (root/'headroom.jsonl').read_text().splitlines()]
summary = {'calls':len(rows),'successes':successes,'expected_refusals':failures,'closed_commands':len(commands),
           'traces':48,'ticks':total_ticks,'missing_stacks':missing,'events_lost':0,'reported_bytes':total_reported,
           'unresolved_reported_bytes':missing_bytes,'heap_events':heap_events,
           'min_available_memory':min(x['available_memory'] for x in headroom),
           'min_available_disk':min(x['available_disk'] for x in headroom),
           'max_campaign_logical_bytes':max(x['campaign_logical_bytes'] for x in headroom),
           'binding_sha256':digest(root/'binding.json'),'artifact_binding_sha256':digest(root/'generated-and-artifact-binding.json'),
           'small_receipt_sha256':digest(receipt_paths[0]),'csv_sha256':digest(destination/'observations.csv')}
(destination/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
