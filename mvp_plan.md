# PyAIChat — Implementation Plan

## Overview

PyAIChat is implemented as a Rust host application that embeds a Python interpreter. The Rust application manages the REPL, LLM integration, and capability system, while the embedded Python maintains session state and executes user code.

## Decision Locks (Current)

- `TAB` always toggles mode in Phase 1 (completion deferred)
- Prompt format for Phase 1: `py>` / `ai>`
- Assistant mode in Phase 1 returns a fixed placeholder response
- Python strategy: system Python now, vendored distribution deferred
- Phase 1 remains synchronous; async runtime wiring is deferred to LLM phase
- Phase 1 requires automated PTY E2E coverage (using `expectrl`)
- Rust edition baseline: 2024

## Architecture Layers

```
┌─────────────────────────────────────────┐
│         Rust Host Application           │
│  ┌─────────────────────────────────┐    │
│  │  CLI & REPL Loop                │    │
│  │  (mode switching, input handling)│    │
│  └─────────────────────────────────┘    │
│  ┌─────────────────────────────────┐    │
│  │  Agent Loop                     │    │
│  │  (multi-step reasoning,        │    │
│  │   capability orchestration)     │    │
│  └─────────────────────────────────┘    │
│  ┌─────────────────────────────────┐    │
│  │  LLM Integration Layer          │    │
│  │  (abstract provider interface)  │    │
│  └─────────────────────────────────┘    │
│  ┌─────────────────────────────────┐    │
│  │  Capability System              │    │
│  │  (read-only interface to Python)│   │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
                    ↓ PyO3
┌─────────────────────────────────────────┐
│      Embedded Python Interpreter        │
│  (persistent session, state, execution) │
└─────────────────────────────────────────┘
```

## Implementation Phases

### Phase 1: Rust Host Foundation

**Goal**: Set up the basic Rust application with CLI and Python embedding.

- Initialize Rust project (Cargo.toml with dependencies)
- Set up PyO3 for Python interpreter embedding
- Implement basic REPL loop with two modes (Python/Assistant)
- Implement TAB key handler for mode switching
- Add prompt rendering with current mode indicator

**Dependencies**:
- `pyo3` - Python interpreter embedding
- `crossterm` or `rustyline` - CLI and terminal handling
- `anyhow` - Error handling
- `tokio` - Async runtime (for LLM calls, deferred from Phase 1)

**Deliverables**:
- Running Rust binary that starts a REPL
- Mode switching functional
- Python interpreter initialized and persistent
- Automated PTY E2E tests covering startup prompt, TAB toggling, and assistant placeholder behavior

---

### Phase 2: Python Runtime Interface

**Goal**: Establish the bridge between Rust and the embedded Python interpreter.

_Deferred from Phase 1_: public exec/eval runtime API and exception capture/query surface.

- Create Python module structure for runtime access
- Implement persistent Python session management
- Add exception capture and storage mechanism
- Build PyO3 bindings for:
  - Executing Python code (`exec()`)
  - Evaluating expressions (`eval()`)
  - Retrieving globals dictionary
  - Accessing last exception
- Add session state persistence between commands

**Deliverables**:
- Rust can execute Python code and capture output/errors
- Python session maintains state across commands
- Exceptions are captured and accessible

---

### Phase 3: Capability System

**Goal**: Implement the capability inspection interface between Rust and Python for trusted local MVP usage.

**Core Capabilities**:

1. `list_globals()` → Return all variables in current scope
2. `inspect(expr)` → Return structured inspection payload for expression
3. `eval_expr(expr)` → Evaluate expression (MVP: unrestricted)

**Implementation**:
- Define capability trait/interface in Rust
- Implement each capability as PyO3 binding to Python
- Add Python-side helper functions for introspection
- Keep MVP guardrails lightweight:
  - Output truncation caps for large capability responses
  - Internal helper filtering for `list_globals`
  - No AST validation/sandbox/timeouts in this phase (deferred)

**Deliverables**:
- All 3 capabilities implemented
- Well-defined Rust interface
- MVP security posture documented (trusted local use; hardening deferred)

---

### Phase 4: LLM Integration

**Goal**: Integrate LLM backend for conversational assistant functionality.

- Define abstract LLM provider trait
- Implement Gemini provider for Gemini Developer API
- Design prompt templates:
  - System prompt for direct-answer assistant behavior
  - Response formatting instructions
- Build LLM client using raw `reqwest` REST calls (no provider SDK)
- Add API key management via environment variables
  - `GEMINI_API_KEY` (required)
  - `GEMINI_MODEL` (optional with default)
- Load `.env` at startup without overriding existing shell environment variables
- Keep assistant behavior direct-answer only in this phase (no capability/tool loop yet)
- Return API/network errors to the REPL with actionable messages (no retries in this phase)
- Streaming response handling deferred

