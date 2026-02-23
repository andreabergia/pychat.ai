# Phase 3 Plan: Linux Packaging + CI Packaging Enhancements

## Summary

Phase 2 implemented macOS portable `dist/` packaging and a macOS packaging smoke CI job.

This Phase 3 plan captures:

- the major missing piece: Linux portable packaging + CI validation
- follow-up packaging hardening and distribution improvements across macOS/Linux
- CI enhancements for packaging artifacts and release readiness

Primary goal: extend the same reproducible + relocatable packaging guarantees from macOS to Linux, then harden the packaging pipeline for broader distribution use.

## Current State (Post-Phase 2)

Implemented and working:

- pinned Python build workflow (`.python-version` + uv-managed local Python)
- CI checks job on Linux + macOS using pinned Python (`checks`)
- `--smoke-python` CLI flag for non-interactive embedded Python startup validation
- macOS `dist/` packaging (`scripts/dist/package-macos.sh`)
- macOS moved-path relocatability smoke (`scripts/dist/smoke-macos.sh`)
- macOS packaging CI job (`package-macos`)

Missing:

- Linux portable `dist/` packaging script
- Linux packaging relocatability smoke script
- Linux packaging CI job
- Packaging artifacts upload / retention in CI
- Signing/notarization/release packaging workflows (macOS)
- Windows packaging support

## Goals / Success Criteria (Phase 3 Core)

### Core (required)

- `scripts/dist/package-linux.sh` produces a relocatable Linux `dist/` folder
- packaged Linux binary does not retain absolute repo-local Python paths
- moved-path Linux `dist/` smoke succeeds via `--smoke-python`
- CI validates Linux packaging + relocatability on `ubuntu-latest`

### Enhancements (stretch / follow-up)

- CI uploads packaged `dist/` artifacts for macOS and Linux
- packaging scripts emit machine-readable metadata (platform, python version, binary path)
- packaging docs cover both macOS and Linux flows consistently

## Out of Scope (for initial Linux packaging phase)

- Windows packaging
- macOS signing / notarization
- `.app` bundle format
- universal binaries / multi-arch release orchestration
- runtime-selectable Python versions
- minimizing bundled Python footprint (stdlib trimming) unless required for functionality

## Important Public API / Interface Changes

No additional public CLI changes are required for the Linux phase because `--smoke-python` already exists and is suitable for packaging validation.

New developer-facing interfaces to add:

- `scripts/dist/package-linux.sh`
- `scripts/dist/smoke-linux.sh`

Potential optional follow-up interfaces (not required in first Linux cut):

- `scripts/dist/package-all.sh`
- `scripts/dist/verify-dist.sh` (shared post-package checks)

## Phase 3A: Linux Portable Packaging (Decision-Complete Plan)

### 1. Add `scripts/dist/package-linux.sh`

#### Purpose

Build and assemble a relocatable Linux `dist/` folder using the pinned uv-managed Python runtime and patch ELF linkage to a relative runtime path.

#### Preconditions

- Linux only (`uname -s == Linux`)
- pinned Python installed via `scripts/python/install-managed-python.sh`
- Rust toolchain installed
- ELF tooling available:
  - `patchelf` (required)
  - `ldd` (expected on CI runner)
  - `readelf` (optional but recommended diagnostics)

#### Packaging layout (same contract as macOS where possible)

```text
dist/
├── pychat_ai                 # wrapper launcher script
├── bin/
│   └── pychat_ai-bin         # real Rust binary (patched)
└── python/
    └── runtime/              # uv-managed Python home copy
```

#### Exact packaging flow

1. Resolve pinned Python executable via `scripts/python/resolve-python.sh`
2. Derive and export:
   - `PYO3_PYTHON`
   - `PYTHONHOME`
3. Build release binary:
   - `cargo build --release`
4. Recreate `dist/`
5. Copy release binary to `dist/bin/pychat_ai-bin`
6. Copy full `$PYTHONHOME` to `dist/python/runtime` (preserve symlinks/permissions; use `rsync -a` or `cp -a`)
7. Detect bundled `libpython*.so*` under `dist/python/runtime/lib`
   - require exactly one primary match used by the binary (`libpython3.x.so.1.0` is likely)
