#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
version_file="${repo_root}/.python-version"
install_dir="${repo_root}/.local/python"

if ! command -v uv >/dev/null 2>&1; then
  echo "error: uv is required but was not found on PATH" >&2
  echo "install uv first, then rerun this script" >&2
  exit 1
fi

if [[ ! -f "${version_file}" ]]; then
  echo "error: missing ${version_file}" >&2
  exit 1
fi

python_version="$(tr -d '[:space:]' < "${version_file}")"
if [[ -z "${python_version}" ]]; then
  echo "error: ${version_file} is empty" >&2
  exit 1
fi

matches_pinned_version() {
  local candidate="$1"
  local candidate_version

  if [[ ! -x "${candidate}" ]]; then
    return 1
  fi

  if ! candidate_version="$("${candidate}" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))' 2>/dev/null)"; then
    return 1
  fi

  [[ "${candidate_version}" == "${python_version}" ]]
}

mkdir -p "${repo_root}/.local"

echo "Installing Python ${python_version} into ${install_dir}"
uv python install --install-dir "${install_dir}" "${python_version}"

python_bin=""
for candidate in "${install_dir}/bin/python3" "${install_dir}/bin/python"; do
  if matches_pinned_version "${candidate}"; then
    python_bin="${candidate}"
    break
  fi
done

if [[ -z "${python_bin}" ]]; then
  while IFS= read -r candidate; do
    if matches_pinned_version "${candidate}"; then
      python_bin="${candidate}"
      break
    fi
  done < <(find "${install_dir}" -maxdepth 3 -type f \( -name 'python3' -o -name 'python3.*' \) ! -name '*-config' | sort)
fi

if [[ -z "${python_bin}" ]]; then
  echo "error: could not find Python ${python_version} executable under ${install_dir}" >&2
  exit 1
fi

mkdir -p "${install_dir}/bin"
ln -sf "${python_bin}" "${install_dir}/bin/python3"

echo "Managed Python executable: ${python_bin}"
"${python_bin}" --version
