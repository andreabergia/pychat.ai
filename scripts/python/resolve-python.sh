#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
install_dir="${repo_root}/.local/python"
version_file="${repo_root}/.python-version"

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

while IFS= read -r candidate; do
  if matches_pinned_version "${candidate}"; then
    printf '%s\n' "${candidate}"
    exit 0
  fi
done < <(find "${install_dir}" -maxdepth 3 -type f \( -name 'python3' -o -name 'python3.*' \) ! -name '*-config' | sort)

for candidate in "${install_dir}/bin/python3" "${install_dir}/bin/python"; do
  if matches_pinned_version "${candidate}"; then
    printf '%s\n' "${candidate}"
    exit 0
  fi
done

echo "error: managed Python ${python_version} not found under ${install_dir}" >&2
echo "run scripts/python/install-managed-python.sh first" >&2
exit 1
