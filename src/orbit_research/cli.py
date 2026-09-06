"""JSON-only command results; imports are always dry runs in milestone one."""
import argparse
import json
from pathlib import Path
import sys
import sqlite3

from .contract import reference, reconcile, validate
from .native import OPERATIONS, Owner, write_json_new
from .orbit_path import task_context
from importlib.resources import files
import subprocess
from .importers import PATTERNS, import_source, strict_json, write_report
from .index import IndexBuildError, rebuild, trace as index_trace
from .browser import export_browser


def main(argv=None):
    parser = argparse.ArgumentParser(prog='orbit-research')
    commands = parser.add_subparsers(dest='command', required=True)
    index = commands.add_parser('index', help='atomically rebuild a disposable SQLite projection')
    index.add_argument('--config', type=Path, required=True)
    index.add_argument('--database', type=Path, required=True)
    browse = commands.add_parser('browse-export', help='export a portable local static evidence browser')
    browse.add_argument('--config', type=Path, required=True)
    browse.add_argument('--database', type=Path, required=True)
    browse.add_argument('--output', type=Path, required=True)
    indexed_trace = commands.add_parser('index-trace', help='trace an exact indexed snapshot including assessments')
    indexed_trace.add_argument('--database', type=Path, required=True)
    indexed_trace.add_argument('--key', required=True)
    resource = commands.add_parser('resource', help='print packaged native workflow instructions')
    resource.add_argument('--version', choices=['1'], default='1')
    context = commands.add_parser('task-context', help='read an assigned Orbit task through the registered CLI')
    for field in ('orbit-root', 'host', 'workspace', 'task', 'run'):
        context.add_argument('--' + field, required=True)
    context.add_argument('--orbit-executable', default='orbit', help='operator-selected CLI or remote wrapper')
    check = commands.add_parser('validate', help='validate a v1 record, manifest or migration report')
    check.add_argument('input', type=Path)
    check.add_argument('--target', type=Path, action='append', default=[],
                       help='independently validated record for exact reference resolution; repeatable')
    imp = commands.add_parser('import', help='inventory sources and emit read-only migration candidates')
    imp.add_argument('adapter', choices=PATTERNS)
    imp.add_argument('--source-root', required=True, type=Path)
    imp.add_argument('--repository', required=True, help='stable owner namespace, independent of filesystem path')
    imp.add_argument('--expect-revision', help='exact Git HEAD required before reading')
    imp.add_argument('--select', action='append', help='relative input file; repeat to override default discovery')
    imp.add_argument('--dry-run', action='store_true', default=True, help='default and only supported mode')
    imp.add_argument('--output', type=Path, help='new output file outside source root; default stdout')
    for name in [*OPERATIONS, 'heads', 'ref', 'trace', 'export']:
        cmd = commands.add_parser(name)
        cmd.add_argument('--owner-root', type=Path, required=True)
        cmd.add_argument('--repository', required=True)
        cmd.add_argument('--records', default='research/records')
        cmd.add_argument('--source', action='append', default=[], metavar='REPOSITORY=ROOT')
        if name in OPERATIONS:
            cmd.add_argument('--request', type=Path, required=True)
        if name in {'heads', 'ref', 'trace'}:
            cmd.add_argument('--id', required=True, help='full canonical URN')
        if name in {'ref', 'trace'}:
            cmd.add_argument('--revision', required=True)
        if name in {'ref', 'export'}:
            cmd.add_argument('--source-revision', required=True)
        if name == 'export':
            cmd.add_argument('--output', type=Path, required=True)
    rec = commands.add_parser('reconcile')
    rec.add_argument('input', type=Path)
    rec.add_argument('--target', type=Path, action='append', default=[])
    rec.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if args.command in {'index', 'browse-export', 'index-trace'}:
            if args.command == 'index':
                result = rebuild(args.config, args.database)
            elif args.command == 'browse-export':
                result = export_browser(args.database, args.output, config_path=args.config)
            else:
                result = index_trace(args.database, args.key)
            print(json.dumps(result, ensure_ascii=False, allow_nan=False))
            return 0
        if args.command == 'resource':
            print(json.dumps(dict(version=1, skill=files('orbit_research').joinpath('resources/v1/SKILL.md').read_text())))
            return 0
        if args.command == 'task-context':
            print(json.dumps(task_context(orbit_root=args.orbit_root, host=args.host, workspace=args.workspace,
                                          task=args.task, run=args.run, executable=args.orbit_executable)))
            return 0
        if args.command == 'validate':
            errors = validate(strict_json(args.input.read_bytes()),
                              targets=[strict_json(p.read_bytes()) for p in args.target])
            print(json.dumps(dict(valid=not errors, errors=errors)))
            return 1 if errors else 0
        if args.command == 'reconcile':
            targets = [strict_json(p.read_bytes()) for p in args.target]
            result = reconcile(strict_json(args.input.read_bytes()), targets)
            errors = validate(result, targets=targets)
            if errors:
                raise ValueError('; '.join(errors))
            write_json_new(result, args.output)
            print(json.dumps(dict(output=str(args.output))))
            return 0
        if args.command != 'import':
            sources = {}
            for value in args.source:
                name, sep, root = value.partition('=')
                if not sep or not name or not root or name in sources:
                    raise ValueError('source routing requires unique REPOSITORY=ROOT mappings')
                sources[name] = root
            owner = Owner(args.owner_root, args.repository, records=args.records, sources=sources)
            if args.command in OPERATIONS:
                result = owner.apply(args.command, strict_json(args.request.read_bytes()))
            elif args.command == 'heads':
                result = dict(heads=owner.heads(args.id))
            elif args.command == 'ref':
                result = reference(owner.pin(args.id, args.revision, args.source_revision), 'resolved')
            elif args.command == 'trace':
                result = owner.trace(args.id, args.revision)
            else:
                if args.output.resolve().is_relative_to(owner.directory.resolve()):
                    raise ValueError('export cannot write inside canonical records')
                result = owner.export(args.source_revision)
                write_json_new(result, args.output)
                result = dict(output=str(args.output), records=len(result['records']), manifests=len(result['manifests']))
            print(json.dumps(result, ensure_ascii=False, allow_nan=False))
            return 0
        report = import_source(args.source_root, args.adapter, args.repository,
                               selected=args.select, expected_revision=args.expect_revision)
        errors = validate(report)
        if errors:
            raise ValueError('candidate validation failed: ' + '; '.join(errors[:20]))
        if args.output:
            write_report(report, args.output, [args.source_root])
            print(json.dumps(dict(output=str(args.output), counts=report['counts'], source_unchanged=True)))
        else:
            print(json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False))
        return 0
    except IndexBuildError as exc:
        print(json.dumps(dict(error=dict(code='invalid-owner-documents', message=str(exc)), problems=exc.problems)), file=sys.stderr)
        return 2
    except (ValueError, OSError, KeyError, TypeError, sqlite3.Error, subprocess.SubprocessError) as exc:
        print(json.dumps(dict(error=dict(code='invalid-input', message=str(exc)))), file=sys.stderr)
        return 2
