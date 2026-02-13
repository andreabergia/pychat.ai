import contextlib
import io
import traceback

_PYAICHAT_LAST_EXCEPTION = None


def _pyaichat_capture_exception(exc):
    global _PYAICHAT_LAST_EXCEPTION
    _PYAICHAT_LAST_EXCEPTION = {
        "exc_type": type(exc).__name__,
        "message": str(exc),
        "traceback": traceback.format_exc(),
    }
    return _PYAICHAT_LAST_EXCEPTION


def _pyaichat_exec_code(code):
    out = io.StringIO()
    err = io.StringIO()
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            exec(code, globals(), globals())
        return {"ok": True, "stdout": out.getvalue(), "stderr": err.getvalue()}
    except BaseException as exc:
        return {
            "ok": False,
            "stdout": out.getvalue(),
            "stderr": err.getvalue(),
            "exception": _pyaichat_capture_exception(exc),
        }


def _pyaichat_eval_expr(expr):
    out = io.StringIO()
    err = io.StringIO()
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            value = eval(expr, globals(), globals())
        return {
            "ok": True,
            "value_repr": repr(value),
            "stdout": out.getvalue(),
            "stderr": err.getvalue(),
        }
    except BaseException as exc:
        return {
            "ok": False,
            "stdout": out.getvalue(),
            "stderr": err.getvalue(),
            "exception": _pyaichat_capture_exception(exc),
        }


def _pyaichat_get_type(expr):
    try:
        value = eval(expr, globals(), globals())
        value_type = type(value)
        name = value_type.__name__
        module = getattr(value_type, "__module__", "")
        qualified_name = getattr(value_type, "__qualname__", name)
        qualified = qualified_name if not module else f"{module}.{qualified_name}"
        return {
            "ok": True,
            "name": name,
            "module": module,
            "qualified": qualified,
        }
    except BaseException as exc:
        return {"ok": False, "exception": _pyaichat_capture_exception(exc)}


def _pyaichat_get_repr(expr):
    try:
        value = eval(expr, globals(), globals())
        return {"ok": True, "repr": repr(value)}
    except BaseException as exc:
        return {"ok": False, "exception": _pyaichat_capture_exception(exc)}


def _pyaichat_get_dir(expr):
    try:
        value = eval(expr, globals(), globals())
        return {"ok": True, "members": sorted(dir(value))}
    except BaseException as exc:
        return {"ok": False, "exception": _pyaichat_capture_exception(exc)}


def _pyaichat_get_doc(expr):
    try:
        value = eval(expr, globals(), globals())
        return {"ok": True, "doc": getattr(value, "__doc__", None)}
    except BaseException as exc:
        return {"ok": False, "exception": _pyaichat_capture_exception(exc)}


def _pyaichat_run_user_input(line):
    try:
        compile(line, "<stdin>", "eval")
    except SyntaxError:
        result = _pyaichat_exec_code(line)
        result["kind"] = "executed"
        return result
    result = _pyaichat_eval_expr(line)
    result["kind"] = "evaluated"
    return result


def _pyaichat_list_globals():
    entries = []
    for name, value in globals().items():
        if name == "__builtins__":
            continue
        if name.startswith("_pyaichat_"):
            continue
        if name.startswith("__") and name.endswith("__"):
            continue
        entries.append((name, type(value).__name__))
    entries.sort(key=lambda item: item[0])
    return entries


def _pyaichat_get_last_exception():
    return _PYAICHAT_LAST_EXCEPTION
