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
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
