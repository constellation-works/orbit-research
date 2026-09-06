"""JSON-only command results; imports are always dry runs in milestone one."""
import argparse
import json
from pathlib import Path
import sys

from .contract import validate
from .importers import PATTERNS, import_source, strict_json, write_report


def main(argv=None):
    parser = argparse.ArgumentParser(prog='orbit-research')
    commands = parser.add_subparsers(dest='command', required=True)
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
    args = parser.parse_args(argv)
    try:
        if args.command == 'validate':
            errors = validate(strict_json(args.input.read_bytes()),
                              targets=[strict_json(p.read_bytes()) for p in args.target])
            print(json.dumps(dict(valid=not errors, errors=errors)))
            return 1 if errors else 0
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
    except (ValueError, OSError) as exc:
        print(json.dumps(dict(error=dict(code='invalid-input', message=str(exc)))), file=sys.stderr)
        return 2
