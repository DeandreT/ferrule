#!/usr/bin/env python3
"""Prepare the finite sort command plan without executing it, or classify retained 48 returns."""
import argparse
import hashlib
import json
import os
from pathlib import Path

ROLES = ('programs.json', 'expected.json', 'emitter.rs', 'emitter.Cargo.toml.template',
         'rust-caller.template.rs', 'rust-caller.Cargo.toml.template',
         'csharp-caller.template.cs', 'csharp-caller.csproj.template', 'prepare.py')
MODELS = ('ascending_scalar', 'filter_then_sort', 'sort_then_filter', 'stable_total_order',
          'descending_secondary', 'named_collision', 'raw_position_keys', 'nested_outer_phases',
          'fts_predicate_before_key', 'fts_predicate_before_target', 'stf_key_before_target',
          'stf_target_before_predicate', 'candidate_major_secondary', 'rejected_and_empty',
          'typed_owned_key', 'plain_identity_positions')
NINE = ('st_dev', 'st_ino', 'st_mode', 'st_uid', 'st_gid', 'st_nlink', 'st_size', 'st_mtime_ns', 'st_ctime_ns')
COUNTS = {'models': 16, 'calls_per_backend': 16, 'total_calls': 48}
RUST_FEATURE_ROLES = ('crates/codegen-rust/src/lib.rs', 'crates/codegen-rust/src/owned_basic.rs',
                      'crates/codegen-rust/src/tests/owned_basic.rs',
                      'crates/codegen-runtime/src/context/basic.rs', 'crates/codegen-runtime/src/context/basic_tests.rs')
CSHARP_FEATURE_ROLES = ('crates/codegen-csharp/src/lib.rs', 'crates/codegen-csharp/src/owned_basic.rs',
                        'crates/codegen-csharp/src/mapping.rs', 'crates/codegen-csharp/src/tests/owned_basic.rs',
                        'runtime/csharp/Ferrule.Runtime/ScopeContext.cs',
                        'runtime/csharp/Ferrule.Runtime.SmokeTests/BasicOwnershipContextTests.cs')


def read(path, digest=None):
    path = Path(path)
    before = path.stat()
    body = path.read_bytes()
    after = path.stat()
    nine = {name: getattr(before, name) for name in NINE}
    if nine != {name: getattr(after, name) for name in NINE} or len(body) != before.st_size:
        raise ValueError('Input changed while reading: ' + str(path))
    sha = hashlib.sha256(body).hexdigest()
    if digest is not None and sha != digest:
        raise ValueError('Input digest mismatch: ' + str(path))
    return body, {'path': str(path), 'bytes': len(body), 'sha256': sha, 'nine_stat': nine, 'complete_EOF': True}


def write_new(path, body):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as stream:
        stream.write(body)
        stream.flush()
        os.fsync(stream.fileno())
    return read(path)[1]


def json_new(path, value):
    return write_new(path, (json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + '\n').encode())


