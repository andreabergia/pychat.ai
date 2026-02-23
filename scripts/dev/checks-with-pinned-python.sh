#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
resolve_script="${repo_root}/scripts/python/resolve-python.sh"

python_bin="$("${resolve_script}")"
export PYO3_PYTHON="${python_bin}"
export PYTHONHOME="$(cd "$(dirname "${python_bin}")/.." && pwd)"
python_libdir="$("${PYO3_PYTHON}" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR") or "")')"

case "$(uname -s)" in
  Linux)
    if [[ -n "${python_libdir}" && -d "${python_libdir}" ]]; then
      export LD_LIBRARY_PATH="${python_libdir}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
    fi
    ;;
  Darwin)
    if [[ -n "${python_libdir}" && -d "${python_libdir}" ]]; then
      export DYLD_LIBRARY_PATH="${python_libdir}${DYLD_LIBRARY_PATH:+:${DYLD_LIBRARY_PATH}}"
    fi
    ;;
esac

echo "Using PYO3_PYTHON=${PYO3_PYTHON}"
echo "Using PYTHONHOME=${PYTHONHOME}"
if [[ -n "${python_libdir}" ]]; then
  echo "Using PYTHON_LIBDIR=${python_libdir}"
fi
"${PYO3_PYTHON}" --version

cd "${repo_root}"
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
