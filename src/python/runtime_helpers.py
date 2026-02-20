import codeop
import collections.abc
import contextlib
import inspect as pyinspect
import io
import json
import linecache
import signal
import threading
import time
import traceback

_PYAICHAT_LAST_EXCEPTION = None
_PYAICHAT_SOURCE_COUNTER = 0
_REPR_MAX_LEN = 4096
_DOC_MAX_LEN = 4096
_SAMPLE_MAX_ITEMS = 16
_MEMBER_MAX_PER_GROUP = 24
_SOURCE_PREVIEW_MAX_LEN = 1200
_INSPECT_TIMEOUT_SECONDS = 1.0


def _pyaichat_capture_exception(exc):
    global _PYAICHAT_LAST_EXCEPTION
    _PYAICHAT_LAST_EXCEPTION = {
        "exc_type": type(exc).__name__,
        "message": str(exc),
        "traceback": traceback.format_exc(),
    }
    return _PYAICHAT_LAST_EXCEPTION


def _pyaichat_register_source(source, mode):
    global _PYAICHAT_SOURCE_COUNTER
    _PYAICHAT_SOURCE_COUNTER += 1
    filename = f"<pyaichat-{mode}-{_PYAICHAT_SOURCE_COUNTER}>"
    text = source if source.endswith("\n") else f"{source}\n"
    lines = text.splitlines(keepends=True)
    linecache.cache[filename] = (len(text), None, lines, filename)
    return filename


def _truncate_text(value, max_chars):
    original_len = len(value)
    if original_len <= max_chars:
        return value, False, original_len
    return value[:max_chars], True, original_len


def _safe_repr(value):
    try:
        return repr(value), None
    except BaseException as exc:
        return f"<repr failed: {type(exc).__name__}: {exc}>", f"{type(exc).__name__}: {exc}"


def _repr_payload(value):
    text, repr_error = _safe_repr(value)
    text, truncated, original_len = _truncate_text(text, _REPR_MAX_LEN)
    payload = {
        "text": text,
        "truncated": truncated,
        "original_len": original_len,
    }
    if repr_error is not None:
        payload["repr_error"] = repr_error
    return payload


def _doc_payload(value):
    try:
        doc = getattr(value, "__doc__", None)
    except BaseException as exc:
        return {
            "text": None,
            "truncated": False,
            "original_len": 0,
            "doc_error": f"{type(exc).__name__}: {exc}",
        }

    if doc is None:
        return {"text": None, "truncated": False, "original_len": 0}

    if not isinstance(doc, str):
        try:
            doc = str(doc)
        except BaseException as exc:
            return {
                "text": None,
                "truncated": False,
                "original_len": 0,
                "doc_error": f"{type(exc).__name__}: {exc}",
            }

    text, truncated, original_len = _truncate_text(doc, _DOC_MAX_LEN)
    return {
        "text": text,
        "truncated": truncated,
        "original_len": original_len,
    }


def _kind_of(value):
    if value is None:
        return "none"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, (int, float, complex)):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, (bytes, bytearray, memoryview)):
        return "bytes"
    if isinstance(value, dict):
        return "mapping"
    if isinstance(value, (list, tuple, range)):
        return "sequence"
    if isinstance(value, (set, frozenset)):
        return "set"
    if pyinspect.isasyncgen(value):
        return "async_generator"
    if pyinspect.iscoroutine(value):
        return "coroutine"
    if pyinspect.isgenerator(value):
        return "generator"
    if isinstance(value, collections.abc.Iterator):
        return "iterator"
    if pyinspect.ismodule(value):
        return "module"
    if pyinspect.isclass(value):
        return "class"
    if isinstance(value, BaseException):
        return "exception"
    if callable(value):
        return "callable"
    return "object"


def _type_payload(value):
    value_type = type(value)
    name = value_type.__name__
    module = getattr(value_type, "__module__", "")
    qualified_name = getattr(value_type, "__qualname__", name)
    qualified = qualified_name if not module else f"{module}.{qualified_name}"
    return {
        "name": name,
        "module": module,
        "qualified": qualified,
    }


def _size_payload(value):
    payload = {}

    try:
        payload["len"] = len(value)
    except BaseException:
        pass

    shape = getattr(value, "shape", None)
    if shape is not None:
        try:
            if isinstance(shape, tuple):
                payload["shape"] = list(shape)
            elif isinstance(shape, list):
                payload["shape"] = shape
            else:
                payload["shape"] = [shape]
        except BaseException:
            pass

    return payload if payload else None


def _sample_items_from_mapping(value):
    items = []
    for key, item in value.items():
        if len(items) >= _SAMPLE_MAX_ITEMS:
            break
        key_repr, _ = _safe_repr(key)
        item_repr, _ = _safe_repr(item)
        items.append(f"{key_repr}: {item_repr}")
    return items


def _sample_items_from_iterable(value):
    items = []
    for item in value:
        if len(items) >= _SAMPLE_MAX_ITEMS:
            break
        item_repr, _ = _safe_repr(item)
        items.append(item_repr)
    return items


def _sample_payload(value, kind):
    if kind in {"generator", "iterator", "coroutine", "async_generator"}:
        return None

    try:
        if isinstance(value, dict):
            items = _sample_items_from_mapping(value)
        elif isinstance(value, (list, tuple, range, set, frozenset)):
            items = _sample_items_from_iterable(value)
        else:
            return None
    except BaseException as exc:
        return {
            "items": [],
            "shown": 0,
            "total": 0,
            "truncated": False,
            "sample_error": f"{type(exc).__name__}: {exc}",
        }

    total = len(items)
    try:
        total = len(value)
    except BaseException:
        pass

    return {
        "items": items,
        "shown": len(items),
        "total": total,
        "truncated": total > len(items),
    }


