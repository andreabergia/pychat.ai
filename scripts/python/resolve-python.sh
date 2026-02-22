#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
install_dir="${repo_root}/.local/python"

while IFS= read -r candidate; do
  if [[ -x "${candidate}" ]]; then
    printf '%s\n' "${candidate}"
    exit 0
  fi
done < <(find "${install_dir}" -maxdepth 3 -type f \( -name 'python3' -o -name 'python3.*' \) ! -name '*-config' | sort)

for candidate in "${install_dir}/bin/python3" "${install_dir}/bin/python"; do
  if [[ -x "${candidate}" ]]; then
    printf '%s\n' "${candidate}"
    exit 0
  fi
done

echo "error: managed Python not found under ${install_dir}" >&2
echo "run scripts/python/install-managed-python.sh first" >&2
exit 1
