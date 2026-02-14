# Phase 4 Plan: Gemini Provider Integration with `.env` API Key Config

## Summary
Implement Phase 4 as a direct-answer assistant backed by Gemini Developer API using `reqwest`, with configuration from environment variables (auto-loading `.env` without overriding shell env). Assistant mode will call Gemini and print the text response; capability/tool orchestration remains out of scope for Phase 4.

## Locked Decisions
- Provider target: Gemini Developer API.
- Assistant behavior in Phase 4: direct answer only, no tool/capability loop yet.
- API key variable: `GEMINI_API_KEY`.
- Model selection: `GEMINI_MODEL` with an internal default.
- `.env` precedence: OS/shell environment wins over `.env`.
- Missing key UX: show actionable error in assistant mode, keep REPL running.
- HTTP implementation: raw `reqwest` REST client.
- Retry policy: no retries in this phase.
- Tests: unit + mocked HTTP integration tests.

## Public Interfaces / Types / Config Changes

### Environment variables
- `GEMINI_API_KEY` (required for assistant requests)
- `GEMINI_MODEL` (optional; fallback default in code)
- Optional later-safe extension (not required now): `GEMINI_BASE_URL` for tests/dev stubbing, defaulting to official endpoint.

### New Rust modules
- `src/config.rs`
  - `AppConfig` struct with:
    - `gemini_api_key: Option<String>`
    - `gemini_model: String`
    - `gemini_base_url: String` (if included for testability)
  - `AppConfig::from_env()` loads `.env` and reads env vars.
- `src/llm/mod.rs`
- `src/llm/provider.rs`
  - `trait LlmProvider { async fn generate(&self, input: AssistantInput) -> Result<AssistantOutput>; }`
- `src/llm/gemini.rs`
  - `GeminiProvider` implementing `LlmProvider`
  - request/response DTOs for Gemini REST JSON.

### CLI/AppState integration
- `AppState` gains:
  - config snapshot (`AppConfig`)
  - provider instance (`GeminiProvider` or trait object)
- Assistant path in `src/cli/repl.rs` calls provider instead of placeholder text.
- Runtime entrypoint moves to async (`#[tokio::main]`) and propagates async usage into REPL handling strategy.

## Implementation Design (Decision Complete)

1. Add dependencies
- Runtime/network/config:
  - `tokio` (rt-multi-thread, macros)
  - `reqwest` (json, rustls-tls)
  - `serde`, `serde_json`
  - `dotenvy`
- Test:
  - `wiremock` (or `mockito`) for HTTP contract tests.

2. Add config loading
- On startup, call `dotenvy::dotenv().ok()` once.
- Read `GEMINI_API_KEY` and `GEMINI_MODEL`.
- Default model constant in config module (single explicit string).
- Do not crash if key missing; keep `Option<String>` and defer error to assistant invocation.

3. Define LLM provider abstraction
- Keep abstraction minimal for Phase 4:
  - input: user prompt text + optional system instruction text.
  - output: final assistant text only.
- No tool schema or function-call parsing yet.

4. Implement Gemini REST provider
- Endpoint format:
  - `POST {base_url}/v1beta/models/{model}:generateContent?key={api_key}`
- Build payload with:
  - system instruction (static prompt for PyAIChat assistant behavior constraints)
  - user message content
- Parse response text from candidate parts.
- Error mapping:
  - missing/empty response text -> typed provider error
  - non-2xx -> include status + concise body excerpt
  - transport/json parse errors -> structured provider errors.

5. Wire assistant mode
- Replace placeholder branch in assistant mode:
  - If no `GEMINI_API_KEY`: print clear guidance:
    - variable name required
    - `.env` supported
    - example line: `GEMINI_API_KEY=...`
  - Else call Gemini provider and print response.
- Preserve REPL continuity on all failures.

6. Prompting (Phase 4)
- Add a fixed system prompt (code constant/module) that tells model:
  - this is PyAIChat assistant mode
  - answer directly and concisely
  - do not claim to execute code/capabilities yet.
- Keep prompt versioned as constant for easy Phase 5 evolution.

7. Repo hygiene for secrets
- Update `.gitignore` to include `.env`.
- Add `.env.example` with placeholders:
  - `GEMINI_API_KEY=`
  - `GEMINI_MODEL=` (optional comment/default note).

## Testing Plan

### Unit tests
- `config::from_env` behavior:
  - reads model default when unset
  - respects shell env precedence over `.env` values.
- Gemini request serialization:
  - includes expected endpoint path/query and payload shape.
- Gemini response parsing:
  - valid candidate text extraction
  - empty/malformed payload handling.

### Mocked HTTP integration tests
- Success path:
  - assistant query returns expected text.
- Error paths:
  - 401/403 invalid key
  - 429 and 5xx (returned directly, no retries)
  - malformed JSON response.
- Assistant-mode behavior:
  - missing key shows setup message and does not terminate REPL flow.

### Existing tests impact
- Replace or update `assistant_mode_returns_placeholder` in `tests/e2e_repl.rs`:
  - when key absent, expect configuration error message (not placeholder).
- Keep mode-toggle and Python-mode tests unchanged.

## Acceptance Criteria
- Assistant mode sends prompt to Gemini and prints model text response.
- `.env` is auto-loaded; exported env vars still take precedence.
- Missing `GEMINI_API_KEY` produces clear non-fatal guidance.
- No tool/capability orchestration exists yet (reserved for Phase 5).
- Test suite includes unit + mocked HTTP coverage for happy and failure paths.

## Assumptions and Defaults
- Default model is a single hardcoded Gemini model string chosen at implementation time.
- Gemini Developer API key auth remains query-param based for this endpoint shape.
- Streaming responses are deferred.
- Retry logic is intentionally deferred (explicitly out of scope for this phase).