**Dependencies**:
- `reqwest` - HTTP client
- `serde`, `serde_json` - Serialization
- `dotenvy` - `.env` loading for local configuration

**Deliverables**:
- Working Gemini integration
- Prompt templates defined
- Response parsing functional
- Assistant mode reads API settings from env / `.env`

---

### Phase 5: Agent Interaction Loop

**Goal**: Implement bounded multi-step reasoning with capability orchestration.

- Design bounded agent loop structure:
  - `max_steps = 6`
  - `per_step_timeout = 8s`
  - `total_timeout = 20s`
  - `invalid_response_retries = 1` (shared path for malformed/empty/unusable model responses)
- Use Gemini-native function-calling protocol (no custom JSON action envelope):
  - Send capability declarations via `tools.functionDeclarations`
  - Keep tool-calling mode `AUTO` only
  - Allow model to answer directly without any tool calls
- Implement capability dispatch logic:
  - Parse `functionCall` parts from selected model candidate
  - Execute calls through `CapabilityProvider` in deterministic order
  - Return results as `functionResponse` parts
- Implement step-by-step reasoning:
  1. User question → LLM analysis
  2. LLM requests capabilities
  3. Execute capabilities in Python
  4. Return results to LLM
  5. LLM generates final answer
- Add per-question context management across steps only (no cross-question assistant history)
- Add candidate/termination handling:
  - Do not assume `candidates[0]` is always usable
  - Respect finish/safety metadata when selecting candidate
  - Treat empty text + no tool calls as invalid response (retry once, then soft-fail)
- Add startup model capability check:
  - Fail fast with actionable message when selected model lacks function-calling support

**Deliverables**:
- Multi-step agent loop functional
- Capability orchestration working
- End-to-end question answering
- Soft-fail degradation implemented for timeout/limit/invalid-response cases

---

### Phase 6: Refinement & Polish

**Goal**: Improve UX, error handling, and robustness.

- Improve error messages at Rust/Python boundary
- Add graceful degradation for capability failures
- Enhance prompt readability
- Add basic usage instructions
- Implement environment variable configuration
- Add logging for debugging
- Security hardening:
  - Validate expressions more strictly
  - Add resource limits (memory, execution time)
  - Review PyO3 safety guarantees

**Testing**:
- Integration tests for capability system
- E2E tests for common workflows
- Error scenario testing

**Deliverables**:
- Robust, user-friendly application
- Test coverage for core paths
- Security review completed

---

## Key Technical Decisions

### LLM Provider
**Decision**: Gemini Developer API (first implementation)

**Reasoning**: Simple API-key-based setup for local development, clean fit with `.env` configuration, and still swappable later through an abstract provider trait.

### REPL Library
**Decision**: `rustyline`

**Reasoning**: Mature readline-like library, supports multiline editing, history, and custom keybindings (TAB for mode toggle).

### Python Embedding Approach
**Decision**: PyO3 with embedded interpreter

**Reasoning**: Well-maintained, type-safe bindings, good documentation, supports persistent session.

### Safety Strategy
**Decision**: Phased approach.
1. MVP (Phase 3): trusted local use with minimal guardrails (output truncation + internal helper filtering).
2. Hardening (Phase 6): AST validation/allowlist, Python-side sandboxing, execution timeouts, and stricter read-only enforcement.

---

## Directory Structure (Proposed)

```
pyaichat/
├── Cargo.toml
├── src/
│   ├── main.rs              # Entry point, REPL loop
│   ├── cli/                 # CLI and prompt handling
│   ├── python/              # Python runtime bridge
│   │   ├── mod.rs
│   │   ├── interpreter.rs   # PyO3 interpreter wrapper
│   │   └── capabilities.rs  # Capability implementations
│   ├── llm/                 # LLM integration
│   │   ├── mod.rs
│   │   ├── provider.rs      # Abstract provider trait
│   │   └── gemini.rs        # Gemini implementation
│   ├── agent/               # Agent loop and orchestration
│   │   ├── mod.rs
│   │   └── loop.rs          # Reasoning loop
│   └── config.rs            # Configuration management
└── python/
    └── helpers.py           # Python-side helper functions
```

---

## Future Enhancements (Post-MVP)

- Structured object adapters (pandas, numpy, etc.)
- Full agentic reasoning with memory
- Multi-language support
- Session persistence and snapshots
- Remote runtime support
- Advanced REPL features (syntax highlighting, rich output)
- Multiple LLM provider support
- Plugin system for custom capabilities

## Explicit Deferrals From Phase 1

- Capability APIs (`list_globals`, `inspect`, `eval_expr`)
- LLM provider implementation and agent loop wiring
- Vendored Python packaging/distribution workflow