8. Inspect binary linkage:
   - `ldd dist/bin/pychat_ai-bin`
   - optionally `readelf -d dist/bin/pychat_ai-bin`
9. Patch ELF runtime search path on the packaged binary with `patchelf`:
   - set RPATH/RUNPATH to locate bundled libpython relative to binary
   - expected relative path: `$ORIGIN/../python/runtime/lib`
10. If needed, patch interpreter-linked library references (depends on actual binary linkage shape)
    - prefer RPATH-based solution first
11. Create wrapper launcher `dist/pychat_ai`:
    - export `PYTHONHOME="$DIST_DIR/python/runtime"`
    - export `PYTHONNOUSERSITE=1`
    - `exec "$DIST_DIR/bin/pychat_ai-bin" "$@"`
12. Verify package:
    - `ldd dist/bin/pychat_ai-bin`
    - ensure no repo `.local/python` absolute path remains
    - run `dist/pychat_ai --smoke-python`

#### Notes / Implementation constraints

- Prefer `patchelf --set-rpath` on the packaged binary first; only add more invasive ELF edits if inspection shows they are required.
- The script should print diagnostics (`ldd`, `readelf -d`) on failure.
- Keep script Bash-3-compatible patterns when practical for consistency, but Linux CI can rely on modern Bash.

### 2. Add `scripts/dist/smoke-linux.sh`

#### Purpose

Validate Linux `dist/` relocatability after moving the package to a temporary path.

#### Flow

1. Linux guard (`uname -s == Linux`)
2. Ensure `dist/pychat_ai` exists
3. Create temp dir and copy `dist/` into it
4. Run moved package:
   - `<tmp>/dist/pychat_ai --smoke-python`
5. Inspect linkage:
   - `ldd <tmp>/dist/bin/pychat_ai-bin`
6. Fail if output references:
   - repo-local `.local/python`
   - known system-specific absolute Python paths that should not be required for bundled runtime

### 3. Extend CI with Linux packaging job

#### File

- `.github/workflows/ci.yml`

#### Add job (separate from `checks`)

- job name: `package-linux`
- `runs-on: ubuntu-latest`

#### Steps

1. Checkout
2. Install Rust toolchain (stable)
3. Install `uv`
4. Install system package for `patchelf` (e.g. `apt-get update && apt-get install -y patchelf`)
5. Install pinned Python (`scripts/python/install-managed-python.sh`)
6. Run `scripts/dist/package-linux.sh`
7. Run `scripts/dist/smoke-linux.sh`

#### CI assertions

- packaging script enforces no repo-local Python linkage in packaged binary
- moved-path `--smoke-python` succeeds

### 4. Docs updates (Linux packaging)

#### `docs/build-python.md`

Add Linux packaging section parallel to macOS:

- `scripts/dist/package-linux.sh`
- `scripts/dist/smoke-linux.sh`
- ELF patching with `patchelf`
- verification with `ldd` (and optional `readelf -d`)
- moved-path smoke command

#### `README.md` (optional small update)

Adjust packaging note to mention Linux packaging once implemented.

### 5. Acceptance Criteria (Phase 3A)

- `scripts/dist/package-linux.sh` exists and runs on Linux
- `scripts/dist/smoke-linux.sh` exists and validates moved-path package
- packaged Linux binary uses relative runtime search path (`$ORIGIN/...`) for bundled libpython
- `--smoke-python` succeeds from original and moved Linux `dist/`
- CI `package-linux` job passes on `ubuntu-latest`
- docs explain Linux packaging workflow and verification commands

## Phase 3B: Packaging/CI Enhancements Backlog (Future Work)

This section records improvements that are useful after Linux packaging lands.

### A. Shared packaging verification and code reuse

- Extract shared shell helpers for:
  - repo root resolution
  - copy-tree fallback (`rsync`/`cp -a`)
  - linkage assertion helpers
  - common smoke invocation
