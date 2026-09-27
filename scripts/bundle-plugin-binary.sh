#!/bin/sh
set -eu

# Place a compatible orbit-research executable at <plugin-root>/bin/orbit-research.bin,
# where the plugin launcher finds it. The plugin root is either
# this checkout's .orbit-plugin/ (the default) or an installed tree printed as "Install path" by
# `orbit plugin show research`. The binary is copied, never linked: Orbit refuses
# a plugin tree that contains a symbolic link.

usage() {
    echo "usage: $0 [--binary PATH] [PLUGIN_ROOT]" >&2
    exit 2
}

research_binary=""
plugin_root=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary) [ "$#" -ge 2 ] && [ -n "$2" ] || usage; research_binary=$2; shift 2 ;;
        -*) usage ;;
        *) [ -z "$plugin_root" ] || usage; plugin_root=$1; shift ;;
    esac
done

if [ -z "$plugin_root" ]; then
    plugin_root="$(CDPATH= cd -- "$(dirname -- "$0")/../.orbit-plugin" && pwd -P)"
fi
[ -d "$plugin_root" ] || { echo "plugin root is not a directory: $plugin_root" >&2; exit 2; }
plugin_root=$(CDPATH= cd -- "$plugin_root" && pwd -P)
launcher="$plugin_root/bin/orbit-research"
[ -f "$launcher" ] || { echo "no plugin launcher at $launcher" >&2; exit 2; }

if [ -z "$research_binary" ]; then
    research_binary=$(command -v orbit-research) || {
        echo "orbit-research is not on PATH; pass --binary PATH" >&2
        exit 1
    }
fi
[ -f "$research_binary" ] && [ -x "$research_binary" ] || {
    echo "orbit-research binary is not an executable file: $research_binary" >&2
    exit 2
}

probe=$(printf '%s\n' '{"tool":"version","input":{}}' | "$research_binary" orbit-tool 2>/dev/null) || probe=""
case "$probe" in
    *'"ok":true'*) ;;
    *) echo "candidate binary does not answer the orbit-tool exec envelope: $research_binary" >&2; exit 1 ;;
esac

target="$plugin_root/bin/orbit-research.bin"
staging="$target.tmp.$$"
trap 'rm -f "$staging"' EXIT
cp "$research_binary" "$staging"
chmod 755 "$staging"
mv -f "$staging" "$target"
trap - EXIT
echo "bundled $research_binary as $target"