def _members_payload(value):
    try:
        names = sorted(dir(value))
    except BaseException as exc:
        return {
            "data": [],
            "callables": [],
            "dunder_count": 0,
            "shown_per_group": _MEMBER_MAX_PER_GROUP,
            "truncated": False,
            "dir_error": f"{type(exc).__name__}: {exc}",
        }

    dunder_count = 0
    data = []
    callables = []

    for name in names:
        if name.startswith("__") and name.endswith("__"):
            dunder_count += 1
            continue

        try:
            attr = pyinspect.getattr_static(value, name)
        except BaseException:
            if len(data) < _MEMBER_MAX_PER_GROUP:
                data.append(name)
            continue

        if callable(attr):
            if len(callables) < _MEMBER_MAX_PER_GROUP:
                callables.append(name)
        else:
            if len(data) < _MEMBER_MAX_PER_GROUP:
                data.append(name)

    non_dunder_total = len([n for n in names if not (n.startswith("__") and n.endswith("__"))])
    truncated = non_dunder_total > (len(data) + len(callables))

    return {
        "data": data,
        "callables": callables,
        "dunder_count": dunder_count,
        "shown_per_group": _MEMBER_MAX_PER_GROUP,
        "truncated": truncated,
    }


def _safe_signature(value):
    try:
        return str(pyinspect.signature(value))
    except BaseException:
        return None


def _source_preview(value):
    try:
        source = pyinspect.getsource(value)
    except BaseException as exc:
        return {
            "text": None,
            "truncated": False,
            "source_error": f"{type(exc).__name__}: {exc}",
        }

    text, truncated, _ = _truncate_text(source, _SOURCE_PREVIEW_MAX_LEN)
    return {
        "text": text,
        "truncated": truncated,
    }


def _callable_payload(value):
    module = getattr(value, "__module__", None)
    signature = _safe_signature(value)
    doc = _doc_payload(value)
    source = _source_preview(value)
    return {
        "module": module,
        "signature": signature,
        "doc": doc["text"],
        "source_preview": source["text"],
        "source_truncated": source["truncated"],
        **({"source_error": source["source_error"]} if "source_error" in source else {}),
    }


def _exception_payload(value):
    if isinstance(value, BaseException):
        return {
            "exc_type": type(value).__name__,
            "message": str(value),
        }
    return None


def _limits_payload():
    return {
        "repr_max_chars": _REPR_MAX_LEN,
        "doc_max_chars": _DOC_MAX_LEN,
        "sample_max_items": _SAMPLE_MAX_ITEMS,
        "member_max_per_group": _MEMBER_MAX_PER_GROUP,
        "source_preview_max_chars": _SOURCE_PREVIEW_MAX_LEN,
    }


def _with_timeout(fn):
    if not hasattr(signal, "SIGALRM"):
        return fn()
    if not hasattr(signal, "ITIMER_REAL"):
        return fn()
    if not hasattr(signal, "getitimer"):
        return fn()
    if not hasattr(signal, "setitimer"):
        return fn()
    if threading.current_thread() is not threading.main_thread():
        return fn()

    previous_handler = signal.getsignal(signal.SIGALRM)
    previous_timer = signal.getitimer(signal.ITIMER_REAL)
    timer_start = time.monotonic()

    def _timeout_handler(_signum, _frame):
        raise TimeoutError("inspect timed out")

    signal.signal(signal.SIGALRM, _timeout_handler)
    signal.setitimer(signal.ITIMER_REAL, _INSPECT_TIMEOUT_SECONDS)
    try:
        return fn()
    finally:
        elapsed = time.monotonic() - timer_start
        previous_delay, previous_interval = previous_timer
        restored_delay = max(0.0, previous_delay - elapsed)
        signal.setitimer(signal.ITIMER_REAL, restored_delay, previous_interval)
        signal.signal(signal.SIGALRM, previous_handler)


def _pyaichat_inspect(expr):
    def _run_inspect():
        value = eval(expr, globals(), globals())
        kind = _kind_of(value)
        payload = {
            "type": _type_payload(value),
            "kind": kind,
            "repr": _repr_payload(value),
            "doc": _doc_payload(value),
            "members": _members_payload(value),
            "limits": _limits_payload(),
        }

        size = _size_payload(value)
        if size is not None:
            payload["size"] = size

        sample = _sample_payload(value, kind)
        if sample is not None:
            payload["sample"] = sample

        if callable(value):
            payload["callable"] = _callable_payload(value)

        exception = _exception_payload(value)
        if exception is not None:
            payload["exception"] = exception

        return {"ok": True, "inspect_json": json.dumps(payload)}

    try:
        return _with_timeout(_run_inspect)
    except BaseException as exc:
        return {"ok": False, "exception": _pyaichat_capture_exception(exc)}


def _pyaichat_exec_code(code):
    out = io.StringIO()
    err = io.StringIO()
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            filename = _pyaichat_register_source(code, "exec")
            compiled = compile(code, filename, "exec")
            exec(compiled, globals(), globals())
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
            filename = _pyaichat_register_source(expr, "eval")
            compiled = compile(expr, filename, "eval")
            value = eval(compiled, globals(), globals())
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


def _pyaichat_check_input_complete(source):
    try:
        compiled = codeop.compile_command(source, "<stdin>", "exec")
        status = "incomplete" if compiled is None else "complete"
        return {"ok": True, "status": status}
    except (SyntaxError, OverflowError, ValueError, TypeError):
        return {"ok": True, "status": "invalid"}
    except BaseException as exc:
        return {"ok": False, "exception": _pyaichat_capture_exception(exc)}


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
