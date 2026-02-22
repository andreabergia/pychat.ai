# Contributing

## Setup

1. Install Rust and `uv`.
2. Clone the repo.
3. Install the pinned project-managed Python runtime:

```bash
scripts/python/install-managed-python.sh
```

4. Run checks with the pinned interpreter:

```bash
scripts/dev/checks-with-pinned-python.sh
```

If PyO3 appears to bind to the wrong interpreter, run:

```bash
scripts/dev/pyo3-config-check.sh
```

## Workflow

- Keep changes focused and small.
- Use conventional commits.
- Add/update tests for behavior changes.
- Keep docs aligned with behavior.
- Prefer the pinned Python workflow when building/testing locally.

## Quality Gates

Before opening a PR, all of these should pass:

- formatting
- clippy (warnings denied)
- full test suite

## Commit Style

Use short conventional commits, for example:

- `feat(repl): add command alias`
- `fix(agent): handle empty candidate list`
- `docs: update user guide`
