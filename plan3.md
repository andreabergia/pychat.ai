# Step 3 Implementation Plan: Capability System (Decision-Complete)

## Summary
Implement a backend-only capability layer on top of `PythonSession` that provides all 7 MVP capabilities with typed Rust APIs, structured errors, and bounded output sizes. Keep REPL assistant mode unchanged (`placeholder`) and adopt an MVP security-lite posture: trusted local use, no eval restrictions, and no capability timeouts in Step 3.

## Scope and Non-Goals
1. In scope: capability trait, typed results, Python helper bindings, truncation handling, tests, module exports.
2. Out of scope: assistant-mode wiring, LLM integration, timeout enforcement, eval hardening/AST validation.

## MVP Security Posture
1. Threat model: trusted local developer workflow only.
2. Capability expressions are evaluated as regular Python in the live session.
3. No AST allowlist/blocklist, sandboxing, or timeout enforcement in Step 3.
4. Safety controls in Step 3 are UX-oriented only:
   - Output truncation (`repr/doc` length and `dir` member caps).
   - Internal helper-name filtering in `list_globals`.
5. Hardening is explicitly deferred to later phases.

## Public Interfaces and Types
1. Add `src/python/capabilities.rs` containing:
   - `pub trait CapabilityProvider`
   - `pub enum CapabilityError`
   - `pub struct GlobalEntry` (reuse/relocate current type)
   - `pub struct TypeInfo { name: String, module: String, qualified: String }`
   - `pub struct ReprInfo { repr: String, truncated: bool, original_len: usize }`
   - `pub struct DirInfo { members: Vec<String>, truncated: bool, original_len: usize }`
   - `pub struct DocInfo { doc: Option<String>, truncated: bool, original_len: usize }`
   - `pub struct EvalInfo { value_repr: String, stdout: String, stderr: String }`
2. `CapabilityProvider` methods:
   - `list_globals() -> Result<Vec<GlobalEntry>, CapabilityError>`
   - `get_type(expr: &str) -> Result<TypeInfo, CapabilityError>`
   - `get_repr(expr: &str) -> Result<ReprInfo, CapabilityError>`
   - `get_dir(expr: &str) -> Result<DirInfo, CapabilityError>`
   - `get_doc(expr: &str) -> Result<DocInfo, CapabilityError>`
   - `eval_expr(expr: &str) -> Result<EvalInfo, CapabilityError>`
   - `get_last_exception() -> Result<Option<ExceptionInfo>, CapabilityError>`
3. Implement `CapabilityProvider for PythonSession` in `src/python/interpreter.rs`.
4. Update `src/python/mod.rs` exports so Step 4/5 can consume these types directly.

## Python Runtime Helper Changes
1. Extend `src/python/runtime_helpers.py` with:
   - `_pyaichat_get_type(expr)`
   - `_pyaichat_get_repr(expr)`
   - `_pyaichat_get_dir(expr)`
   - `_pyaichat_get_doc(expr)`
2. Behavior:
   - Resolve targets by evaluating expression directly in current globals.
   - Return stable dict payloads with `ok`, data fields, and structured exception payload on failure.
   - `get_dir` returns sorted ascending list.
   - `get_doc` returns `None` when missing (not error).
3. Keep existing `_pyaichat_eval_expr`, `_pyaichat_list_globals`, `_pyaichat_get_last_exception` and align payload shape where helpful.

## Safety and Data Policy (Step 3 Decisions)
1. Eval restrictions: none beyond Python runtime semantics.
2. Timeouts: none in Step 3.
3. Output limits:
   - `repr`: max 4096 bytes/chars
   - `doc`: max 4096 bytes/chars
   - `dir`: max 256 members
4. Truncation model:
   - Not an error; return truncated payload plus metadata (`truncated`, `original_len`).
5. `list_globals` filtering:
   - Exclude `__builtins__`
   - Exclude dunder names (`__x__`)
   - Exclude internal helper names prefixed `_pyaichat_`

## Error Contract
1. `CapabilityError` variants:
   - `PythonException(ExceptionInfo)` for expression/runtime failures.
   - `InvalidResultShape(String)` for malformed helper payloads.
   - `Internal(String)` for bridge/casting/conversion failures.
2. `InvalidExpr` is not a separate Step 3 validation layer (no pre-validation).
3. Missing docstring is successful result (`DocInfo { doc: None, ... }`).

## File-by-File Implementation Steps
1. Create `src/python/capabilities.rs` with trait, result structs, constants, error enum, and truncation utilities.
2. Refactor `src/python/interpreter.rs`:
   - Move/reuse shared structs (`ExceptionInfo`, possibly `GlobalEntry`) to avoid duplication.
   - Add helper-call wrappers for new Python helper functions.
   - Implement `CapabilityProvider for PythonSession`.
   - Keep existing `run_user_input` behavior untouched.
3. Update `src/python/runtime_helpers.py` with new helper functions.
4. Update `src/python/mod.rs` re-exports.
5. Do not change `src/cli/repl.rs` assistant placeholder in Step 3.

## Tests and Scenarios
1. Unit tests for each capability success path:
   - `list_globals` returns user vars and hides internals.
   - `get_type` returns `{name,module,qualified}` correctly for builtin and user-defined objects.
   - `get_repr` returns expected repr and truncation metadata.
   - `get_dir` sorted ordering and truncation at 256.
   - `get_doc` returns `Some` for documented objects and `None` for missing docs.
   - `eval_expr` works and captures stdout/stderr.
   - `get_last_exception` unchanged behavior.
2. Failure-path tests:
   - Undefined names and runtime errors map to `CapabilityError::PythonException`.
   - Malformed helper payload maps to `InvalidResultShape` (inject via focused test helper/mocking seam in Rust utility function tests).
3. Regression tests:
   - Existing REPL and interpreter tests remain green.
4. Verification commands (non-mutating format/check/test run in implementation phase):
   - `cargo fmt --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo test`

## Acceptance Criteria
1. All 7 capabilities exposed via typed Rust interface.
2. No assistant-mode behavioral change.
3. Truncation behavior deterministic and metadata-visible.
4. Internal `_pyaichat_*` names not exposed by `list_globals`.
5. Full test suite + lint + format checks pass.

## Assumptions and Defaults Locked
1. Direct expression evaluation is allowed for inspection capabilities.
2. No timeout enforcement in Step 3.
3. No AST/read-only enforcement in Step 3 (deferred).
4. Truncation thresholds are fixed at `repr/doc=4096`, `dir=256`.
5. `get_doc` missing docs is a successful `None`.
6. `get_dir` result ordering is sorted ascending.
7. MVP usage is trusted-local only; hostile input scenarios are out of scope for Step 3.
