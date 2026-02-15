## Phase 5 Plan: Bounded Agent Loop With Gemini-Native Function Calling

### Summary
Implement a new agent orchestration layer that turns assistant mode from single-shot text generation into a bounded multi-step loop using Gemini native function-calling semantics: model emits `functionCall` parts, runtime executes capabilities via `CapabilityProvider`, runtime returns `functionResponse` parts, and model returns final user text.

This phase must not use a custom text-JSON tool protocol. It must use Gemini request/response structures so wire format matches official API behavior.

### Scope and Outcomes
1. Add an agent loop with limits: max 6 steps, per-step timeout 8s, total timeout 20s.
2. Support multiple capability calls per model step (parallel intent).
3. Execute batched calls in deterministic serialized order against the single Python session.
4. Keep `eval_expr` enabled in the loop under existing MVP safety posture.
5. Use a unified recoverable-invalid-response path for malformed protocol outputs, including empty outputs.
6. Return soft-fail best-effort final answers when limits/errors prevent full completion.
7. Keep assistant conversation memory per question only (no cross-turn chat history in Phase 5).
8. Keep tool-calling mode as `AUTO` only; the model may answer directly without tool calls.

### Architecture and File-Level Changes
1. Add new module tree:
- `/Users/andry/src/pyaichat/src/agent/mod.rs`
- `/Users/andry/src/pyaichat/src/agent/loop.rs`
- `/Users/andry/src/pyaichat/src/agent/dispatch.rs`
- `/Users/andry/src/pyaichat/src/agent/prompt.rs`

2. Keep `/Users/andry/src/pyaichat/src/cli/repl.rs` thin:
- Replace direct `provider.generate(...)` call in Assistant mode with `agent.run_question(...)`.

3. Keep `/Users/andry/src/pyaichat/src/llm/gemini.rs` provider generic in responsibility:
- Provider is still transport + model invocation.
- Agent module owns orchestration of function-call loop.

4. Update `/Users/andry/src/pyaichat/src/main.rs`:
- Construct agent config and wire it into app state.
- Add model support check at startup; fail fast for models that do not support function calling.

### Public Interfaces and Type Changes
1. Update provider input/output for multi-turn + tool parts.

In `/Users/andry/src/pyaichat/src/llm/provider.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantRole {
    User,
    Model,
    Tool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssistantPart {
    Text(String),
    FunctionCall {
        id: Option<String>,
        name: String,
        args_json: serde_json::Value,
    },
    FunctionResponse {
        id: Option<String>,
        name: String,
        response_json: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantMessage {
    pub role: AssistantRole,
    pub parts: Vec<AssistantPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantInput {
    pub system_instruction: Option<String>,
    pub messages: Vec<AssistantMessage>,
    pub tools: Vec<FunctionDeclaration>,
    pub tool_calling_mode: ToolCallingMode, // AUTO only
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantCandidate {
    pub message: AssistantMessage,
    pub finish_reason: Option<String>,
    pub safety_blocked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantOutput {
    pub candidates: Vec<AssistantCandidate>,
}
```

2. Add function declaration and tool mode types aligned to Gemini.

In `/Users/andry/src/pyaichat/src/llm/provider.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters_json_schema: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallingMode {
    Auto,
}
```

3. Agent API in `/Users/andry/src/pyaichat/src/agent/mod.rs`:
```rust
pub struct AgentConfig {
    pub max_steps: usize,            // default 6
    pub per_step_timeout_ms: u64,    // default 8000
    pub total_timeout_ms: u64,       // default 20000
    pub invalid_response_retries: usize, // default 1
}

pub struct AgentAnswer {
    pub text: String,
    pub degraded: bool,
}

pub async fn run_question<P: LlmProvider, C: CapabilityProvider>(
    provider: &P,
    capabilities: &C,
    question: &str,
    config: &AgentConfig,
) -> anyhow::Result<AgentAnswer>;
```

### Gemini Protocol Contract (Authoritative)
1. Request shape for each model turn:
- `contents`: full turn history for this question.
- `systemInstruction`: protocol/system guidance.
- `tools`: one `Tool` containing capability `functionDeclarations`.
- `toolConfig.functionCallingConfig.mode`: `AUTO` only.

