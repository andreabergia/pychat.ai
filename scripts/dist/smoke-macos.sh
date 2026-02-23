#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
dist_dir="${repo_root}/dist"

copy_tree() {
  local src="$1"
  local dst="$2"

  mkdir -p "${dst}"
  if command -v rsync >/dev/null 2>&1; then
    rsync -a "${src}/" "${dst}/"
  else
    cp -a "${src}/." "${dst}/"
  fi
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: scripts/dist/smoke-macos.sh only supports macOS (Darwin)" >&2
  exit 1
fi

for cmd in otool mktemp; do
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "error: required command not found: ${cmd}" >&2
    exit 1
  fi
done

if [[ ! -x "${dist_dir}/pychat_ai" ]]; then
  echo "error: missing packaged launcher at ${dist_dir}/pychat_ai" >&2
  echo "run scripts/dist/package-macos.sh first" >&2
  exit 1
fi

tmp_root="$(mktemp -d)"
cleanup() {
  rm -rf "${tmp_root}"
}
trap cleanup EXIT

moved_dist="${tmp_root}/dist"
copy_tree "${dist_dir}" "${moved_dist}"

echo "Running moved-path smoke check from ${moved_dist}"
"${moved_dist}/pychat_ai" --smoke-python

echo
echo "Moved binary linkage:"
otool -L "${moved_dist}/bin/pychat_ai-bin"

moved_otool_output="$(otool -L "${moved_dist}/bin/pychat_ai-bin")"
if grep -Fq "/opt/homebrew" <<<"${moved_otool_output}"; then
  echo "error: moved packaged binary still links to Homebrew path" >&2
  exit 1
fi
if grep -Fq "${repo_root}/.local/python" <<<"${moved_otool_output}"; then
  echo "error: moved packaged binary still links to repo-local managed python path" >&2
  exit 1
fi

echo
echo "Moved-path smoke passed"
