# Contributing

## Setup

1. Install Rust and `uv`.
2. Clone the repo.
3. Install the pinned Python version from `.python-version`:

```bash
scripts/python/install-managed-python.sh
```

4. Run checks with the pinned interpreter:

```bash
scripts/dev/checks-with-pinned-python.sh
```

If PyO3/Python linkage looks wrong locally, run:

```bash
scripts/dev/pyo3-config-check.sh
```

## Workflow

- Keep changes focused and small.
- Use conventional commits.
- Add/update tests for behavior changes.
- Keep docs aligned with behavior.

## Quality Gates

Before opening a PR, all of these should pass:

- formatting
- clippy (warnings denied)
- full test suite

## CI Matrix

GitHub Actions CI runs the `checks` job on:

- `ubuntu-latest`
- `macos-latest`

Each matrix job installs `uv`, installs the pinned Python from `.python-version`, and runs
`scripts/dev/checks-with-pinned-python.sh`.

Additional linkage diagnostics run in CI:

- `otool -L target/debug/pychat_ai`
- `ldd` on `target/debug/deps/pychat_ai-*` test binaries (Linux only)

## Commit Style

Use short conventional commits, for example:

- `feat(repl): add command alias`
- `fix(agent): handle empty candidate list`
- `docs: update user guide`
