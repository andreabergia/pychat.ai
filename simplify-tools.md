# Simplify Tool Surface Plan

## Goal
Consolidate model-facing tools from seven to three:
1. `list_globals()`
2. `inspect(expr)`
3. `eval_expr(expr)`

`inspect(expr)` becomes the primary semantic inspection interface and subsumes the current use cases of `get_type`, `get_repr`, `get_dir`, `get_doc`, and most use of `get_last_exception` for diagnosis.

## Scope
In scope:
- Rust capability interface changes
- Python runtime helper implementation for `inspect`
- Agent tool declaration/dispatch updates
- Prompt updates to guide model behavior
- Test updates and additions
- Removal of obsolete tools/capability code/tests

Out of scope:
- Tightening `eval_expr` security (explicitly postponed)
- Provider protocol changes (Gemini wire format remains unchanged)

## Current Baseline
Current model-facing tools in `src/agent/dispatch.rs`:
- `list_globals`
- `get_type`
- `get_repr`
- `get_dir`
- `get_doc`
- `eval_expr`
- `get_last_exception`

Current capability trait in `src/python/capabilities.rs`:
- `list_globals`, `get_type`, `get_repr`, `get_dir`, `get_doc`, `eval_expr`, `get_last_exception`

## Target API

### 1) list_globals
Keep semantics mostly unchanged.

Request:
```json
{}
```

Response:
```json
{
  "ok": true,
  "result": {
    "globals": [
      {"name": "x", "type_name": "int"}
    ]
  }
}
```

### 2) inspect
Single inspection endpoint with structured output and deterministic limits.

Request:
```json
{
  "expr": "next"
}
```

Response shape (high-level):
```json
{
  "ok": true,
  "result": {
    "type": {
      "name": "function",
      "module": "builtins",
      "qualified": "builtins.function"
    },
    "kind": "none|bool|number|string|bytes|mapping|sequence|set|iterator|generator|coroutine|async_generator|callable|class|module|exception|object|other",
    "repr": {
      "text": "...",
      "truncated": false,
      "original_len": 123
    },
    "size": {
      "len": 10,
      "shape": [3, 2]
    },
    "sample": {
      "items": ["..."],
      "shown": 3,
      "total": 10,
      "truncated": true
    },
    "members": {
      "data": ["x", "y"],
      "callables": ["append", "clear"],
      "dunder_count": 42,
      "shown_per_group": 24,
      "truncated": true
    },
    "doc": {
      "text": "...",
      "truncated": false,
      "original_len": 250
    },
    "callable": {
      "module": "__main__",
      "signature": "(x)",
      "doc": null,
      "source_preview": "def next(x):\\n    x + 1",
      "source_truncated": false
    },
    "limits": {
      "repr_max_chars": 4096,
      "doc_max_chars": 4096,
      "sample_max_items": 16,
      "member_max_per_group": 24,
      "source_preview_max_chars": 1200
    }
  }
}
```

Notes:
- All top-level sections except `type` and `kind` are optional.
- `kind` is the only required semantic classifier used by the agent.
- `inspect` always returns machine-usable fields first; text blobs are secondary.
- Stable ordering: lists sorted where possible, deterministic sampling (first N).

### 3) eval_expr
Keep current behavior and shape.

Request:
```json
{"expr": "x + 1"}
```

Response:
```json
{
  "ok": true,
  "result": {
    "value_repr": "42",
    "stdout": "",
    "stderr": ""
  }
}
```

## Detailed Design

### A. Rust Type Changes
File: `src/python/capabilities.rs`

1. Add new structs:
- `InspectInfo`
- nested helper structs for sections (`InspectTypeInfo`, `InspectReprInfo`, etc.)

2. Extend trait:
- add `fn inspect(&self, expr: &str) -> CapabilityResult<InspectInfo>;`

3. Remove obsolete structs tied only to dropped tools (`TypeInfo`, `ReprInfo`, `DirInfo`, `DocInfo`), unless reused by `inspect` internals.

4. Add new constants for inspect-specific limits:
- `INSPECT_SAMPLE_MAX_ITEMS`
- `INSPECT_MEMBER_MAX_PER_GROUP`
- `INSPECT_SOURCE_PREVIEW_MAX_LEN`

### B. Python Runtime Helper
File: `src/python/runtime_helpers.py`

1. Add `_pyaichat_inspect(expr)` returning dict payload in helper envelope (`ok` + result fields).

2. Internal helper functions (private, pure formatting logic):
- `_pyaichat_safe_signature(value)`
- `_pyaichat_source_preview(value, max_chars)`
- `_pyaichat_repr_payload(value)`
- `_pyaichat_doc_payload(value)`
- `_pyaichat_members_payload(value)`
- `_pyaichat_sample_payload(value)`
- `_pyaichat_kind_of(value)`

3. Callable-specific logic:
- `callable(value)` check
- module from `__module__`
- signature via `inspect.signature` with failure-safe fallback
- source preview via `inspect.getsource` with fallback

4. Explicit edge-case handling in helper:
- `None`: returns `kind=none` and stable repr payload.
- exception instances/classes: include exception-specific metadata when available (`exc_type`, `message`, concise traceback if present on captured exception objects).
- circular/self-referential containers: never recurse structurally; only shallow sample of immediate elements via safe repr of each item.
- broken `__repr__` or `__dir__` or `__doc__`: section-level fallback (e.g., `repr_error`, `dir_error`, `doc_error`) instead of failing the entire inspect call.
- generators/coroutines/async generators: classify via `kind`; do not consume/advance iterators as part of sampling.

5. Always wrap failures in existing exception envelope behavior.

### C. Rust Interpreter Bridge
File: `src/python/interpreter.rs`

