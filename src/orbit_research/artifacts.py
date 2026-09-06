"""Typed, opt-in owner verification of retained non-Git dataset snapshots.

The owner supplies format/schema validation; the framework verifies exact record,
content descriptor, byte members and parent pins. No dynamic plugins or data store.
"""
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Mapping, Protocol

from .contract import canonical, digest_bytes, reference
from .importers import file_digest, strict_json
from .science import require


class SchemaCheck(Protocol):
    def __call__(self, root: Path, record: dict, descriptor: dict) -> None:
        """Read only; raise ValueError if schema, units, sidecar or parent meaning differs."""


def _path(root, relative):
    path = Path(relative)
    require(not path.is_absolute() and '..' not in path.parts, 'artifact member must stay under its explicit root')
    current = root
    for part in path.parts:
        current /= part
        require(not current.is_symlink(), 'artifact member cannot be a symlink')
    require(current.resolve().is_relative_to(root) and current.is_file(), f'missing artifact member: {relative}')
    return current


@dataclass(frozen=True)
class VerifiedArtifact:
    """A recheckable capability, not a serialized claim that external bytes are trusted."""
    root: Path
    record_bytes: bytes
    descriptor_bytes: bytes
    byte_fields: tuple[tuple[str, str], ...]
    record_path: str
    semantic_check: SchemaCheck

    @property
    def record(self):
        return strict_json(self.record_bytes)

    def verify(self):
        from .contract import validate
        record, descriptor = self.record, strict_json(self.descriptor_bytes)
        require(not validate(record, _structural=True), 'invalid external artifact record')
        require(record['kind'] == 'artifact' and record['payload']['role'] == 'dataset' and record['payload']['availability'] == 'available',
                'only available dataset artifacts can use owner byte verification')
        require(digest_bytes(canonical(descriptor)) == record['payload']['snapshot_digest'], 'snapshot descriptor digest mismatch')
        require('schema' in descriptor and descriptor.get('parent_pins') == record['references'], 'schema and exact parent pins must be retained in descriptor')
        require(self.byte_fields and len(dict(self.byte_fields)) == len(self.byte_fields), 'unique descriptor byte fields required')
        require(set(dict(self.byte_fields)) == {key for key in descriptor if key.endswith('_sha256')},
                'every descriptor byte digest must be verified; no omitted data or sidecar member')
        record_path = _path(self.root, self.record_path)
        require(canonical(strict_json(record_path.read_bytes())) == self.record_bytes, 'retained artifact record was changed')
        members = []
        for field, relative in self.byte_fields:
            require(field in descriptor and isinstance(descriptor[field], str), 'byte member lacks descriptor digest')
            path = _path(self.root, relative)
            require(file_digest(path) == descriptor[field], f'artifact byte digest mismatch: {relative}')
            members.append((path, descriptor[field]))
        # Owner-specific parsing (e.g. Arrow schema/metadata, rows, units) lives in its
        # owning adapter, avoiding a pyarrow/astropy dependency in this shared package.
        self.semantic_check(self.root, record, descriptor)
        # Re-read after owner parsing: no claimed stable proof across a concurrent edit.
        require(canonical(strict_json(record_path.read_bytes())) == self.record_bytes, 'artifact record changed during verification')
        for path, digest in members:
            require(file_digest(path) == digest, 'artifact bytes changed during verification')
        return record


def verify_artifact(record: dict, *, root: Path, descriptor: dict,
                    byte_fields: Mapping[str, str], semantic_check: SchemaCheck,
                    record_path: str = 'record.json') -> VerifiedArtifact:
    """Verify an explicit immutable owner snapshot; never infer missing parent lineage."""
    require(callable(semantic_check), 'an explicit owner schema/sidecar verifier is required')
    root = Path(root).resolve(strict=True)
    require(root.is_dir(), 'artifact root must be a directory')
    proof = VerifiedArtifact(root, canonical(record), canonical(descriptor), tuple(sorted(byte_fields.items())), record_path, semantic_check)
    proof.verify()
    return proof


ArtifactResolver = Callable[[dict], VerifiedArtifact | None]


def resolved_artifact(record, resolver, cache):
    """Return true only for an exact, freshly byte-verified record; cache per operation."""
    if resolver is None or record['kind'] != 'artifact':
        return False
    key = digest_bytes(canonical(record))
    if key not in cache:
        proof = resolver(reference(record))
        if proof is None:
            cache[key] = False
        else:
            require(isinstance(proof, VerifiedArtifact), 'owner resolver must return a VerifiedArtifact capability')
            verified = proof.verify()
            require(canonical(verified) == canonical(record), 'owner verifier returned a different artifact record/pin')
            cache[key] = True
    return cache[key]
