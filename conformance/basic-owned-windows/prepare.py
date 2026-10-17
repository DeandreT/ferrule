#!/usr/bin/env python3
"""Portable Basic-window fixture preparation, source binding and complete capture comparison.

Preparation/snapshot/bind never execute a compiler or mapping. `run` is explicit,
serial, and must be invoked only in the repository's assigned compiler lane.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROLES = ('programs.json', 'expected.json', 'emitter.rs', 'emitter.Cargo.toml.template',
         'rust-caller.template.rs', 'rust-caller.Cargo.toml.template',
         'csharp-caller.template.cs', 'csharp-caller.csproj.template', 'prepare.py')
MODELS = ('operators', 'filter_then_sort', 'sort_then_filter', 'nested_parent_bounds',
          'empty_bounds_eager', 'zero_window_non_bool', 'invalid_bound',
          'fts_predicate_priority', 'stf_key_priority', 'first_predicate_priority')
COUNTS = {'models': 10, 'calls_per_backend': 12, 'total_calls': 36}
FLAGS = {'full_window_parity': False, 'full_error_parity': False, 'total_parity': False}
CASES = tuple('W' + str(index).zfill(2) for index in range(1, 13))
IDS = tuple('WINDOW-' + backend + '-' + case
            for backend in ('native', 'rust', 'csharp') for case in CASES)
FROZEN_MODELS_SHA256 = 'c5d909518aacae5e85551ef36a7434cb631ac850f8fc2151ca83434405e01f02'


def encoded(value):
    return (json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + '\n').encode()


def same_json(left, right):
    # Python considers True == 1; complete typed JSON comparisons must not.
    options = {'sort_keys': True, 'ensure_ascii': False, 'allow_nan': False}
    return json.dumps(left, **options) == json.dumps(right, **options)


def read(path, digest=None):
    path = Path(path).resolve()
    before = path.stat()
    body = path.read_bytes()
    after = path.stat()
    fields = ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_ctime_ns')
    if any(getattr(before, key) != getattr(after, key) for key in fields):
        raise ValueError('Input changed while reading: ' + str(path))
    pin = {'path': str(path), 'bytes': len(body), 'sha256': hashlib.sha256(body).hexdigest()}
    if digest is not None and pin['sha256'] != digest:
        raise ValueError('Input digest mismatch: ' + str(path))
    return body, pin


def write_new(path, body):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as stream:
        stream.write(body)
        stream.flush()
        os.fsync(stream.fileno())
    return read(path)[1]


def json_new(path, value):
    return write_new(path, encoded(value))


def sources():
    fixture = Path(__file__).resolve().parent
    pins = {name: read(fixture / name) for name in ROLES}
    spec = json.loads(pins['programs.json'][0])
    expected = json.loads(pins['expected.json'][0])
    if pins['programs.json'][1]['sha256'] != FROZEN_MODELS_SHA256:
        raise ValueError('The accepted ten-model inventory must remain frozen')
    if spec['exact_counts'] != COUNTS or tuple(model['id'] for model in spec['models']) != MODELS:
        raise ValueError('Exact ten-model/twelve-call inventory required')
    if spec['issue'] != 704 or expected['issue'] != 704:
        raise ValueError('Wrong issue authority')
    if tuple(call['case'] for call in spec['calls']) != CASES:
        raise ValueError('Exact frozen call ordering required')
    if tuple(row['id'] for row in expected['calls']) != IDS:
        raise ValueError('Exact 36-oracle ordering required')
    if any(spec[key] is not False or expected[key] is not False for key in FLAGS):
        raise ValueError('Finite corpus cannot claim full parity')

    def expand(value, trail=()):
        if isinstance(value, dict):
            if '$ref' in value:
                name = value['$ref']
                if set(value) != {'$ref'} or name in trail:
                    raise ValueError('Malformed/cyclic oracle reference')
                return expand(expected['trees'][name], trail + (name,))
            return {key: expand(item, trail) for key, item in value.items()}
        if isinstance(value, list):
            return [expand(item, trail) for item in value]
        return value

    # This is static reference expansion, never evaluation of the subject Project.
    # Expanded materialization also works as adjacent input when copied out of-tree.
    if 'outcomes' in expected:
        for row in expected['calls']:
            outcome = expected['outcomes'][row['case']]
            row['expected'] = expand(outcome['expected'])
            row['typed_error'] = outcome.get('csharp_typed_error', outcome['typed_error']) \
                if row['backend'] == 'csharp' else outcome['typed_error']
        expected.pop('trees')
        expected.pop('outcomes')
    for row, call in zip(expected['calls'], spec['calls'] * 3):
        if row['case'] != call['case'] or row['model'] != call['model'] or row['expected'] is None:
            raise ValueError('Complete per-backend oracle required')
    return pins, spec, expected


def source_snapshot(root):
    """Hash the current transitive local Rust closure and complete C# source tree."""
    root = Path(root).resolve()
    workspace = tomllib.loads(read(root / 'Cargo.toml')[0].decode())
    pending = [root / 'crates' / name for name in
               ('engine', 'mapping', 'ir', 'codegen', 'codegen-rust', 'codegen-csharp', 'codegen-runtime')]
    visited, paths = set(), {root / 'Cargo.toml', root / 'Cargo.lock'}
    while pending:
        crate = pending.pop().resolve()
        if crate in visited:
            continue
        crate.relative_to(root)  # Never authorize dependencies outside this checkout.
        visited.add(crate)
        manifest = tomllib.loads(read(crate / 'Cargo.toml')[0].decode())
        paths.update(path for path in crate.rglob('*') if path.is_file()
                     and not {'target', 'bin', 'obj'} & set(path.relative_to(crate).parts)
                     and (path.suffix in ('.rs', '.toml', '.json', '.xsd', '.xml') or path.name == 'Cargo.lock'))
        tables = [manifest] + list(manifest.get('target', {}).values())
        for table in tables:
            for section in ('dependencies', 'dev-dependencies', 'build-dependencies'):
                for name, dependency in table.get(section, {}).items():
                    if not isinstance(dependency, dict):
                        continue
                    base = crate
                    if dependency.get('workspace'):
                        dependency = workspace['workspace']['dependencies'][name]
                        base = root
                    if isinstance(dependency, dict) and 'path' in dependency:
                        pending.append(base / dependency['path'])
    runtime = root / 'runtime/csharp'
    paths.update(path for path in runtime.rglob('*') if path.is_file()
                 and not {'bin', 'obj'} & set(path.relative_to(runtime).parts)
                 and path.suffix in ('.cs', '.csproj', '.props', '.targets', '.json', '.config'))
    for name in ('rust-toolchain.toml', 'rust-toolchain', '.cargo/config.toml', 'global.json'):
        if (root / name).is_file():
            paths.add(root / name)
    files = {}
    for path in sorted(paths):
        _, pin = read(path)
        files[path.relative_to(root).as_posix()] = {key: pin[key] for key in ('bytes', 'sha256')}
    fingerprint = hashlib.sha256(encoded(files)).hexdigest()
    return {'issue': 704, 'source_root': str(root), 'source_fingerprint': fingerprint, 'files': files}