def sources(proposal_path=None, proposal_sha=None):
    if (proposal_path is None) != (proposal_sha is None):
        raise ValueError('Supply both proposal arguments or omit both for adjacent nine-file input')
    pins = {}
    proposal_pin = None
    if proposal_path is not None:
        if not Path(proposal_path).is_absolute():
            raise ValueError('Absolute source proposal required')
        body, proposal_pin = read(proposal_path, proposal_sha)
        proposal = json.loads(body)
        if proposal['issue'] != 670 or proposal['qualified'] is not False:
            raise ValueError('Source-only issue670 proposal required')
        posts = proposal['postimages']
        if set(posts) != {'conformance/basic-owned-sort/' + name for name in ROLES}:
            raise ValueError('Exact nine fixture source leaves required')
        for name in ROLES:
            descriptor = posts['conformance/basic-owned-sort/' + name]
            body, pin = read(descriptor['path'], descriptor['sha256'])
            if pin != descriptor:
                raise ValueError('Immutable full descriptor differs: ' + name)
            pins[name] = (body, pin)
        if pins['prepare.py'][1]['path'] != str(Path(__file__).resolve()):
            raise ValueError('Preparer must match its source proposal')
        mode = 'explicit_immutable_source_proposal'
    else:
        fixture = Path(__file__).resolve().parent
        pins = {name: read(fixture / name) for name in ROLES}
        mode = 'adjacent_nine_portable_files'
    spec = json.loads(pins['programs.json'][0])
    expected = json.loads(pins['expected.json'][0])
    cases = ['S' + str(index).zfill(2) for index in range(1, 17)]
    ids = ['SORT-' + backend + '-' + case for backend in ('native', 'rust', 'csharp') for case in cases]
    if spec['fixture'] != 'basic-owned-sort-v1' or spec['issue'] != 670 or expected['issue'] != 670:
        raise ValueError('Wrong fixture authority')
    if spec['exact_counts'] != COUNTS or tuple(model['id'] for model in spec['models']) != MODELS:
        raise ValueError('Exact sixteen genuine model inventory required')
    if [call['case'] for call in spec['calls']] != cases or [row['id'] for row in expected['calls']] != ids:
        raise ValueError('Exact sixteen/forty-eight call inventory required')
    if any(spec[name] is not False or expected[name] is not False for name in ('full_sort_parity', 'full_error_parity', 'total_parity')):
        raise ValueError('Finite corpus cannot claim complete parity')
    return pins, spec, expected, ids, mode, proposal_pin


def qualified(path, sha):
    body, pin = read(path, sha)
    actual = json.loads(body)
    if actual.get('qualified') is not True or actual.get('failures'):
        raise ValueError('Successful exact prerequisite required')
    if not any(actual.get(field) is True for field in ('normal_owner_closed', 'normal_owned_closure', 'normal_owner_closure')):
        raise ValueError('Normal owned closure prerequisite required')
    return actual, pin


def check_binding(args, metadata):
    body, pin = read(args.current_runtime_binding, args.current_runtime_binding_sha256)
    binding = json.loads(body)
    if binding.get('qualified') is not True or binding.get('Root_only') is not True:
        raise ValueError('Root-qualified current sort runtime binding required')
    if binding.get('main_commit') != metadata['main_commit'] or binding.get('main_tree') != metadata['main_tree']:
        raise ValueError('Sort binding is not current main')
    if any(binding.get(flag) is not True for flag in ('merged_668_and_669', 'complete_current_source_correspondence',
             'Rust_sort_and_preserved_filter_generated_drivers_qualified', 'CSharp_PUBLIC28_and_preserved_PUBLIC16_qualified',
             'fresh_Runtime95_and_Smoke216_qualified', 'no_full_sort_or_error_or_total_parity_claim')):
        raise ValueError('Exact current finite sort prerequisites missing')
    current = metadata['complete_main_roles']
    for kind, issue, roles in [('Rust', 668, RUST_FEATURE_ROLES), ('CSharp', 669, CSHARP_FEATURE_ROLES)]:
        part = binding[kind]
        actual, actual_pin = qualified(part['ACTUAL']['path'], part['ACTUAL']['sha256'])
        if actual_pin != part['ACTUAL'] or actual.get('issue') != issue:
            raise ValueError('Exact successful sort prerequisite descriptor required')
        if set(part['feature_source_correspondence']) != set(roles):
            raise ValueError('Exact focused sort source-role inventory required')
        for role in roles:
            row = part['feature_source_correspondence'][role]
            if row['current'] != current[role]:
                raise ValueError('Current feature source mismatch: ' + role)
            for descriptor in (row['current'], row['qualified']):
                _, observed = read(descriptor['path'], descriptor['sha256'])
                if observed != descriptor:
                    raise ValueError('Full source descriptor changed: ' + role)
            if (row['current']['bytes'], row['current']['sha256']) != (row['qualified']['bytes'], row['qualified']['sha256']):
                raise ValueError('Qualified feature source bytes differ: ' + role)
    for key, count in [('RustRuntime69', 69), ('CSharpRuntime95', 95), ('CSharpSmoke72', 72)]:
        rows = binding[key]
        if len(rows) != count or len({row['role'] for row in rows}) != count:
            raise ValueError('Exact complete runtime source list required: ' + key)
        for row in rows:
            if row['current'] != current[row['role']]:
                raise ValueError('Current runtime row differs')
            for descriptor in (row['current'], row['qualified']):
                _, observed = read(descriptor['path'], descriptor['sha256'])
                if observed != descriptor:
                    raise ValueError('Complete runtime source descriptor changed')
            if (row['current']['bytes'], row['current']['sha256']) != (row['qualified']['bytes'], row['qualified']['sha256']):
                raise ValueError('Runtime source bytes differ from actual qualified closure')
    _, dll = read(args.runtime_dll, args.runtime_dll_sha256)
    if dll != binding['Runtime95_DLL'] or dll != metadata['pins']['runtime_DLL']:
        raise ValueError('Fresh current Runtime95 DLL must match binding and metadata')
    return binding, pin, dll


