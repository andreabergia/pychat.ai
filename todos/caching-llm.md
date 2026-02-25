---
id: caching-llm
created_at: 2026-02-23T13:14:00.369181Z
status: open
summary: Caching for LLM
---
# Implement Gemini-Native LLM Caching (Explicit `cachedContents`) for PyChat.ai

## Summary

Implement Gemini-native context caching using the explicit `cachedContents` API inside `GeminiProvider`, with an in-memory cache-handle registry and configurable TTL.

This plan does **not** add a local response replay cache. It uses Gemini caching only.

Chosen defaults from this planning conversation:
1. `Gemini-native only` (no app-side response cache).
2. `Explicit cachedContents API`.
3. `In-memory only` storage (cache metadata/handles live only for the process lifetime).
4. `TTL by config`.
5. Cache `successful` cache-resource operations only (no negative/error caching).
6. Cache `stable prefix + recent messages`.
7. Invalidate message-based caches on `any Python execution`.
8. Optimize for `within-turn prefix reuse` first (multi-step agent/tool loop in a single assistant question).

As of **February 23, 2026**, this aligns with Gemini’s documented explicit context caching (`cachedContents`) and `generateContent.cachedContent` usage, while still allowing implicit provider caching on misses.

## Why This Design Fits PyChat.ai

1. PyChat.ai is runtime-aware and stateful, so a naive local response cache risks stale answers.
2. Gemini explicit context caching reduces repeated token processing while preserving live inference on uncached suffix messages.
3. The existing code already has a clean provider seam (`LlmProvider` / `GeminiProvider`), so caching can be implemented inside the Gemini integration without changing agent logic.
4. The agent loop makes multiple LLM calls per question; within-turn prefix reuse is the highest-confidence first win.

## Scope (In / Out)

In scope:
1. Gemini explicit `cachedContents` create/reuse flow.
2. In-memory cache registry in the process.
3. TTL config in `config.toml`.
4. Prefix-building strategy for within-turn agent calls.
5. Invalidation on any Python-mode execution.
6. Trace logging for cache behavior.
7. Tests for config parsing, provider behavior, invalidation, and fallback handling.

Out of scope (first implementation):
1. Local `AssistantInput -> AssistantOutput` response replay cache.
2. Disk persistence of cache handles.
3. Cross-provider cache abstraction (keep Gemini-specific internals).
4. Gemini cache cleanup/delete API integration (optional later).
5. Across-turn aggressive reuse optimization.
6. CLI commands to inspect/clear cache (optional follow-up).

## Implementation Plan

## 1. Add Config for Gemini Context Caching

Add a new nested config section in `config.toml`:
1. `[gemini_cache] enabled = true|false`
2. `[gemini_cache] ttl_seconds = <u64>`
3. `[gemini_cache] recent_message_window = <usize>`

Defaults:
1. `enabled = false` (safe rollout; no behavior change unless user opts in).
2. `ttl_seconds = 300` (5 minutes).
3. `recent_message_window = 4` (small rolling window; enough to help multi-step tool loops).

Validation rules:
1. `ttl_seconds > 0`
2. `recent_message_window >= 0` (allow `0` to mean stable-prefix-only mode)
3. Reject unknown fields via existing `serde(deny_unknown_fields)` pattern.

Files impacted:
1. `src/config.rs` (`AppConfig`, `RawFileConfig`, parsing/validation, tests)
2. `src/lib.rs` (pass config into provider)
3. `AGENTS.md` docs are not required unless user asks for docs update in implementation turn

## 2. Extend GeminiProvider with Cache Manager State

Add Gemini-specific cache management inside `GeminiProvider`:
1. New internal field, e.g. `cache: Arc<Mutex<GeminiContextCacheState>>`
2. New config field on provider, e.g. `cache_config: GeminiContextCacheConfig`
3. New runtime invalidation epoch counter, e.g. `runtime_epoch: AtomicU64`

`GeminiContextCacheState` responsibilities:
1. Store created Gemini cache handles (`cachedContents/...`) and local expiry timestamps.
2. Key entries by a normalized prefix key.
3. Track separate namespaces for:
   - Stable prefix cache (system prompt + tools/toolConfig)
   - Message-augmented rolling prefix cache (includes selected message prefix and runtime epoch)

Chosen behavior:
1. In-memory only; nothing persisted to disk.
2. No caching of failed create attempts.
3. If a cached handle is expired locally, ignore and recreate.
4. If `generateContent` returns an error indicating invalid/missing cached content, evict local handle and retry once without cache (or recreate then retry, see section 5).

Files impacted:
1. `src/llm/gemini.rs`

## 3. Build Prefix Cache Keys (Deterministic and Safe)

Use deterministic cache keys derived from normalized request content.

