# Architecture

## Goal

Provide a Python REPL where an assistant can answer questions using the actual live runtime state.

## High-Level Design

- Rust host application
- Embedded Python interpreter (PyO3)
- TUI loop with Python and assistant modes
- Agent loop that can call runtime capabilities
- LLM provider abstraction (Gemini implemented)

## Core Runtime Capabilities

- `list_globals()`
- `inspect(expr)`
- `eval_expr(expr)`

These are exposed as tool calls to the assistant loop.

## Current State (Implemented)

- TUI mode switching and timeline UI
- Python execution with output/error capture
- Structured inspect payloads (repr/doc/type/sample/member metadata)
- Bounded multi-step tool loop with timeouts/retries
- Gemini provider integration
- Config and theming system
- Session and HTTP trace logging

## Plan: `docs/architecture.md` Scope

Near-term plan for architecture work:

1. Harden capability safety
- tighten expression validation for tool calls
- make state-mutation guarantees explicit and testable

2. Expand provider layer
- keep provider trait stable
- add at least one additional provider implementation

3. Improve inspect adapters
- first-class summaries for common data-science objects
- reduce noisy payloads while preserving grounding

4. Strengthen reliability and observability
- richer error taxonomy and user-facing diagnostics
- trace redaction and configurable trace verbosity

5. Split runtime boundaries (optional next phase)
- evaluate subprocess/runtime-isolation model
- define protocol if moving off embedded interpreter

## Proposed Additional Docs

- `docs/security-model.md` for safety guarantees and known limits
- `docs/troubleshooting.md` for operational issues
- `docs/contributing.md` for local development and release workflow
