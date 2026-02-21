# Command Reference

## Commands

- `/help`
Shows available commands.

- `/mode [py|ai]`
Shows current mode or switches mode.
Examples: `/mode`, `/mode py`, `/mode ai`

- `/clear`
Clears timeline output.

- `/history [n]`
Shows input history, optionally last `n` items.
Examples: `/history`, `/history 20`

- `/trace`
Prints the current session trace file path.

- `/inspect <expr>`
Runs structured inspect on a Python expression.
Example: `/inspect my_var[0]`

- `/last_error`
Prints the last Python exception traceback.

- `/include <file.py>`
Executes a Python file in the current session.

- `/run <file>`
Alias for include with no extension restriction.

- `/show_source <name>`
Shows source for a safe identifier path (function/class/module-style names).
Example: `/show_source my_module.my_function`

- `/steps [on|off]`
Toggles assistant tool-step visibility.
Examples: `/steps`, `/steps on`, `/steps off`

## Notes

- Commands work in both modes.
- Assistant responses require `GEMINI_API_KEY`.