def verify_snapshot(snapshot):
    current = source_snapshot(snapshot['source_root'])
    if current != snapshot:
        raise ValueError('Current source no longer matches the qualified snapshot')


def bind(args):
    snapshot_body, snapshot_pin = read(args.snapshot)
    snapshot = json.loads(snapshot_body)
    verify_snapshot(snapshot)
    _, dll = read(args.runtime_dll)
    evidence_pins = {}
    for issue, path in ((702, args.rust_evidence), (703, args.csharp_evidence)):
        body, pin = read(path)
        evidence = json.loads(body)
        if evidence.get('issue') != issue or evidence.get('qualified') is not True \
                or evidence.get('source_fingerprint') != snapshot['source_fingerprint']:
            raise ValueError('Successful source-bound current qualification required: ' + str(issue))
        checks = {'generated_windows', 'preserved_sort_filter', 'runtime'}
        if not checks <= set(evidence.get('checks', [])) or not evidence.get('commands'):
            raise ValueError('Window, preserved-profile and runtime qualifications required')
        commands = []
        for command in evidence['commands']:
            if command['exit_code'] != 0 or not command['argv']:
                raise ValueError('Only successful retained qualification commands may bind')
            commands.append({**command, 'stdout_pin': read(command['stdout'])[1],
                             'stderr_pin': read(command['stderr'])[1]})
        if issue == 703 and evidence.get('runtime_dll_sha256') != dll['sha256']:
            raise ValueError('Fresh DLL must be an output of this source-bound C# qualification')
        evidence_pins[str(issue)] = {'input': pin, 'commands': commands}
    result = {'issue': 704, 'qualified': True, 'snapshot': snapshot, 'snapshot_pin': snapshot_pin,
              'runtime_dll': dll, 'qualification_evidence': evidence_pins, **FLAGS}
    print(json.dumps(json_new(args.out, result)))