Key material for all cache keys:
1. `base_url`
2. `model`
3. `tool_calling_mode`
4. `system_instruction`
5. `tools`
6. `tool_config`
7. Prefix messages included in the cache
8. `runtime_epoch` for message-based cache entries only

Normalization strategy:
1. Reuse the provider’s existing Gemini request conversion logic (`build_request` / `to_content` / `to_part`) to avoid mismatches.
2. Serialize the prefix payload to canonical JSON for hashing.
3. Hash with a stable digest (for example SHA-256 via a Rust crate) and use hex digest as the map key.

Important decision:
1. Message-based cache entries must include `runtime_epoch` so any Python execution invalidates them automatically without clearing stable prefix entries.

## 4. Define Prefix-Building Strategy (Within-Turn Reuse)

Because Gemini `cachedContent` is a prefix cache, the first version will optimize for multi-step agent calls in the same assistant turn.

Prefix selection algorithm for each `generate` call:
1. Start with the full request produced from `AssistantInput`.
2. Build a stable prefix candidate containing:
   - `system_instruction`
   - `tools`
   - `tool_config`
   - No messages
3. Build a message-augmented prefix candidate by adding a prefix of `messages` that is likely to be reused in the next step within the same turn.
4. Reserve an uncached suffix so the next LLM call can still reuse the same cached prefix.
5. Use `recent_message_window` to determine how many newest messages remain uncached.
6. Cache prefix messages as: `messages[..messages.len().saturating_sub(recent_message_window)]`

Resulting behavior:
1. If `recent_message_window = 0`, cache the full current message list prefix (mostly useful for retries).
2. If `recent_message_window > 0`, keep the newest messages uncached to improve reuse on subsequent tool-loop steps.
3. This implements your “stable prefix + recent messages” preference in a prefix-safe way by preserving a rolling uncached tail.

## 5. Integrate Gemini `cachedContents` Create + Generate Flow

Add explicit cache resource creation before `generateContent` when enabled.

Request flow in `GeminiProvider::generate`:
1. Build normal `GeminiGenerateRequest` as today.
2. If cache disabled, run existing path unchanged.
3. If cache enabled:
   - Try to resolve/create stable prefix cache.
   - Try to resolve/create message-augmented prefix cache (if there are enough messages to justify it).
   - Prefer message-augmented cache over stable-only cache.
4. Attach selected cache handle to `generateContent` request via `cachedContent`.
5. Send `generateContent`.
6. On success, parse output as today (plus cache usage metadata if available).
7. On cached-content-not-found / invalid error from Gemini:
   - Evict the offending local handle
   - Retry exactly once without `cachedContent` (or recreate then retry if prefix still eligible; choose recreate-then-retry only if the API error is clearly cache-specific)

Creation details:
1. Add a Gemini API call for `POST /v1beta/cachedContents`
2. Include TTL from config (`ttl` or equivalent Gemini field per docs)
3. Include the prefix payload (`contents`, and when present `systemInstruction`, `tools`, `toolConfig`)
4. Parse returned cache name and expiration time
5. Store in local registry

Minimal HTTP changes:
1. Existing `HttpClient::post_json` is sufficient for cache creation and generate requests
2. No `GET`, `PATCH`, or `DELETE` required in v1

## 6. Add Runtime Invalidation Hook on Python Execution

Invalidate message-based cache reuse when Python mode executes code.

Implementation behavior:
1. Any Python execution attempt in Python mode increments `GeminiProvider.runtime_epoch`.
2. Stable prefix cache entries remain valid.
3. Message-based entries become unreachable because their key includes the old epoch.

Hook location:
1. `src/cli/repl.rs` in the Python input execution path (the same path that currently calls into the embedded Python session)
2. Trigger on any Python execution submission, including failed executions (safer due to partial mutations)

Provider API addition:
1. Add a non-trait Gemini-specific method such as `GeminiProvider::notify_runtime_mutation()`
2. No changes to `LlmProvider` trait required

## 7. Add Observability and Trace Logging for Cache Behavior

Add trace lines to help validate cache efficacy and diagnose misses.

Trace events to log:
1. `ai.cache` cache lookup hit/miss (stable vs message-prefix)
2. `ai.cache` cache create success (cache name, local expiry)
3. `ai.cache` cache create failure (error summary)
4. `ai.cache` cachedContent attached to generate request
5. `ai.cache` cache invalidated due to runtime mutation
6. `ai.cache` stale/invalid cache handle eviction after provider error

Optional but recommended interface enhancement:
1. Extend token usage parsing if Gemini returns cached-token counts in `usageMetadata`
2. Surface cached token counts in trace/session summary later
3. If not implemented now, explicitly leave parsing as a follow-up and keep plan scoped to functional caching first

