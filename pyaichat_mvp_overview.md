# PyAIChat — High-Level MVP Overview

## Summary

PyAIChat is a minimal command-line Python environment that combines normal code execution with a conversational assistant capable of inspecting the current runtime state. Users can write and run Python code normally and switch into assistant mode to ask questions about variables, objects, functions, or errors.

The MVP focuses on **exploratory usage**: helping users understand their Python session interactively with minimal friction.

All future enhancements (structured object summaries, multi-language support, session history, advanced debugging, etc.) are deferred beyond this MVP.

---

## CLI Experience

- **Minimal REPL**: single prompt, simple mode toggle between Python and assistant.
- **Reasoning**: keeps MVP lightweight and fast to iterate; advanced features like syntax highlighting, multiline editing, and history per mode can be added later.

---

## Python Runtime

- **Embedded interpreter**: a single persistent Python session runs within the host.
- **Reasoning**: simplest way to maintain runtime state while enabling inspection; easier iteration before moving to an RPC or subprocess model.

---

## Object Representation

- **repr-first**: assistant sees objects primarily via `repr()` and type information.
- **Reasoning**: sufficient for most exploratory questions; structured adapters and summaries deferred to future iterations.

---

## Capability System (Inspection Layer)

Defines the **read-only interface** between the assistant and Python runtime.

**MVP capabilities**:

- `list_globals()`: discover variables in scope
- `get_type(expr)`: retrieve the type of an expression
- `get_repr(expr)`: get a textual representation
- `get_dir(expr)`: list attributes/members
- `get_doc(expr)`: fetch documentation
- `eval_expr(expr)`: evaluate expressions (read-only, restricted)
- `get_last_exception()`: inspect last runtime error

**Reasoning**: minimal, safe, and sufficient for interactive exploration; other capabilities deferred.

---

## LLM Integration

- **Abstracted model interface**: the assistant interacts with the runtime via the capability system; the underlying LLM provider (local or remote) is configurable.
- **Reasoning**: keeps MVP flexible without committing to a specific backend.

---

## Agent Interaction Model

- **Bounded multi-step reasoning loop**: the assistant can chain a small number of capability calls to answer questions.
- **Reasoning**: allows the LLM to answer slightly more complex questions without full agent orchestration; simpler to implement and debug.

---

## Deferred Features (Future Enhancements)

- Advanced REPL features (history, syntax highlighting, multiline editing)
- Structured object adapters (e.g., pandas, numpy)
- Full agentic reasoning loop
- Multi-language support
- Session snapshots, time-travel, or persistent history
- Remote runtime or DAP integration
- Security enforcement policies beyond logical read-only

---

## One-Line Description

**PyAIChat:** A minimal Python REPL with a conversational assistant that inspects and explains live runtime state for interactive exploration.
