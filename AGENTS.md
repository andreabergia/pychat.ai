# PyChat.ai Memory File

## Project Overview
PyChat.ai is a minimal command-line Python environment combining normal code execution with a conversational assistant that inspects live runtime state. Users can write/run Python code and switch to assistant mode to ask questions about variables, objects, functions, or errors.

**One-liner**: A minimal Python REPL with a conversational assistant that inspects and explains live runtime state for interactive exploration.

It has not been released yet, so we do not have to worry about backward compatibility or preserving any user's settings.

## Core Architecture

### Tech Stack
- **Rust host application** written in Rust using PyO3 to embed Python interpreter
- **LLM Provider**: Google Gemini
- **REPL Library**: `rustyline`
- **Python Embedding**: PyO3 with embedded interpreter

### Architecture Layers
```
Rust Host Application
├── CLI & REPL Loop (mode switching, input handling)
├── Agent Loop (multi-step reasoning, capability orchestration)
├── LLM Integration Layer (abstract provider interface)
└── Capability System (runtime inspection + expression evaluation tools)
         ↓ PyO3
Embedded Python Interpreter (persistent session, state, execution)
```

### Core Capabilities (MVP)
1. `list_globals()` - discover variables in scope
2. `inspect(expr)` - structured inspection for objects/functions/containers (evaluates Python expressions)
3. `eval_expr(expr)` - evaluate Python expressions and capture value/stdout/stderr

Note: this prototype is not sandboxed. Assistant tool calls can evaluate arbitrary Python expressions and may cause side effects (including file/network access via Python code).

### Two Modes
- **Python Mode**: Standard REPL execution
- **Assistant Mode**: Conversational interface for asking about runtime environment
- Switch via TAB key

## Operative rules

<important>These rules must **always be followed**

- We use conventional commits format
- Keep the commit messages short and focused
- Always run tests, formatter, and linter before declaring a task "done"
- When implementing a plan, split into multiple logical git commits