Files impacted:
1. `src/trace/mod.rs`
2. `src/llm/gemini.rs`

## Important Public API / Interface / Type Changes

## Config Types

Add to `src/config.rs`:
1. `pub gemini_cache: GeminiCacheConfig` on `AppConfig`
2. `pub struct GeminiCacheConfig { enabled: bool, ttl_seconds: u64, recent_message_window: usize }`
3. `RawGeminiCacheConfig` in file config parsing (`[gemini_cache]`)

## Gemini Provider Constructor

Change `GeminiProvider::new(...)` signature to accept cache config:
1. Add `gemini_cache: GeminiCacheConfig` parameter (or a Gemini-specific cloned subset)
2. Preserve current behavior when `enabled = false`

Call site updates:
1. `src/lib.rs` provider construction
2. Gemini provider tests in `src/llm/gemini.rs`

## Gemini Request/Response DTOs

Extend `src/llm/gemini.rs` internal structs:
1. `GeminiGenerateRequest` with optional `cached_content`
2. New `GeminiCreateCachedContentRequest`
3. New `GeminiCachedContentResponse` (name, expire_time, etc.)
4. Optional `usageMetadata` fields for cache-token counters if implemented now

## GeminiProvider Methods

Add methods (internal/public as appropriate):
1. `notify_runtime_mutation(&self)`
2. `resolve_cached_content_for_request(...)`
3. `create_cached_content(...)`
4. `build_cache_prefix_candidates(...)`
5. `cache_key_for_prefix(...)`
6. `evict_cache_handle(...)`

## Test Cases and Scenarios

## Config Parsing Tests (`src/config.rs`)

1. Loads defaults when `[gemini_cache]` is absent.
2. Parses valid `[gemini_cache]` values.
3. Rejects `ttl_seconds = 0`.
4. Rejects unknown fields under `[gemini_cache]`.
5. Preserves existing config behavior when cache config is disabled/defaulted.

## Gemini Provider Unit/Integration Tests (`src/llm/gemini.rs` with `wiremock`)

1. `generate` sends `cachedContent` when cache enabled and handle exists.
2. Provider creates a `cachedContents` resource before `generateContent` on cache miss.
3. Provider reuses in-memory handle until local TTL expiry.
4. Provider recreates cache after local TTL expiry.
5. Provider does not store failed cache creation responses.
6. Provider falls back correctly when `generateContent` returns invalid cached-content error.
7. Provider preserves existing non-cache behavior when cache disabled.
8. Provider serializes cached-content creation payload with `systemInstruction`, `tools`, and `toolConfig` when present.
9. Provider uses rolling prefix logic consistent with `recent_message_window`.

## REPL / Runtime Invalidation Tests

1. Python execution calls `notify_runtime_mutation()` and invalidates message-based cache namespace.
2. Stable prefix cache remains reusable after runtime invalidation.
3. Message-based cached prefix is not reused after Python execution.
4. Assistant-only turns without Python execution can reuse within-turn message-prefix cache.

## Regression Tests

1. Existing Gemini generate tests still pass with cache disabled.
2. Agent loop behavior remains unchanged (same outputs/fallback behavior).
3. Token usage aggregation remains correct (cached or uncached responses).

## Acceptance Criteria

1. With cache disabled, behavior and tests match current behavior.
2. With cache enabled, repeated multi-step assistant questions reduce repeated prefix processing by reusing Gemini `cachedContents` resources.
3. Python execution invalidates message-based cache reuse immediately.
4. Trace logs clearly show cache hits/misses/creates/invalidations.
5. Cache failures never break normal assistant behavior; provider degrades to uncached requests.

## Assumptions and Defaults Chosen

1. Default rollout is `disabled` to avoid surprise behavior changes before release.
2. Cache handles are process-local only (in-memory registry), matching your `in-memory only` choice.
3. We will not implement local response replay caching in this phase.
4. We will optimize explicit cache reuse for `within-turn` agent loops first, not across-turn reuse.
5. Message-based cache invalidation happens on any Python execution attempt (including failed exec) for safety.
6. We will not add manual cache-clear CLI commands in v1.
7. We will use Gemini explicit cache TTL from config and trust server expiry; local expiry is a best-effort guard.

## External References (Used for This Plan)

1. Gemini context caching docs (explicit and implicit caching behavior): https://ai.google.dev/gemini-api/docs/caching
2. Gemini API reference for `generateContent` / `cachedContent` request field: https://ai.google.dev/api/generate-content
3. Gemini API reference for `cachedContents` resources: https://ai.google.dev/api/caching
4. OpenAI prompt caching overview (general LLM caching context): https://platform.openai.com/docs/guides/prompt-caching
