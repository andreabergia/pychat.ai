# Contributing

## Setup

1. Install Rust and Python.
2. Clone the repo.
3. Run checks:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
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

## Commit Style

Use short conventional commits, for example:

- `feat(repl): add command alias`
- `fix(agent): handle empty candidate list`
- `docs: update user guide`
