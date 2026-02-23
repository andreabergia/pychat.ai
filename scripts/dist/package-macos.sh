#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
resolve_script="${repo_root}/scripts/python/resolve-python.sh"
dist_dir="${repo_root}/dist"
packaged_bin="${dist_dir}/bin/pychat_ai-bin"
wrapper_path="${dist_dir}/pychat_ai"
runtime_dir="${dist_dir}/python/runtime"
bundled_libpython=""

on_error() {
  local line_no="${1:-unknown}"
  echo "error: packaging failed (line ${line_no})" >&2
  if [[ -n "${PYO3_PYTHON:-}" ]]; then
    echo "PYO3_PYTHON=${PYO3_PYTHON}" >&2
  fi
  if [[ -n "${PYTHONHOME:-}" ]]; then
    echo "PYTHONHOME=${PYTHONHOME}" >&2
  fi
  if [[ -d "${runtime_dir}/lib" ]]; then
    echo "Detected bundled libpython candidates:" >&2
    find "${runtime_dir}/lib" -maxdepth 1 \( -type f -o -type l \) -name 'libpython*.dylib' | sort >&2 || true
  fi
  if [[ -f "${packaged_bin}" ]]; then
    echo "otool -L ${packaged_bin}:" >&2
    otool -L "${packaged_bin}" >&2 || true
  fi
}
trap 'on_error $LINENO' ERR

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
  echo "error: scripts/dist/package-macos.sh only supports macOS (Darwin)" >&2
  exit 1
fi

for cmd in cargo otool install_name_tool; do
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "error: required command not found: ${cmd}" >&2
    exit 1
  fi
done

if [[ ! -x "${resolve_script}" ]]; then
  echo "error: missing resolver script at ${resolve_script}" >&2
  exit 1
fi

PYO3_PYTHON="$("${resolve_script}")"
PYTHONHOME="$(cd "$(dirname "${PYO3_PYTHON}")/.." && pwd)"
export PYO3_PYTHON
export PYTHONHOME

echo "Using PYO3_PYTHON=${PYO3_PYTHON}"
echo "Using PYTHONHOME=${PYTHONHOME}"

cd "${repo_root}"
cargo build --release

rm -rf "${dist_dir}"
mkdir -p "${dist_dir}/bin" "${dist_dir}/python"

cp "target/release/pychat_ai" "${packaged_bin}"
copy_tree "${PYTHONHOME}" "${runtime_dir}"

libpython_candidates=()
while IFS= read -r candidate; do
  libpython_candidates+=("${candidate}")
done < <(find "${runtime_dir}/lib" -maxdepth 1 \( -type f -o -type l \) -name 'libpython*.dylib' | sort)
if [[ "${#libpython_candidates[@]}" -ne 1 ]]; then
  echo "error: expected exactly one bundled libpython*.dylib under ${runtime_dir}/lib, found ${#libpython_candidates[@]}" >&2
  exit 1
fi
bundled_libpython="${libpython_candidates[0]}"
libpython_name="$(basename "${bundled_libpython}")"

original_libpython_path="$(
  otool -L "${packaged_bin}" \
    | awk '/libpython[0-9.]*\.dylib/ { print $1; exit }'
)"
if [[ -z "${original_libpython_path}" ]]; then
  echo "error: could not find libpython dependency in ${packaged_bin}" >&2
  exit 1
fi

loader_relative_path="@loader_path/../python/runtime/lib/${libpython_name}"

install_name_tool -change "${original_libpython_path}" "${loader_relative_path}" "${packaged_bin}"
install_name_tool -id "@loader_path/${libpython_name}" "${bundled_libpython}"

cat > "${wrapper_path}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PYTHONHOME="${script_dir}/python/runtime"
export PYTHONNOUSERSITE=1

exec "${script_dir}/bin/pychat_ai-bin" "$@"
EOF
chmod +x "${wrapper_path}"

echo "Packaged binary linkage:"
otool -L "${packaged_bin}"
echo
echo "Bundled libpython install id:"
otool -D "${bundled_libpython}"

packaged_otool_output="$(otool -L "${packaged_bin}")"
if grep -Fq "/opt/homebrew" <<<"${packaged_otool_output}"; then
  echo "error: packaged binary still links to Homebrew path" >&2
  exit 1
fi
if grep -Fq "${repo_root}/.local/python" <<<"${packaged_otool_output}"; then
  echo "error: packaged binary still links to repo-local managed python path" >&2
  exit 1
fi

echo
echo "Running packaged smoke check"
"${wrapper_path}" --smoke-python

echo
echo "Packaging complete: ${dist_dir}"