def check_binding(path):
    body, pin = read(path)
    binding = json.loads(body)
    if binding['issue'] != 704 or binding['qualified'] is not True:
        raise ValueError('Qualified current local binding required')
    verify_snapshot(binding['snapshot'])
    read(binding['snapshot_pin']['path'], binding['snapshot_pin']['sha256'])
    read(binding['runtime_dll']['path'], binding['runtime_dll']['sha256'])
    for evidence in binding['qualification_evidence'].values():
        read(evidence['input']['path'], evidence['input']['sha256'])
        for command in evidence['commands']:
            for key in ('stdout_pin', 'stderr_pin'):
                read(command[key]['path'], command[key]['sha256'])
    return binding, pin


def validate_complete_error(record, oracle):
    typed = oracle['typed_error']
    if not same_json(record['typed_error'], typed):
        raise ValueError('Complete typed envelope differs')
    if typed is None:
        if record.get('complete_error') is not None or record.get('complete_error_chain') is not None:
            raise ValueError('Successful return unexpectedly retained an error')
        return
    if record['backend'] != 'csharp':
        # Native includes the top error, Rust retains the source chain separately.
        chain = record['complete_error_chain']
        expected_length = typed['source_chain_length'] + (record['backend'] == 'native')
        if len(chain) != expected_length:
            raise ValueError('Complete cause chain length differs')
        if record['backend'] == 'native' and chain[0]['display'] != typed['display']:
            raise ValueError('Original top error display differs')
        if record['backend'] == 'native' and (not chain[0]['debug'] or not record['complete_return_debug']):
            raise ValueError('Original native error debug was not retained')
        if record['backend'] == 'rust' and not record['complete_error_debug']:
            raise ValueError('Original Rust error debug was not retained')
        return
    error = record['complete_error']
    if error['type'] != 'Ferrule.Runtime.FerruleRuntimeException' or error['hresult'] != -2146233088:
        raise ValueError('CLR type/HResult differs')
    if error['message'] != typed['display'] or error['data'] or error['help_link'] is not None \
            or not error['full_native'] or not error['stack_trace'] or not error['base_exception_same_error']:
        raise ValueError('Complete original CLR payload differs or was not retained')
    if any(error[key] is not None for key in ('inner', 'owned', 'bridge', 'traversal', 'argument')):
        raise ValueError('Unexpected typed inner cause')
    runtime = error['runtime']
    category_numbers = {'MappingException': 62, 'NotABool': 21, 'NotAnItemCount': 22}
    present = {'category': typed['variant'], 'category_numeric': category_numbers[typed['variant']],
               'same_captured_exception': True, 'Node': typed['node'],
               'owned_same_inner': False, 'bridge_same_inner': False}
    if typed['variant'] == 'MappingException':
        present['MappingExceptionMessage'] = typed['mapping_exception_message']
    else:
        present['found_kind'] = typed['found_kind']
    if any(runtime.get(key) != value for key, value in present.items()) \
            or any(value is not None for key, value in runtime.items() if key not in present):
        raise ValueError('CLR metadata including absent generated fields differs')


