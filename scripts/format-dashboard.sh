#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
case "${1:---check}" in
  --check|--write) mode="${1:---check}" ;;
  *) echo "usage: $0 [--check|--write]" >&2; exit 2 ;;
esac
cd "$repo_root"
npm exec --yes --package=prettier@3.6.2 -- prettier "$mode" 'crates/orbit-research-web/assets/dashboard/*'
