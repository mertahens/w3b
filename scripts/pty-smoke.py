#!/usr/bin/env python3
"""End-to-end smoke test for the interactive TUI, driven through a pty.

This is the only check that exercises raw mode, the alternate screen, the event
loop and a quit through real terminal I/O. It needs no network: a throwaway
HTTP server on 127.0.0.1 serves one page.

    python3 scripts/pty-smoke.py [path-to-w3b]
"""
from __future__ import annotations

import fcntl
import os
import signal
import pty
import re
import struct
import subprocess
import sys
import termios
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

PAGE = b"""<!doctype html>
<html><head><title>Pty smoke</title>
<style>.secret { display: none } .term { font-family: monospace }</style>
</head>
<body>
<h1>Pty heading</h1>
<p>some paragraph text that should show up on screen.</p>
<p class="secret">SECRET-NEVER-SHOWN</p>
<p class="secret" style="display: BLOCK !important">IMPORTANT-SHOWN</p>
<table><tr><th>Tool</th><th>Stars</th></tr><tr><td>w3b</td><td>12</td></tr></table>
<p>with <code>code-text</code> and <span class="term">term-text</span> inline.</p>
<p><img src="/img/logo.png" alt="logo"></p>
<a href="/second">the second page</a>
</body></html>
"""
SECOND = (
    b"<html><head><title>Second</title></head><body>"
    b"<p>second page body</p>"
    b'<p>search: <input name="q" value="typed-in"></p>'
    b"</body></html>"
)

LAST = {"path": "/"}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        LAST["path"] = self.path
        body = SECOND if self.path.startswith("/second") else PAGE
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):  # silence
        pass


def serve() -> tuple[HTTPServer, int]:
    server = HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