def validate_complete_error(record, oracle):
    typed = oracle['typed_error']
    if typed is None:
        if record.get('typed_error') is not None:
            raise ValueError('Unexpected typed error in successful row')
        return
    if record['typed_error'] != typed:
        raise ValueError('Complete finite typed error envelope differs: ' + record['id'])
    if record['backend'] != 'csharp':
        if not record.get('complete_error_chain') and typed['source_chain_length']:
            raise ValueError('Full native/Rust cause chain not captured')
        return
    error = record['complete_error']
    if error['type'] != 'Ferrule.Runtime.FerruleRuntimeException' or error['hresult'] != -2146233088:
        raise ValueError('Exact CLR type/HResult changed')
    if error['data'] or error['help_link'] is not None or not error['full_native'] or not error['stack_trace']:
        raise ValueError('Complete original CLR exception payload not retained')
    runtime = error['runtime']
    permitted = {'category', 'category_numeric', 'same_captured_exception', 'Node', 'owned_same_inner', 'bridge_same_inner'}
    if typed['variant'] == 'MappingException':
        permitted.add('MappingExceptionMessage')
        if error['inner'] is not None or runtime['owned'] is not None or runtime['bridge'] is not None:
            raise ValueError('Ordinary mapping Raise unexpectedly has an inner cause')
    else:
        permitted.add('owned')
        if runtime['owned_same_inner'] is not True or error['inner'] != runtime['owned']:
            raise ValueError('Original typed owned inner cause identity or complete tree differs')
        owned = error['inner']
        if owned['type'] != 'Ferrule.Runtime.FerruleOwnedSourceException' or owned['message'] != 'Owned source operation failed.':
            raise ValueError('Exact CLR owned cause type/display changed')
        if owned['inner'] is not None or owned['owned']['kind'] != 'SourceUnavailable':
            raise ValueError('Exact bare owned failure cause changed')
        payload = owned['owned']
        if payload['owner'] != {'is_primary': False, 'name': 'Ref'} or payload['requested_name'] != 'Ref':
            raise ValueError('Typed missing named root owner/request differs')
        for key, value in payload.items():
            if key not in ('kind', 'owner', 'requested_name') and value is not None:
                raise ValueError('Unexpected owned source metadata: ' + key)
    # The two absent embedded cause fields are explicit in the complete capture.
    for key, value in runtime.items():
        if key not in permitted and key not in ('owned', 'bridge') and value is not None:
            raise ValueError('Unexpected runtime metadata: ' + key)
    if runtime['bridge'] is not None or error['owned'] is not None or error['bridge'] is not None or error['traversal'] is not None:
        raise ValueError('Unrelated typed CLR error payload changed')


