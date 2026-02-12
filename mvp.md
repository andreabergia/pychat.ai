# PyAIChat — Unified Vision Document

## Summary

PyAIChat is a minimal command-line Python environment that combines normal code execution with a conversational assistant capable of inspecting the current runtime state. Users can write and run Python code normally and switch into assistant mode to ask questions about variables, objects, functions, or errors.

The MVP focuses on **exploratory usage**: helping users understand their Python session interactively with minimal friction.

The goal is simple:

**Make it easier to understand what's happening inside a running Python session without manually inspecting everything.**

---

## One-Line Description

PyAIChat: A minimal Python REPL with a conversational assistant that inspects and explains live runtime state for interactive exploration.

---

## What Problem It Solves

Working in a REPL often involves repeatedly checking:

- Types of values
- Contents of objects
- Function signatures
- Error causes
- Intermediate results

This typically requires printing values, exploring attributes manually, or consulting documentation.

PyAIChat reduces this friction by allowing users to ask questions about the current state in plain language and receive grounded explanations.

---

## Core Interaction Model

The environment has two complementary modes:

### Python Mode

Standard REPL execution.

Users write and run code normally.

### Assistant Mode

A conversational interface for asking questions about the current runtime environment.

Users can switch between modes at any time with <TAB> without losing context.

**Typical workflow:**

run code → ask questions → continue coding

---

## Key Product Principles

### Runtime Awareness

The assistant bases its responses on the actual live environment.

### Lazy Inspection

Only relevant information is inspected when needed.

### Read-Only Access

The assistant does not modify program state.

### Familiar Workflow

The experience stays close to a traditional REPL.

---

## Technical Architecture

### Rust Host Application

**Rust-based host**: the application is written in Rust and embeds the Python interpreter via PyO3.

**Reasoning**: Creates a clear security and architectural boundary between the LLM and the Python runtime. Rust provides memory safety, handles the capability system and orchestration, and enforces read-only guarantees before delegating to Python.

---

### CLI Experience

**Minimal REPL**: single prompt, simple mode toggle between Python and assistant.

**Reasoning**: Keeps MVP lightweight and fast to iterate; advanced features like syntax highlighting, multiline editing, and history per mode can be added later.

---

### Python Runtime

**Embedded interpreter**: a single persistent Python session runs within the host.

**Reasoning**: Simplest way to maintain runtime state while enabling inspection; easier iteration before moving to an RPC or subprocess model.

---

### Object Representation

**repr-first**: assistant sees objects primarily via `repr()` and type information.

**Reasoning**: Sufficient for most exploratory questions; structured adapters and summaries deferred to future iterations.

---

### Capability System (Inspection Layer)

Defines the **read-only interface** between the assistant and Python runtime.

**MVP capabilities**:

- `list_globals()`: discover variables in scope
- `get_type(expr)`: retrieve the type of an expression
- `get_repr(expr)`: get a textual representation
- `get_dir(expr)`: list attributes/members
- `get_doc(expr)`: fetch documentation
- `eval_expr(expr)`: evaluate expressions (read-only, restricted)
- `get_last_exception()`: inspect last runtime error

**Reasoning**: Minimal, safe, and sufficient for interactive exploration; other capabilities deferred.

---

### LLM Integration

**Abstracted model interface**: the assistant interacts with the runtime via the capability system; the underlying LLM provider (local or remote) is configurable.

**Reasoning**: Keeps MVP flexible without committing to a specific backend.

---

### Agent Interaction Model

**Bounded multi-step reasoning loop**: the assistant can chain a small number of capability calls to answer questions.

**Reasoning**: Allows the LLM to answer slightly more complex questions without full agent orchestration; simpler to implement and debug.

---

## Example Interactions

```
>>> x = [1, 2, 3]

Assistant> what is x?
list with 3 elements

Assistant> what type is len?
builtin function

Assistant> why did my last command fail?
Explanation of the exception and likely cause
```

---

## Use Cases

### Learning Python

- Understand objects and functions interactively
- Explore built-ins and libraries
- Get explanations of errors
- Reduce need to search documentation

### Debugging

- Inspect variable state quickly
- Understand exceptions
- Check assumptions without writing extra code
- Explore unfamiliar values

### Data Exploration

- Inspect structure of data objects
- Summarize collections or tables
- Understand intermediate results

### Interactive Experimentation

- Prototype ideas quickly
- Verify behavior
- Explore APIs while using them

---

## Target Users

- Python learners
- Data scientists
- Developers working interactively
- Anyone who frequently uses the Python REPL

---

## High-Value Features

### Conversational Inspection

Ask questions about any value or object in scope.

### Exception Explanation

When an error occurs, the assistant can help interpret it and describe what likely caused it.

### Object Summaries

Large or complex objects are described in concise, readable form.

### Environment Awareness

The assistant understands what variables and objects currently exist.

### Continuous Workflow

No need to switch tools or add diagnostic code.

---

## Future Enhancements

- Advanced REPL features (history, syntax highlighting, multiline editing)
- Structured object adapters (e.g., pandas, numpy)
- Full agentic reasoning loop
- Multi-language support
- Session snapshots, time-travel, or persistent history
- Remote runtime or DAP integration
- Security enforcement policies beyond logical read-only
- Richer summaries for common data structures
- Better support for debugging workflows
- Session history awareness
- Integration with external Python processes