ANSI = re.compile(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b[()][A-Za-z0-9]|\x1b[>=<!]|\x1b[=>]")


def main() -> int:
    binary = sys.argv[1] if len(sys.argv) > 1 else "target/release/w3b"
    if not os.path.isabs(binary):
        binary = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), binary)
    server, port = serve()
    url = f"http://127.0.0.1:{port}/"

    pid, fd = pty.fork()
    if pid == 0:  # child
        os.environ["TERM"] = "xterm-256color"
        try:
            os.execv(binary, [binary, url])
        except OSError as exc:
            print(f"cannot exec {binary}: {exc}", file=sys.stderr)
            os._exit(127)

    # Give the child a real terminal size.
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))

    # Move, move, hover the link target, Tab onto it, follow it.
    keys = [b"j", b"j", b"l", b"\t", b"\r"]
    # On page two: Tab onto the form field, Enter to type, then a character.
    typing = [b"\t", b"\r", b"X"]
    transcript = bytearray()
    deadline = time.time() + 15.0
    stage = 0
    quit_sent = False

    while time.time() < deadline:
        r, _, _ = select_read(fd, 0.3)
        if r:
            try:
                chunk = os.read(fd, 4096)
            except OSError:
                break
            if not chunk:
                break
            transcript.extend(chunk)

        # ratatui writes cell runs separated by cursor moves, so match on the
        # text that would actually be visible in the terminal.
        plain = ANSI.sub(b"", transcript)
        if stage == 0 and b"Pty heading" in plain:
            stage = 1
            for key in keys:
                os.write(fd, key)
                time.sleep(0.15)
        elif stage == 1 and b"second page body" in plain:
            stage = 2
            for key in typing:
                os.write(fd, key)
                time.sleep(0.15)
        # A diff write repaints only what changed, so the typed letter shows up
        # as the tail of the field: the letter, then the closing bracket.
        elif stage == 2 and b"X ]" in plain:
            stage = 3
            # Escape leaves the field (q there would only type), then quit.
            os.write(fd, b"\x1b")
            time.sleep(0.3)
            quit_sent = True
            os.write(fd, b"q")
        if quit_sent:
            done, _ = os.waitpid(pid, os.WNOHANG)
            if done:
                break

    # Make sure the child is gone and we have read its final frame, so the
    # leave-alternate-screen sequence is part of the transcript.
    try:
        # ESC on its own: ESC followed straight by a letter reads as Alt.
        os.write(fd, b"\x1b")
        time.sleep(0.3)
        os.write(fd, b"q")
    except OSError:
        pass
    status = 0
    drained_until = time.time() + 3.0
    while time.time() < drained_until:
        try:
            chunk = os.read(fd, 4096)
        except BlockingIOError:
            time.sleep(0.05)
            continue
        except OSError:
            break
        if not chunk:
            break
        transcript.extend(chunk)
        done, waited = os.waitpid(pid, os.WNOHANG)
        if done:
            status = waited
            break
    # Never wait forever: a w3b that ignores q is a bug to report, not a harness
    # that hangs.
    stuck = False
    exit_by = time.time() + 3
    while time.time() < exit_by:
        try:
            done, waited = os.waitpid(pid, os.WNOHANG)
        except ChildProcessError:
            break
        if done:
            status = waited
            break
        time.sleep(0.05)
    else:
        stuck = True
        os.kill(pid, signal.SIGKILL)
        try:
            _, status = os.waitpid(pid, 0)
        except ChildProcessError:
            pass
    os.close(fd)
    server.shutdown()
    server.server_close()

    raw = bytes(transcript)
    plain = ANSI.sub(b"", raw)
    problems = []
    if b"\x1b[?1049h" not in raw and b"\x1b[?1047h" not in raw:
        problems.append("no alternate screen enter sequence")
    if b"\x1b[?1049l" not in raw and b"\x1b[?1047l" not in raw:
        problems.append("no alternate screen leave sequence (terminal left dirty?)")
    for needle in (b"Pty heading", b"the second page", b"200", b"second page body"):
        if needle not in raw and needle not in plain:
            problems.append(f"never rendered {needle!r}")
    for needle in (b"[img: logo]", b"[ typed-in ]", b"[ typing in q", b"X ]"):
        if needle not in plain:
            problems.append(f"never rendered {needle!r}")
    # The monospace background is the dark grey from the shipped config, and it
    # covers the text rather than the whole line.
    if b"48;2;20;20;20" not in raw:
        problems.append("no dark (rgb 20,20,20) monospace background was painted")
    if b"38;2;245;245;245" not in raw:
        problems.append("monospace text was not painted in the bright ink")
    # w3b must not flash an error page on its way to the first frame.
    if b"ERROR" in plain:
        problems.append("an ERROR page was painted at some point")
    for needle in (b"Tool", b"Stars", b"code-text", b"term-text"):
        if needle not in plain:
            problems.append(f"page content {needle!r} never reached the screen")
    # Hovering must highlight the whole link, not the word under the cursor.
    # Cells that share a style are written as one run, so one run of the link's
    # own text in the hover colour (bg 7) proves it.  The cursor sits on the
    # link's first letter, so the run starts at its second.
    if not re.search(rb"48;5;7m[ -~]*second page", raw):
        problems.append("the hover highlight did not cover the whole link text")
    if b"SECRET-NEVER-SHOWN" in plain:
        problems.append("a display: none element was rendered")
    # An inline `display: block !important` outranks the sheet's display: none.
    if b"IMPORTANT-SHOWN" not in plain:
        problems.append("an !important inline display was overridden")
    # A table must come out as a box-drawing grid. The grid characters are
    # multi-byte, and a diff write can split one, so drop replacement
    # characters before looking.
    grid = plain.decode("utf-8", "replace").replace("\ufffd", "")
    if "\u250c" not in grid and "\u2502" not in grid:
        problems.append("no box-drawing table border in the output")
    if stage < 3:
        problems.append(f"the field was never typed into (stages reached: {stage})")
    if LAST["path"] == "/":
        problems.append("Enter on a link never issued a request")
    if stuck:
        problems.append("w3b was still running 3s after q")
    if not os.waitstatus_to_exitcode(status) == 0:
        problems.append(f"exit code {os.waitstatus_to_exitcode(status)}")



    print(f"transcript: {len(raw)} bytes, stages reached: {stage}")
    if problems:
        print("PTY SMOKE FAILED:")
        for problem in problems:
            print(f"  - {problem}")
        print("--- rendered text (ANSI stripped) ---")
        print(plain.decode("utf-8", "replace")[-2000:])
        return 1
    print("PTY SMOKE OK: alt screen entered and restored, page rendered, link followed, quit clean")
    return 0


def select_read(fd: int, timeout: float):
    import select

    return select.select([fd], [], [], timeout)


if __name__ == "__main__":
    sys.exit(main())