def classify(args):
    pins, _, expected = sources()
    root = Path(args.capture_root).resolve()
    plan = json.loads(read(root / 'COMMAND_PLAN.json')[0])
    _, binding_pin = check_binding(plan['binding']['path'])
    if binding_pin != plan['binding']:
        raise ValueError('Runtime binding changed after preparation')
    for pin in plan['materialized_inputs'].values():
        read(pin['path'], pin['sha256'])
    oracle_pin = plan['materialized_inputs']['expected.json']
    frozen_expected = json.loads(read(oracle_pin['path'], oracle_pin['sha256'])[0])
    if not same_json(expected, frozen_expected):
        raise ValueError('Oracles differ from their pre-execution materialization')
    records, missing = [], []
    # No oracle comparison occurs until all 36 complete originals have been read.
    for identifier in IDS:
        parent = root / 'generated' if identifier.startswith('WINDOW-native-') else root / 'captures'
        path = parent / (identifier + '.capture.json')
        if not path.is_file():
            missing.append(identifier)
            continue
        body, descriptor = read(path)
        record = json.loads(body)
        backend = identifier.split('-')[1]
        if record['id'] != identifier or record['backend'] != backend \
                or record['classification'] != 'raw_return_before_any_classification':
            raise ValueError('Original capture identity differs')
        records.append((record, descriptor))
    barrier = {'issue': 704, 'records': [{'id': row['id'], 'descriptor': pin} for row, pin in records],
               'count': len(records), 'missing': missing, 'oracle': oracle_pin,
               'classifier_input': pins['prepare.py'][1], **FLAGS}
    json_new(root / 'ALL36_BEFORE_COMPARISON.json', barrier)
    if missing:
        json_new(root / 'WINDOW36_INCOMPLETE.json', {'qualified': False, 'expected_results_compared': False, **barrier})
        raise SystemExit('Missing complete captures: ' + ', '.join(missing))
    differences = []
    for (record, descriptor), oracle in zip(records, expected['calls']):
        if not same_json(record['snapshot'], oracle['expected']):
            differences.append({'id': record['id'], 'kind': 'complete_public_return',
                                'original': descriptor, 'expected': oracle['expected'], 'actual': record['snapshot']})
        try:
            validate_complete_error(record, oracle)
        except (ValueError, KeyError, TypeError) as error:
            differences.append({'id': record['id'], 'kind': 'complete_typed_error', 'detail': str(error),
                                'expected': oracle['typed_error'], 'actual': record.get('typed_error')})
    semantic_differences = []
    for index, case in enumerate(CASES):
        values = [records[offset + index][0]['snapshot'] for offset in (0, 12, 24)]
        if not same_json(values[0], values[1]) or not same_json(values[0], values[2]):
            semantic_differences.append({'case': case, 'native': values[0], 'rust': values[1], 'csharp': values[2]})
    result = {'issue': 704, 'qualified': not differences and not semantic_differences,
              'classification': 'FINITE_WINDOW36_MATCHED' if not differences and not semantic_differences
              else 'FINITE_WINDOW36_DIFFERENCES_RETAINED', 'count': 36,
              'all36_raw_returns_retained_before_any_comparison': True,
              'native_calibrated': not any(row['id'].startswith('WINDOW-native-') for row in differences),
              'differences': differences, 'semantic_differences': semantic_differences, **FLAGS}
    json_new(root / 'WINDOW36_CLASSIFICATION.json', result)
    print(json.dumps(result, ensure_ascii=False))
    if not result['qualified']:
        raise SystemExit(1)