def classify(args):
    pins, spec, expected, ids, mode, proposal_pin = sources(args.fixture_source_proposal, args.fixture_source_proposal_sha256)
    root = Path(args.capture_root)
    records = []
    # Read and retain all forty-eight complete original records before inspecting an oracle.
    for identifier in ids:
        parent = root / 'generated' if identifier.startswith('SORT-native-') else root / 'captures'
        body, descriptor = read(parent / (identifier + '.capture.json'))
        record = json.loads(body)
        if record['id'] != identifier or record['classification'] != 'raw_return_before_any_classification':
            raise ValueError('Exact original capture identity required')
        records.append((record, descriptor))
    barrier = {'issue': 670, 'classification': 'ALL48_ORIGINALS_RETAINED_BEFORE_CLASSIFICATION',
               'records': [{'id': record['id'], 'descriptor': descriptor} for record, descriptor in records],
               'count': 48, 'expected_original': pins['expected.json'][1], 'source_mode': mode,
               'source_proposal': proposal_pin, 'full_sort_parity': False, 'full_error_parity': False, 'total_parity': False}
    json_new(root / 'ALL48_BEFORE_COMPARISON.json', barrier)
    differences = []
    for (record, descriptor), oracle in zip(records, expected['calls']):
        if record['snapshot'] != oracle['expected']:
            differences.append({'id': record['id'], 'kind': 'complete_public_return_differs',
                                'actual_original': descriptor, 'expected': oracle['expected'], 'actual': record['snapshot']})
        try:
            validate_complete_error(record, oracle)
        except (ValueError, KeyError, TypeError) as error:
            differences.append({'id': record['id'], 'kind': 'complete_typed_error_differs',
                                'actual_original': descriptor, 'expected': oracle['typed_error'],
                                'actual': record.get('typed_error'), 'detail': str(error)})
    # Explicit per-backend expectations preserve the genuine CLR/enum shape; shared semantics must match
    # except for the native-only availability of the error's (absent) scope field.
    semantic_differences = []
    for index in range(16):
        native = dict(records[index][0]['snapshot'])
        rust = dict(records[16 + index][0]['snapshot'])
        csharp = dict(records[32 + index][0]['snapshot'])
        for result in (native, rust, csharp):
            result.pop('scope_metadata_available', None)
        if native != rust or native != csharp:
            semantic_differences.append({'case': 'S' + str(index + 1).zfill(2),
                                         'native': native, 'rust': rust, 'csharp': csharp})
    classification = {'issue': 670, 'classification': 'FINITE_SORT48_MATCHED' if not differences and not semantic_differences else 'FINITE_SORT48_DIFFERENCES_RETAINED',
                      'qualified': not differences and not semantic_differences, 'all48_raw_returns_retained_before_any_comparison': True,
                      'count': 48, 'differences': differences, 'semantic_differences': semantic_differences,
                      'full_sort_parity': False, 'full_error_parity': False, 'total_parity': False}
    json_new(root / 'SORT48_CLASSIFICATION.json', classification)
    print(json.dumps(classification, ensure_ascii=False))
    if differences or semantic_differences:
        raise SystemExit(1)