1. Add bridge method in `CapabilityProvider for PythonSession`:
- call `_pyaichat_inspect`
- validate `ok` and parse expected keys robustly

2. Parsing strategy:
- strict on required keys (`type`, `kind`)
- tolerant/optional for extra sections to avoid fragile schema coupling

3. Reuse existing helper extractors and add typed parsers for nested objects.

4. Add inspect call timeout plumbing:
- introduce a bounded timeout for `_pyaichat_inspect` call path
- map timeout to stable `internal` error payload (e.g. `inspect_timeout`)
- ensure timeout does not crash or poison session state

### D. Agent Dispatch + Tool Declarations
File: `src/agent/dispatch.rs`

1. Tool declaration target set:
- keep: `list_globals`, `eval_expr`
- add: `inspect`
- remove from model-facing declarations: `get_type`, `get_repr`, `get_dir`, `get_doc`, `get_last_exception`

2. Add `dispatch_inspect` analogous to `dispatch_get_type` pattern.

3. Response envelope remains unchanged (`ok/result` and `ok=false/error`).

4. No compatibility layer:
- delete legacy dispatch handlers for removed tools.
- unknown legacy tool names must return standard `unknown_function` error.

### E. Prompt Guidance
Files:
- `src/agent/prompt.rs`
- optionally docs in `reasoning.md` or project docs

Update instructions to bias tool strategy:
1. Discover with `list_globals` only when needed.
2. Prefer `inspect` for understanding objects and callables.
3. Use `eval_expr` for targeted verification/calculation.
4. Avoid repeated shallow probes when one `inspect` can answer.

### F. CLI/Event Rendering
File: `src/cli/repl.rs`

No protocol changes needed. Ensure compact display remains readable for larger `inspect` payloads. If needed:
- cap printed JSON preview length for tool-result lines
- keep full payload in recorded turn events

## Cutover Strategy (Single Pass)
- Implement `inspect` end-to-end and wire declarations/dispatch to exactly 3 tools.
- Remove legacy capabilities from trait, interpreter implementation, runtime helpers, dispatch declarations/handlers, and associated tests.
- Update docs (`AGENTS.md`, `mvp_plan.md`, `mvp.md`) to the new canonical tool list in the same change set.

Exit criteria:
- codebase exposes exactly one primary inspection path (`inspect`) plus `list_globals` and `eval_expr`.

## Test Plan

### Unit tests (Rust)
Files:
- `src/python/interpreter.rs` tests
- `src/agent/dispatch.rs` tests

Add cases:
1. `inspect` basic scalar (`42`) returns kind `number`.
2. `inspect` list returns size + sample truncation metadata.
3. `inspect` callable user-defined returns signature/module/source preview.
4. `inspect(None)` returns `kind=none` and correct flagging.
5. `inspect` exception instance returns exception section without crashing.
6. `inspect` handles circular container without recursion explosion.
7. `inspect` with broken `__repr__` returns warning + fallback payload.
8. `inspect` builtins handles missing source gracefully.
9. malformed helper payload maps to `InvalidResultShape`.
10. helper returns `ok=false` with Python exception maps to `python_exception` envelope.
11. inspect timeout path maps to deterministic internal timeout error (new test after timeout mechanism lands).
12. `tool_declarations` contains exactly target 3 names.
13. dispatch of removed tool names returns `unknown_function`.

### Integration/e2e tests
File: `tests/e2e_repl.rs`

Add transcript-like scenario:
- define custom `next(x)` missing return
- ask assistant why `next(42)` is `None`
- verify tool usage includes `inspect(next)` and final answer explains missing return/shadowing clearly.

### Regression tests
Ensure existing behavior remains valid:
- `list_globals` filtering
- `eval_expr` output capture
- error envelope schema unchanged

## Rollout Risks and Mitigations

1. Risk: `inspect` payload too large/noisy.
- Mitigation: strict limits + optional sections + avoid synthetic summaries.

2. Risk: parser brittleness across optional fields.
- Mitigation: require minimal core fields, make all advanced sections optional.

3. Risk: model still overuses `eval_expr`.
- Mitigation: prompt update + declaration descriptions emphasizing `inspect` first.

4. Risk: callable source retrieval failures.
- Mitigation: explicit null/fallback fields, never fail whole `inspect` because source/signature unavailable.

5. Risk: long-running introspection blocks tool loop (e.g., pathological `__repr__`).
- Mitigation: add per-capability timeout around inspect helper call; return structured timeout error and continue tool loop.

## Concrete Task Breakdown

1. Add `InspectInfo` data model and trait method in `src/python/capabilities.rs`.
2. Implement `_pyaichat_inspect` in `src/python/runtime_helpers.py`.
3. Bridge parse/validation in `src/python/interpreter.rs`.
4. Add inspect timeout plumbing in `src/python/interpreter.rs` and corresponding dispatch mapping.
5. Add `inspect` declaration + dispatch in `src/agent/dispatch.rs`.
6. Update agent prompt guidance in `src/agent/prompt.rs`.
7. Remove legacy-tool unit tests and replace with `inspect`-focused coverage.
8. Add e2e transcript scenario in `tests/e2e_repl.rs`.
9. Delete dead runtime helper functions and unused parser utilities tied to dropped tools.
10. Update docs mentioning canonical tool list.
11. Run formatting/lint/tests:
- `cargo fmt`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

## Definition of Done
- Model-facing tools are exactly: `list_globals`, `inspect`, `eval_expr`.
- `inspect` provides structured, bounded, callable-aware diagnostics.
- Existing tool response envelope is preserved.
- Tests and quality gates pass (`fmt`, `clippy`, `test`).
- Docs updated to reflect simplified tool set.