def prepare(args):
    pins, spec, expected = sources()
    if any(not Path(path).is_absolute() for path in (args.cargo, args.dotnet, args.target, args.out, args.binding)):
        raise ValueError('Absolute tool, target, output and binding paths required')
    binding, binding_pin = check_binding(args.binding)
    source_root = Path(binding['snapshot']['source_root'])
    out = Path(args.out).resolve()
    out.mkdir(exist_ok=False)
    (out / 'captures').mkdir()
    materialized = {name: write_new(out / 'fixture' / name,
                                   encoded(expected) if name == 'expected.json' else body)
                    for name, (body, _) in pins.items()}
    emitter_manifest = pins['emitter.Cargo.toml.template'][0].decode()
    for crate in ('engine', 'mapping', 'ir', 'codegen', 'codegen-rust', 'codegen-csharp'):
        token = '__' + crate.upper().replace('-', '_') + '_PATH__'
        emitter_manifest = emitter_manifest.replace(token, json.dumps(str(source_root / 'crates' / crate)))
    if '__' in emitter_manifest:
        raise ValueError('Unbound emitter manifest token')
    write_new(out / 'emitter/Cargo.toml', emitter_manifest.encode())
    write_new(out / 'emitter/src/main.rs', pins['emitter.rs'][0])
    write_new(out / 'NuGet.Config', b'<configuration><packageSources><clear /></packageSources></configuration>\n')
    capture_env = {'FERRULE_CONFORMANCE_CAPTURE_ONLY': '1', 'FERRULE_CONFORMANCE_CAPTURE_DIR': str(out / 'captures')}
    cargo_env = {'CARGO_TARGET_DIR': str(Path(args.target).resolve()), 'CARGO_BUILD_JOBS': '1',
                 'CARGO_INCREMENTAL': '0', 'CARGO_TERM_COLOR': 'never', **capture_env}
    dotnet_env = {'DOTNET_CLI_USE_MSBUILD_SERVER': '0', 'MSBUILDDISABLENODEREUSE': '1',
                  'DOTNET_PROCESSOR_COUNT': '2',
                  'DOTNET_CLI_TELEMETRY_OPTOUT': '1', 'DOTNET_SKIP_FIRST_TIME_EXPERIENCE': '1',
                  'DOTNET_NOLOGO': '1', 'DOTNET_ADD_GLOBAL_TOOLS_TO_PATH': 'false',
                  'DOTNET_CLI_HOME': str(out / 'dotnet-home'), **capture_env}
    steps = []

    def step(name, argv, environment):
        steps.append({'name': name, 'argv': [str(value) for value in argv], 'environment_delta': environment,
                      'working_directory': str(out), 'stdout': str(out / (name + '.stdout.complete.original')),
                      'stderr': str(out / (name + '.stderr.complete.original'))})

    generated = out / 'generated'
    manifest = out / 'emitter/Cargo.toml'
    step('emitter_lock', [args.cargo, 'generate-lockfile', '--offline', '--manifest-path', manifest], cargo_env)
    step('native12_public_emissions20', [args.cargo, 'run', '--locked', '--offline', '--jobs', '1',
         '--manifest-path', manifest, '--', generated, out / 'fixture', source_root / 'crates/codegen-runtime',
         binding['runtime_dll']['path']], cargo_env)
    manifest = generated / 'rust-host/Cargo.toml'
    step('Rust_host_lock', [args.cargo, 'generate-lockfile', '--offline', '--manifest-path', manifest], cargo_env)
    step('Rust12', [args.cargo, 'test', '--locked', '--offline', '--jobs', '1', '--manifest-path', manifest,
         '--test', 'windows', 'all_twelve_complete_returns_before_comparison', '--', '--exact', '--nocapture',
         '--test-threads=1'], cargo_env)
    for model in MODELS:
        build = out / ('csharp-build-' + model)
        manifest = generated / 'csharp' / model / 'consumer/Host.csproj'
        step('CSharp_' + model + '_build', [args.dotnet, 'build', manifest, '--configuration', 'Debug',
             '--disable-build-servers',
             '--nologo', '--verbosity', 'minimal', '-maxcpucount:1', '-nodeReuse:false',
             '-p:UseSharedCompilation=false', '-p:BuildInParallel=false', '-p:NuGetAudit=false',
             '-p:DisableTransitiveFrameworkReferenceDownloads=true', '-p:AutomaticallyUseReferenceAssemblyPackages=false',
             '-p:EnableTargetingPackDownload=false', '-p:EnableRuntimePackDownload=false',
             '-p:RestoreConfigFile=' + str(out / 'NuGet.Config'),
             '-p:BaseIntermediateOutputPath=' + str(build / 'obj') + '/', '-p:OutputPath=' + str(build / 'bin') + '/',
             '-p:AppendTargetFrameworkToOutputPath=false', '-p:AppendRuntimeIdentifierToOutputPath=false'], dotnet_env)
        step('CSharp_' + model + '_calls', [args.dotnet, build / 'bin/Ferrule.BasicWindows.ExternalProbe.dll',
             out / 'fixture/programs.json', out / 'fixture/expected.json'], dotnet_env)
    step('ALL36_classification', [sys.executable, out / 'fixture/prepare.py', 'classify', '--capture-root', out], {})
    assert len(steps) == 25
    tools = {name: read(path)[1] for name, path in
             (('cargo', args.cargo), ('dotnet', args.dotnet), ('python', sys.executable))}
    plan = {'issue': 704, 'status': 'PREPARED_UNRUN', 'binding': binding_pin, 'steps': steps, 'tools': tools,
            'original_inputs': {name: pin for name, (_, pin) in pins.items()}, 'materialized_inputs': materialized,
            'counts': {**COUNTS, 'commands': 25, 'backend_emissions': 20}, **FLAGS}
    print(json.dumps(json_new(out / 'COMMAND_PLAN.json', plan)))