2. Model tool request parsing:
- Read tool calls from chosen candidate content parts:
  - `candidate.content.parts[*].functionCall`
- Each call includes:
  - `id` (optional)
  - `name` (required)
  - `args` JSON object (optional)

3. Tool result return format:
- Append chosen model content from previous response to history unchanged.
- Append one tool-result message containing one or more function responses in parts:
  - `parts[*].functionResponse = { id?, name, response }`
- If `functionCall.id` exists, echo it in `functionResponse.id`.
- Internal abstraction supports both `role=user` and `role=tool` for portability.
- Gemini provider default wire role for tool results is `user` (REST-compatible).

4. Multiple calls in one model turn:
- Collect all `functionCall` parts from the chosen candidate.
- Execute sequentially (deterministic order as received).
- Send all resulting `functionResponse` parts in one follow-up message.

5. Candidate selection (finalized):
- Do not assume `candidates[0]` is usable.
- Select first candidate that has valid content and acceptable terminal metadata.
- Reject candidates marked safety blocked or with finish reasons that indicate unusable output.
- If no usable candidate exists, treat as invalid response (recoverable once).

6. Final answer detection:
- Candidate contains no `functionCall` parts and has at least one non-empty text part.
- If no function calls and all text parts are empty, treat as invalid response (recoverable once).

### Capability Result Schema (Explicit)
All function responses use a single response envelope:
```json
{
  "ok": true,
  "result": { ... }
}
```
or
```json
{
  "ok": false,
  "error": {
    "code": "invalid_args|unknown_function|python_exception|internal",
    "message": "human readable",
    "details": { ... }
  }
}
```

Per-capability `result` payloads:
1. `list_globals`
```json
{ "globals": [{"name":"x","type_name":"int"}] }
```
2. `get_type`
```json
{ "name":"list", "module":"builtins", "qualified":"builtins.list" }
```
3. `get_repr`
```json
{ "repr":"[1, 2]", "truncated":false, "original_len":6 }
```
4. `get_dir`
```json
{ "members":["append","clear"], "truncated":false, "original_len":2 }
```
5. `get_doc`
```json
{ "doc":"... or null", "truncated":false, "original_len":123 }
```
6. `eval_expr`
```json
{ "value_repr":"42", "stdout":"", "stderr":"" }
```
7. `get_last_exception`
```json
{ "exception": null }
```
or
```json
{
  "exception": {
    "exc_type":"ZeroDivisionError",
    "message":"division by zero",
    "traceback":"Traceback ..."
  }
}
```

Python exception dispatch mapping:
```json
{
  "ok": false,
  "error": {
    "code": "python_exception",
    "message": "ZeroDivisionError: division by zero",
    "details": {
      "exc_type": "ZeroDivisionError",
      "message": "division by zero",
      "traceback": "Traceback ..."
    }
  }
}
```

### Loop Behavior (Decision-Complete)
1. Per-question state management:
- State lives in `agent::loop` as local variables only:
  - `messages: Vec<AssistantMessage>`
  - `steps_used`
  - `deadline_total`
  - `invalid_response_attempts`
- `run_question` creates this state and drops it on return.
- No state is stored in REPL/app state between assistant questions.

2. Conversation history format during a question:
- Start: one `User(Text(question))` message.
- Each iteration appends:
  - selected `Model(...)` message from provider output,
  - then tool-result message (`User` or `Tool` role with `FunctionResponse` parts) when calls were requested.

3. Execute loop:
- Wrap each provider call in `tokio::time::timeout(per_step_timeout)`.
- Also enforce total deadline by computing remaining budget per iteration.
- If remaining total budget <= 0, stop degraded.

4. Invalid response handling (unified):
- Invalid includes: no usable candidate, malformed function-call payload, no function calls + empty text.
- On first invalid response, append a corrective user message and retry once.
- If invalid persists, soft-fail degraded.