def prepare(args):
    pins, spec, expected, ids, mode, proposal_pin = sources(args.fixture_source_proposal, args.fixture_source_proposal_sha256)
    path_args = ('out', 'cargo', 'dotnet', 'python', 'target', 'source_root', 'rust_runtime', 'runtime_dll', 'current_metadata', 'current_runtime_binding')
    for key in path_args:
        if not Path(getattr(args, key)).is_absolute():
            raise ValueError('Absolute execution binding required: ' + key)
    body, metadata_pin = read(args.current_metadata, args.current_metadata_sha256)
    metadata = json.loads(body)
    if metadata['main_commit'] != args.expected_current_main:
        raise ValueError('Late-bound current main differs')
    binding, binding_pin, dll = check_binding(args, metadata)
    out = Path(args.out)
    out.mkdir(exist_ok=False)
    (out / 'captures').mkdir()
    materialized = {name: write_new(out / 'fixture' / name, body) for name, (body, _) in pins.items()}
    emitter_manifest = pins['emitter.Cargo.toml.template'][0].decode()
    for crate in ('engine', 'mapping', 'ir', 'codegen', 'codegen-rust', 'codegen-csharp'):
        token = '__' + crate.upper().replace('-', '_') + '_PATH__'
        emitter_manifest = emitter_manifest.replace(token, json.dumps(str(Path(args.source_root) / 'crates' / crate)))
    if '__' in emitter_manifest:
        raise ValueError('Unbound emitter manifest token')
    write_new(out / 'emitter/Cargo.toml', emitter_manifest.encode())
    write_new(out / 'emitter/src/main.rs', pins['emitter.rs'][0])
    write_new(out / 'NuGet.Config', b'<configuration><packageSources><clear /></packageSources></configuration>\n')
    cargo_env = {'CARGO_TARGET_DIR': args.target, 'CARGO_BUILD_JOBS': '1', 'CARGO_INCREMENTAL': '0', 'CARGO_TERM_COLOR': 'never'}
    capture_env = {'FERRULE_CONFORMANCE_CAPTURE_ONLY': '1', 'FERRULE_CONFORMANCE_CAPTURE_DIR': str(out / 'captures')}
    cargo_env.update(capture_env)
    dotnet_env = {'DOTNET_CLI_USE_MSBUILD_SERVER': '0', 'MSBUILDDISABLENODEREUSE': '1', 'DOTNET_CLI_TELEMETRY_OPTOUT': '1', 'DOTNET_SKIP_FIRST_TIME_EXPERIENCE': '1', 'DOTNET_NOLOGO': '1'}
    dotnet_env.update(capture_env)
    steps = []

    def step(name, argv, environment):
        steps.append({'name': name, 'argv': [str(value) for value in argv], 'environment_delta': environment,
                      'working_directory': str(out), 'stdout': str(out / (name + '.stdout.complete.original')),
                      'stderr': str(out / (name + '.stderr.complete.original')), 'COMMAND': str(out / (name + '.COMMAND.json'))})

    generated = out / 'generated'
    manifest = out / 'emitter/Cargo.toml'
    step('sort_emitter_lock', [args.cargo, 'generate-lockfile', '--offline', '--manifest-path', manifest], cargo_env)
    step('sort_native16_public_emissions32', [args.cargo, 'run', '--locked', '--offline', '--jobs', '1', '--manifest-path', manifest,
                                           '--', generated, out / 'fixture', args.rust_runtime, args.runtime_dll], cargo_env)
    manifest = generated / 'rust-host/Cargo.toml'
    step('sort_Rust_host_lock', [args.cargo, 'generate-lockfile', '--offline', '--manifest-path', manifest], cargo_env)
    step('sort_Rust16', [args.cargo, 'test', '--locked', '--offline', '--jobs', '1', '--manifest-path', manifest, '--test', 'sorts',
                        'all_sixteen_complete_returns_before_comparison', '--', '--exact', '--nocapture', '--test-threads=1'], cargo_env)
    for model in MODELS:
        build = out / ('csharp-build-' + model)
        manifest = generated / 'csharp' / model / 'consumer/Host.csproj'
        assembly = build / 'bin/Ferrule.BasicSort.ExternalProbe.dll'
        step('sort_CSharp_' + model + '_build', [args.dotnet, 'build', manifest, '--configuration', 'Debug', '--nologo', '--verbosity', 'minimal',
              '-maxcpucount:1', '-nodeReuse:false', '-p:UseSharedCompilation=false', '-p:BuildInParallel=false', '-p:NuGetAudit=false',
              '-p:DisableTransitiveFrameworkReferenceDownloads=true', '-p:AutomaticallyUseReferenceAssemblyPackages=false',
              '-p:EnableTargetingPackDownload=false', '-p:EnableRuntimePackDownload=false', '-p:RestoreConfigFile=' + str(out / 'NuGet.Config'),
              '-p:BaseIntermediateOutputPath=' + str(build / 'obj') + '/', '-p:OutputPath=' + str(build / 'bin') + '/',
              '-p:AppendTargetFrameworkToOutputPath=false', '-p:AppendRuntimeIdentifierToOutputPath=false'], dotnet_env)
        step('sort_CSharp_' + model + '_calls', [args.dotnet, assembly, out / 'fixture/programs.json', out / 'fixture/expected.json'], dotnet_env)
    step('sort_ALL48_capture_barrier_and_classification', [args.python, out / 'fixture/prepare.py', 'classify', '--capture-root', out], {})
    if len(steps) != 37:
        raise ValueError('Exact thirty-seven command finite plan required')
    plan = {'status': 'PREPARED_UNRUN_37_STEP_COMMAND_PLAN', 'issue': 670, 'main_commit': metadata['main_commit'], 'main_tree': metadata['main_tree'],
            'pins': {'current_metadata': metadata_pin, 'current_sort_runtime_binding': binding_pin, 'current_Runtime95_DLL': dll},
            'source_mode': mode, 'source_proposal': proposal_pin, 'original_source_inputs': {name: pin for name, (_, pin) in pins.items()},
            'materialized_fixture_inputs': materialized, 'steps': steps, 'exact_call_ids': ids,
            'counts': {'commands': 37, 'models': 16, 'backend_emissions': 32, 'native_calls': 16, 'Rust_calls': 16, 'CSharp_calls': 16, 'total': 48},
            'Root_execution_requirements': [
                'Root alone verifies full current source and runtime bindings, complete immutable toolchains, SDK, admission, source overlays, protected artifacts and normal closure before actors.',
                'Use the existing exclusive compiler lease and shared target, jobs1, incremental0, offline locked dependencies, and task-owned bounded output/cache/native directories. No extra build target or copied source world/runtime/SDK.',
                'Capture every reached complete input/result/stdout/stderr and command once. Native/Rust/CSharp capture-only modes do not compare expected returns. Run the final classifier only after all48 original structured returns exist; it first seals the complete48 descriptor barrier.',
                'Compiler or emission failure retains reached negatives and later UNRUN phases. Typed public mapping errors are successful capture records and never terminate the cohort before all48 returns.',
                'Every emitted Runtime artifact must match current qualified source; do not replace a fresh Runtime95 DLL with a historical file or claim current evidence from source-only proposals.',
                'No UI/display/portal needed for this corpus. Current default options and authentic owner scopes remain explicit. Full sort/error/total parity stays false.',
            ], 'actors_executed_by_preparation': False, 'new_runtime_credit': False, 'full_sort_parity': False, 'full_error_parity': False, 'total_parity': False}
    print(json.dumps(json_new(out / 'COMMAND_PLAN.json', plan)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='mode', required=True)
    prepare_parser = sub.add_parser('prepare')
    for option in ('out', 'cargo', 'dotnet', 'python', 'target', 'source-root', 'rust-runtime', 'runtime-dll', 'current-metadata', 'current-runtime-binding'):
        prepare_parser.add_argument('--' + option, required=True)
    for option in ('expected-current-main', 'runtime-dll-sha256', 'current-metadata-sha256', 'current-runtime-binding-sha256'):
        prepare_parser.add_argument('--' + option, required=True)
    classify_parser = sub.add_parser('classify')
    classify_parser.add_argument('--capture-root', required=True)
    for item in (prepare_parser, classify_parser):
        item.add_argument('--fixture-source-proposal')
        item.add_argument('--fixture-source-proposal-sha256')
    args = parser.parse_args()
    if args.mode == 'classify':
        classify(args)
    else:
        prepare(args)


if __name__ == '__main__':
    main()
