# PyAIChat Memory File

## Project Overview
PyAIChat is a minimal command-line Python environment combining normal code execution with a conversational assistant that inspects live runtime state. Users can write/run Python code and switch to assistant mode to ask questions about variables, objects, functions, or errors.

**One-liner**: A minimal Python REPL with a conversational assistant that inspects and explains live runtime state for interactive exploration.

It has not been released yet, so we do not have to worry about backward compatibility or preserving any user's settings.

## Core Architecture

### Tech Stack
- **Rust host application** written in Rust using PyO3 to embed Python interpreter
- **LLM Provider**: OpenAI GPT-4o (first implementation, swappable via abstract interface)
- **REPL Library**: `rustyline`
- **Python Embedding**: PyO3 with embedded interpreter

### Architecture Layers
```
Rust Host Application
├── CLI & REPL Loop (mode switching, input handling)
├── Agent Loop (multi-step reasoning, capability orchestration)
├── LLM Integration Layer (abstract provider interface)
└── Capability System (read-only interface to Python)
         ↓ PyO3
Embedded Python Interpreter (persistent session, state, execution)
```

### Core Capabilities (MVP)
1. `list_globals()` - discover variables in scope
2. `inspect(expr)` - structured inspection for objects/functions/containers
3. `eval_expr(expr)` - evaluate expressions (read-only, restricted)

### Two Modes
- **Python Mode**: Standard REPL execution
- **Assistant Mode**: Conversational interface for asking about runtime environment
- Switch via TAB key

## Key Product Principles
- **Runtime Awareness**: assistant based on actual live environment
- **Lazy Inspection**: only relevant information inspected when needed
- **Read-Only Access**: assistant does not modify program state
- **Familiar Workflow**: stays close to traditional REPL

## Technical Decisions

### Safety Strategy (Multi-layer)
1. Rust-side validation (AST inspection, allowlist)
2. Python-side sandboxing (restricted globals)
3. Execution timeouts
4. Read-only enforcement at capability level

### Object Representation
- **repr-first**: assistant sees objects primarily via `repr()` and type information
- Structured adapters deferred to future iterations

### Agent Interaction Model
- **Bounded multi-step reasoning loop**: LLM can chain small number of capability calls
- Allows complex questions without full agent orchestration

## Target Users
- Python learners
- Data scientists
- Developers working interactively
- Anyone who frequently uses Python REPL

## Use Cases
- Learning Python: understand objects/functions interactively, explore built-ins
- Debugging: inspect variable state, understand exceptions, check assumptions
- Data exploration: inspect data structures, summarize collections
- Interactive experimentation: prototype ideas, verify behavior, explore APIs

## Implementation Phases
1. **Rust Host Foundation**: Basic Rust app with CLI and Python embedding
2. **Python Runtime Interface**: Bridge between Rust and embedded Python
3. **Capability System**: Read-only interface implementation
4. **LLM Integration**: LLM backend for conversational assistant
5. **Agent Interaction Loop**: Multi-step reasoning with capability orchestration
6. **Refinement & Polish**: UX, error handling, robustness

## Key Dependencies
- `pyo3` - Python interpreter embedding
- `rustyline` - CLI and terminal handling
- `anyhow` - Error handling
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `serde`, `serde_json` - Serialization
- Provider SDK (e.g., `async-openai`)

## Operative rules

<important>These rules must **always be followed**

- We use conventional commits format
- Keep the commit messages short and focused
- Always run tests, formatter, and linter before declaring a task "done"
- When implementing a plan, split into multiple logical git commits
- For task management, to break down complex work, track progress across sessions, and coordinate multi-step implementations, use `/dex`