5. Stop conditions:
- Final answer obtained.
- Step count exceeds 6.
- Total timeout > 20s.
- Per-step timeout exceeded.
- Invalid response retry budget exhausted.

6. Soft-fail behavior:
- Return concise best-effort answer with limitation note (timeout, step cap, invalid response), plus confirmed findings from completed capability calls.

### Prompting Contract (Concrete Text)
`/Users/andry/src/pyaichat/src/agent/prompt.rs` defines:

```text
You are PyAIChat assistant operating over a live Python runtime via declared functions.

Rules:
1) For runtime facts (values, types, attributes, docs, last exception), call functions instead of guessing.
2) You may call multiple functions when needed.
3) When enough information is gathered, provide a concise final answer in plain text.
4) If a function returns an error, adapt and continue when possible.
5) Do not invent variables or results that were not returned by function responses.
6) Keep responses concise and technically precise.
```

### Error Handling Rules
1. Unknown function name from model:
- Return `ok=false` with `error.code="unknown_function"`, continue.

2. Invalid args payload:
- Return `ok=false` with `error.code="invalid_args"`, continue.

3. Unsupported model (startup check):
- Fail fast with clear startup error explaining model lacks function-calling support and how to set `GEMINI_MODEL`.

4. Timeout on model step:
- Per-step timeout -> degraded return.
- Total timeout -> degraded return.

5. No panics:
- All malformed payloads map to recoverable invalid-response flow.

### Testing Plan (Unit + Mocked Integration)
1. Unit tests in `/Users/andry/src/pyaichat/src/agent/dispatch.rs`:
- each tool arg mapping,
- multi-call execution ordering,
- tool error envelope shape,
- id passthrough from call to response,
- per-capability `result` schema stability.

2. Mocked integration tests in `/Users/andry/src/pyaichat/src/agent/loop.rs`:
- happy path: question -> functionCall(s) -> functionResponse(s) -> final text.
- multiple function calls in single model turn.
- unsupported function name surfaces structured error and still converges.
- empty/no-call response retries once then soft-fails.
- malformed function-call payload retries once then soft-fails.
- candidate[0] unusable but later candidate usable.
- max-step soft-fail.
- total-timeout soft-fail.
- per-step-timeout soft-fail.
- capability exception returned and loop continues.

3. Provider tests in `/Users/andry/src/pyaichat/src/llm/gemini.rs`:
- serialize tools as `tools[].functionDeclarations`.
- serialize `AUTO` mode under `toolConfig.functionCallingConfig`.
- parse returned candidates with `finishReason` and safety metadata.
- parse `content.parts[].functionCall`.
- serialize outgoing `functionResponse` parts with role mapping.

4. Keep existing REPL e2e tests passing; update only if assistant-mode visible text changes intentionally.

### Acceptance Criteria
1. Assistant mode can answer runtime-aware questions via at least one real capability round-trip.
2. Multiple function calls in one model response are executed deterministically and returned together.
3. Loop is bounded by configured step and timeout limits.
4. Tool calls/responses use Gemini-native request/response schema only.
5. Invalid or empty model outputs are retried once, then degrade gracefully.
6. Unsupported model configuration fails fast at startup with actionable guidance.
7. All checks pass before done: formatter, linter, and `cargo test`.

### Assumptions and Defaults
1. Phase 5 remains trusted-local MVP; stricter security hardening is Phase 6.
2. Context persistence is per question only.
3. Tool orchestration uses Gemini-native function-calling schema, not custom JSON action protocol.
4. Parallel intent is preserved as batched calls, but execution is serialized due shared Python session constraints.
5. Tool-calling mode is always `AUTO`; no other mode is implemented.
6. The model is free to return a direct answer without any tool calls at any step.
7. Default config values:
- `max_steps = 6`
- `per_step_timeout_ms = 8000`
- `total_timeout_ms = 20000`
- `invalid_response_retries = 1`

### Source References
- https://ai.google.dev/gemini-api/docs/function-calling
- https://ai.google.dev/gemini-api/docs/function-calling?example=meeting#rest_2
- https://ai.google.dev/api/generate-content
- https://ai.google.dev/api/caching