def run(args):
    body, plan_pin = read(args.plan)
    plan = json.loads(body)
    root = Path(args.plan).resolve().parent
    reached, launcher_error = [], None
    try:
        for step in plan['steps']:
            _, binding_pin = check_binding(plan['binding']['path'])
            if binding_pin != plan['binding']:
                raise ValueError('Runtime binding changed after preparation')
            for pin in plan['tools'].values():
                read(pin['path'], pin['sha256'])
            for pin in plan['materialized_inputs'].values():
                read(pin['path'], pin['sha256'])
            env = {**os.environ, **step['environment_delta']}
            # Inherited native-reference mode must never suppress public emissions.
            env.pop('FERRULE_CONFORMANCE_NATIVE_REFERENCE_ONLY', None)
            json_new(root / (step['name'] + '.COMMAND.json'), step)
            with Path(step['stdout']).open('xb') as stdout, Path(step['stderr']).open('xb') as stderr:
                completed = subprocess.run(step['argv'], cwd=step['working_directory'], env=env,
                                           stdout=stdout, stderr=stderr, check=False)
            receipt = {**step, 'exit_code': completed.returncode,
                       'stdout_pin': read(step['stdout'])[1], 'stderr_pin': read(step['stderr'])[1]}
            reached.append(receipt)
            json_new(root / (step['name'] + '.RESULT.json'), receipt)
            if completed.returncode:
                break
    except Exception as error:
        launcher_error = {'type': type(error).__name__, 'message': str(error)}
        raise
    finally:
        json_new(root / 'EXECUTION.json', {'issue': 704, 'plan': plan_pin, 'reached': reached,
                 'unrun': [step['name'] for step in plan['steps'][len(reached):]],
                 'launcher_error': launcher_error,
                 'qualified': launcher_error is None and len(reached) == 25
                 and all(row['exit_code'] == 0 for row in reached), **FLAGS})
    if len(reached) != 25 or any(row['exit_code'] for row in reached):
        raise SystemExit(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='mode', required=True)
    snapshot_parser = sub.add_parser('snapshot')
    snapshot_parser.add_argument('--source-root', required=True)
    snapshot_parser.add_argument('--out', required=True)
    bind_parser = sub.add_parser('bind')
    for option in ('snapshot', 'rust-evidence', 'csharp-evidence', 'runtime-dll', 'out'):
        bind_parser.add_argument('--' + option, required=True)
    prepare_parser = sub.add_parser('prepare')
    for option in ('out', 'cargo', 'dotnet', 'target', 'binding'):
        prepare_parser.add_argument('--' + option, required=True)
    classify_parser = sub.add_parser('classify')
    classify_parser.add_argument('--capture-root', required=True)
    run_parser = sub.add_parser('run')
    run_parser.add_argument('--plan', required=True)
    sub.add_parser('check')
    args = parser.parse_args()
    if args.mode == 'snapshot':
        print(json.dumps(json_new(args.out, source_snapshot(args.source_root))))
    elif args.mode == 'check':
        _, spec, expected = sources()
        print(json.dumps({'issue': 704, **spec['exact_counts'], 'complete_oracles': len(expected['calls']), **FLAGS}))
    else:
        {'bind': bind, 'prepare': prepare, 'classify': classify, 'run': run}[args.mode](args)


if __name__ == '__main__':
    main()