- Add `scripts/dist/verify-dist-<platform>.sh` or a shared `scripts/dist/lib/common.sh`
- Reduce duplication between macOS and Linux packaging scripts

### B. CI artifacts and inspection outputs

- Upload packaged `dist/` directories as CI artifacts for `package-macos` and `package-linux`
- Include metadata file in artifact (git SHA, OS, Python version, Rust target, timestamp)
- Keep artifact retention short for PRs; longer for release branches/tags
- Optionally upload linkage reports (`otool -L`, `otool -D`, `ldd`, `readelf -d`) as text artifacts

### C. Release pipeline readiness

- Add tag-triggered release packaging jobs
- Produce compressed archives:
  - `pychat-ai-macos-aarch64.tar.gz`
  - `pychat-ai-linux-x86_64.tar.gz` (or actual architecture used)
- Generate checksums (SHA256)
- Add release notes scaffolding

### D. macOS hardening (deferred from Phase 2)

- code signing
- notarization
- Gatekeeper validation in CI (where feasible)
- consider `.app` bundle packaging as separate product/distribution track

### E. Linux portability hardening

- test on multiple glibc baselines (or containerized build environment)
- evaluate manylinux-like compatibility constraints vs distro-specific builds
- document supported distro/runtime assumptions
- validate non-default install paths and users without Python preinstalled

### F. Windows packaging (future platform expansion)

- define Windows `dist/` layout and launcher behavior
- determine embedded Python runtime packaging strategy (DLLs + stdlib)
- add `--smoke-python` usage in PowerShell/CMD smoke scripts
- add `package-windows` CI job on `windows-latest`

### G. Package size and performance improvements

- audit bundled runtime size
- evaluate optional pruning (tests, caches, unnecessary files) with explicit allowlist
- ensure pruning does not break stdlib imports used by PyO3 initialization or REPL features

### H. Security and supply-chain hygiene

- record exact Python runtime provenance/version in packaged metadata
- generate SBOM (if desired)
- hash/verify packaged runtime files
- tighten CI shell script error handling/logging consistency

## Suggested Sequencing (Implementation Order)

1. Phase 3A core Linux packaging scripts (`package-linux.sh`, `smoke-linux.sh`)
2. CI `package-linux` job
3. Linux docs updates
4. CI artifact uploads for macOS/Linux packaging jobs
5. Shared packaging helpers/refactor
6. Release pipeline work (archives/checksums)
7. macOS signing/notarization and/or Windows packaging as separate tracks

## Test Scenarios / Verification Checklist

### Local Linux (manual)

1. `scripts/dist/package-linux.sh` builds `dist/`
2. `ldd dist/bin/pychat_ai-bin` shows bundled libpython resolution path behavior (no repo `.local/python` requirement)
3. `dist/pychat_ai --smoke-python` succeeds
4. `scripts/dist/smoke-linux.sh` succeeds after temp-path copy
5. failure diagnostics are actionable when `patchelf` is missing or patching fails

### CI

1. `checks (ubuntu-latest)` still passes (regression guard)
2. new `package-linux` job passes
3. existing `package-macos` job remains green

### Cross-platform regression

1. macOS packaging scripts continue to work unchanged
2. `--smoke-python` remains stable and non-interactive
3. docs remain accurate for both macOS and Linux flows

## Commit Plan (for a future Linux implementation)

1. `feat(dist): add linux packaging and relocatability smoke scripts`
2. `ci: add linux packaging smoke job`
3. `docs: document linux dist packaging workflow`
4. `ci: upload packaging artifacts` (optional follow-up)

## Assumptions and Defaults

- Phase 2 macOS packaging remains the reference implementation pattern
- Linux packaging target starts with `ubuntu-latest` CI and the repo's default Rust target architecture
- `patchelf` is the preferred Linux ELF patching tool
- The existing `--smoke-python` CLI flag is sufficient for packaging validation across platforms
- CI artifact upload is a follow-up enhancement, not a blocker for Linux packaging support
