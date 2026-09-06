"""Portable static export. Imported markup is text, media is opt-in and digest checked."""
import base64
from importlib.resources import files
import os
from pathlib import Path
import shutil
import tempfile
from urllib.parse import quote, urlsplit

from .contract import canonical, digest_bytes
from .index import guard_output, load_config, pin, read_index, key
from .native import git_bytes, safe_path


def local_url(value):
    """Operator mapping into a local static server, never remote or active schemes."""
    if not isinstance(value, str) or not value.startswith('/') or value.startswith('//'):
        raise ValueError('checkout URL must be a local absolute URL path')
    if any(c in value for c in ('\\', '%', '?', '#', ':')) or '..' in value.split('/'):
        raise ValueError('unsafe checkout URL')
    return value.rstrip('/') + '/'


def media_assets(config, roots, destination, nodes):
    result = []
    urls = {r: local_url(u) for r, u in config.get('checkout_urls', {}).items()}
    for item in config.get('media', []):
        # Media is explicitly selected by the operator, never discovered from legacy HTML.
        entry = dict(label=item.get('label', 'Artifact'), role=item.get('role', 'illustration'),
                     record=key(pin(item['record'])), state='inaccessible', reason='', url=None, image=None)
        result.append(entry)
        try:
            if entry['record'] not in nodes:
                raise ValueError('media record pin is absent from index')
            if entry['role'] not in {'illustration', 'empirical-evidence', 'simulation'}:
                raise ValueError('media role must explicitly distinguish illustration, empirical-evidence or simulation')
            # Remote links are visible text only. No request is made at build or page load.
            if item.get('url'):
                parsed = urlsplit(item['url'])
                if parsed.scheme != 'https' or not parsed.hostname or parsed.username or parsed.password:
                    raise ValueError('only explicit credential-free HTTPS navigation is permitted')
                entry.update(url=item['url'], state='external-unverified', reason='External content; opens only on request. Not checked or fetched.')
                continue
            root = roots[item['repository']]
            path = safe_path(root, item['path'])
            data = git_bytes(root, item['source_revision'], item['path'])
            if digest_bytes(data) != item['sha256']:
                raise ValueError('media digest differs from exact Git snapshot')
            if not path.is_file() or digest_bytes(path.read_bytes()) != item['sha256']:
                raise ValueError('media missing or changed in mapped checkout')
            # Raster signatures only; HTML, SVG, scripts and polyglot extensions are not embedded.
            suffix = path.suffix.lower()
            raster = ((suffix == '.png' and data.startswith(b'\x89PNG\r\n\x1a\n')) or
                      (suffix in {'.jpg', '.jpeg'} and data.startswith(b'\xff\xd8\xff')) or
                      (suffix == '.webp' and data[:4] == b'RIFF' and data[8:12] == b'WEBP'))
            if raster and entry['role'] != 'simulation':
                name = item['sha256'][7:] + suffix
                (destination / 'media').mkdir(exist_ok=True)
                (destination / 'media' / name).write_bytes(data)
                entry.update(image='media/' + name, url='media/' + name, state='verified-snapshot',
                             reason='Exact bytes verified; media role is an owner/operator assertion, not scientific adjudication.')
            elif item['repository'] in urls:
                entry.update(url=urls[item['repository']] + quote(item['path'], safe='/'), state='local-navigation',
                             reason='Explicit navigation to mapped checkout. Bytes verified at export; rebuild after changes. Imported scripts never run in this browser.')
            else:
                raise ValueError('non-raster artifact requires an explicit checkout_urls mapping for navigation')
        except (ValueError, OSError, KeyError, TypeError) as exc:
            entry['reason'] = str(exc)
    return result


def export_browser(database, output, *, config_path):
    config, roots, paths = load_config(config_path)
    output = guard_output(output, roots, [database, config_path, *paths])
    if output.exists():
        raise ValueError('static export destination must be new; retain the previous usable export')
    projection = read_index(database)
    output.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix='.research-browser-', dir=output.parent))
    try:
        projection['media'] = media_assets(config, roots, stage, {r['key'] for r in projection['records']})
        # JSON is inert even if it contains </script>, quotes or malicious Markdown.
        (stage / 'index.json').write_bytes(canonical(projection) + b'\n')
        encoded = base64.b64encode(canonical(projection)).decode('ascii')
        (stage / 'data.js').write_text('window.RESEARCH_DATA = JSON.parse(new TextDecoder().decode(Uint8Array.from(atob("' + encoded + '"), c => c.charCodeAt(0))));\n')
        resources = files('orbit_research').joinpath('web')
        for name in ('index.html', 'app.js', 'style.css'):
            (stage / name).write_bytes(resources.joinpath(name).read_bytes())
        os.rename(stage, output)
    finally:
        if stage.exists():
            shutil.rmtree(stage)
    return dict(output=str(output), records=len(projection['records']), content_digest=projection['content_digest'])
