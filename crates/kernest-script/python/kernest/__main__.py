"""Bootstrap Kernest runs as ``python -u -m kernest``.

Reads one JSON request from stdin, runs the user's script/macro/plug-in with a
friendly ``font`` global, streams ``print`` and stderr back as output messages,
and writes one final result message. The real stdout is kept as the protocol
channel; user output is redirected so it can never corrupt the protocol.
"""

from __future__ import annotations

import json
import sys
import traceback

from . import Font, _take_draw, report, line, path, text

_proto = sys.stdout


def _emit(msg):
    _proto.write(json.dumps(msg, ensure_ascii=False))
    _proto.write("\n")
    _proto.flush()


class _Stream:
    """Line-buffered redirect of print/stderr into output messages."""

    def __init__(self, stream):
        self._stream = stream
        self._buf = ""

    def write(self, s):
        self._buf += s
        while "\n" in self._buf:
            line_text, self._buf = self._buf.split("\n", 1)
            _emit({"type": "output", "stream": self._stream, "text": line_text})
        return len(s)

    def flush(self):
        if self._buf:
            _emit({"type": "output", "stream": self._stream, "text": self._buf})
            self._buf = ""

    def isatty(self):
        return False


def _run(request):
    font = Font(request.get("font", {}), request.get("selection", {}))
    kind = request.get("kind", "macro")
    params = request.get("params") or {}
    ns = {
        "__name__": "__main__",
        "font": font,
        "selection": request.get("selection", {}),
        "params": params,
        "report": report,
        "line": line,
        "path": path,
        "text": text,
    }
    code = request.get("code")
    src_path = request.get("path")
    if code is None and src_path:
        with open(src_path, "r", encoding="utf-8") as fh:
            code = fh.read()
        ns["__file__"] = src_path
    if code is None:
        raise RuntimeError("No script code or path was given.")
    exec(compile(code, src_path or "<macro>", "exec"), ns)
    return font, kind


def main():
    sys.stdout = _Stream("stdout")
    sys.stderr = _Stream("stderr")
    try:
        raw = sys.stdin.read()
        request = json.loads(raw) if raw.strip() else {}
    except Exception as e:
        sys.stdout.flush()
        sys.stderr.flush()
        _emit({"type": "result", "ok": False, "error": f"Bad request: {e}"})
        return
    try:
        font, kind = _run(request)
        sys.stdout.flush()
        sys.stderr.flush()
        result = {"type": "result", "ok": True, "font": font.data}
        if kind == "reporter":
            result["draw"] = _take_draw()
        _emit(result)
    except SystemExit as e:
        sys.stdout.flush()
        sys.stderr.flush()
        if e.code in (None, 0):
            _emit({"type": "result", "ok": True, "font": request.get("font", {})})
        else:
            _emit({"type": "result", "ok": False, "error": f"Script exited with code {e.code}."})
    except BaseException:
        sys.stdout.flush()
        sys.stderr.flush()
        tb = traceback.format_exc()
        last = tb.strip().splitlines()[-1] if tb.strip() else "The script failed."
        _emit({"type": "result", "ok": False, "error": last, "traceback": tb})


if __name__ == "__main__":
    main()