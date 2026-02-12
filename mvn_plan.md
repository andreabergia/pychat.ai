# PyAIChat — Implementation Plan

## Overview

PyAIChat is implemented as a Rust host application that embeds a Python interpreter. The Rust application manages the REPL, LLM integration, and capability system, while the embedded Python maintains session state and executes user code.

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
- `tokio` - Async runtime (for LLM calls)

**Deliverables**:
- Running Rust binary that starts a REPL
- Mode switching functional
- Python interpreter initialized and persistent

---

### Phase 2: Python Runtime Interface

**Goal**: Establish the bridge between Rust and the embedded Python interpreter.

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

**Goal**: Implement the read-only capability interface between Rust and Python.

**Core Capabilities**:

1. `list_globals()` → Return all variables in current scope
2. `get_type(expr)` → Get type information for expression
3. `get_repr(expr)` → Get string representation
4. `get_dir(expr)` → List attributes/members
5. `get_doc(expr)` → Fetch documentation string
6. `eval_expr(expr)` → Evaluate expression (read-only, restricted)
7. `get_last_exception()` → Return last exception details

**Implementation**:
- Define capability trait/interface in Rust
- Implement each capability as PyO3 binding to Python
- Add safety checks:
  - Expression validation before eval
  - Read-only enforcement
  - Timeout limits
- Add Python-side helper functions for introspection

**Deliverables**:
- All 7 capabilities implemented
- Safety mechanisms in place
- Well-defined Rust interface

---

### Phase 4: LLM Integration

**Goal**: Integrate LLM backend for conversational assistant functionality.

- Define abstract LLM provider trait
- Implement provider for chosen backend (e.g., OpenAI, Anthropic)
- Design prompt templates:
  - System prompt defining capabilities
  - Capability invocation prompts
  - Response formatting instructions
- Build LLM client with retry logic
- Add API key management
- Implement streaming response handling (optional)

**Dependencies**:
- `reqwest` - HTTP client
- `serde`, `serde_json` - Serialization
- Provider SDK (e.g., `async-openai`)

**Deliverables**:
- Working LLM integration
- Prompt templates defined
- Response parsing functional

---

### Phase 5: Agent Interaction Loop

**Goal**: Implement bounded multi-step reasoning with capability orchestration.

- Design agent loop structure (max steps, timeout)
- Implement capability dispatch logic:
  - Parse LLM response for capability calls
  - Execute capabilities via Python bridge
  - Feed results back to LLM
- Implement step-by-step reasoning:
  1. User question → LLM analysis
  2. LLM requests capabilities
  3. Execute capabilities in Python
  4. Return results to LLM
  5. LLM generates final answer
- Add context management across steps
- Implement response aggregation

**Deliverables**:
- Multi-step agent loop functional
- Capability orchestration working
- End-to-end question answering

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
**Decision**: OpenAI GPT-4o (first implementation)

**Reasoning**: Strong reasoning capabilities, widely used, good tool-calling support. Easy to swap later via abstract interface.

### REPL Library
**Decision**: `rustyline`

**Reasoning**: Mature readline-like library, supports multiline editing, history, and custom keybindings (TAB for mode toggle).

### Python Embedding Approach
**Decision**: PyO3 with embedded interpreter

**Reasoning**: Well-maintained, type-safe bindings, good documentation, supports persistent session.

### Safety Strategy
**Decision**: Multi-layer approach:
1. Rust-side validation (AST inspection, allowlist)
2. Python-side sandboxing (restricted globals)
3. Execution timeouts
4. Read-only enforcement at capability level

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
│   │   └── openai.rs        # OpenAI implementation
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
