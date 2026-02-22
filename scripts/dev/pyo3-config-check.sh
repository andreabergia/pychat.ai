#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
resolve_script="${repo_root}/scripts/python/resolve-python.sh"

python_bin="$("${resolve_script}")"
export PYO3_PYTHON="${python_bin}"
export PYTHONHOME="$(cd "$(dirname "${python_bin}")/.." && pwd)"

echo "Using PYO3_PYTHON=${PYO3_PYTHON}"
echo "Using PYTHONHOME=${PYTHONHOME}"
"${PYO3_PYTHON}" --version

cd "${repo_root}"
set +e
PYO3_PRINT_CONFIG=1 cargo check
status=$?
set -e

# Cargo exits non-zero after PyO3 prints config and aborts the build on purpose.
if [[ "${status}" -ne 0 && "${status}" -ne 101 ]]; then
  exit "${status}"
fi
