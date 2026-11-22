"""Regression tests for bounded event-export RPC calls and collection size (#350)."""

import http.server
import importlib.util
import json
import os
import socket
import threading
import time
import unittest
from urllib.parse import urlparse, parse_qs


MODULE_PATH = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "fetch_testnet_snapshot.py",
)


def _load_module():
    spec = importlib.util.spec_from_file_location("fetch_testnet_snapshot", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class _FixtureHandler(http.server.BaseHTTPRequestHandler):
    """Configurable JSON-RPC fixture driven by the server's ``mode`` attribute."""

    def log_message(self, *args):  # silence test output
        pass

    def _send_json(self, payload, status=200):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length) if length else b"{}"
        try:
            request = json.loads(raw)
        except ValueError:
            request = {}
        mode = self.server.mode

        if mode == "stall":
            # Never respond; hold the connection until the client gives up.
            time.sleep(self.server.stall_seconds)
            return

        if mode == "error_after_partial":
            cursor = request.get("params", {}).get("cursor")
            if cursor:
                self._send_json({"error": {"message": "boom"}}, status=500)
                return
            self._send_json({"result": {"events": [{"id": 1}], "cursor": "next"}})
            return

        if mode == "repeat_cursor":
            self._send_json({"result": {"events": [{"id": 1}], "cursor": "same"}})
            return

        if mode == "many_pages":
            cursor = request.get("params", {}).get("cursor")
            index = int(cursor) if cursor else 0
            events = [{"id": index * 10 + i} for i in range(10)]
            self._send_json({"result": {"events": events, "cursor": str(index + 1)}})
            return

        self._send_json({"result": {"events": [], "cursor": None}})


class _FixtureServer(http.server.ThreadingHTTPServer):
    daemon_threads = True


def _start_server(mode, stall_seconds=5.0):
    server = _FixtureServer(("127.0.0.1", 0), _FixtureHandler)
    server.mode = mode
    server.stall_seconds = stall_seconds
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


class FetchEventsBoundedTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.module = _load_module()

    def setUp(self):
        self._servers = []

    def tearDown(self):
        for server in self._servers:
            server.shutdown()
            server.server_close()

    def _url(self, mode, stall_seconds=5.0):
        server = _start_server(mode, stall_seconds=stall_seconds)
        self._servers.append(server)
        host, port = server.server_address
        return "http://%s:%d" % (host, port)

    def test_stalled_request_terminates_with_actionable_error(self):
        url = self._url("stall", stall_seconds=10.0)
        started = time.monotonic()
        with self.assertRaises(Exception) as ctx:
            self.module.fetch_events(url, timeout=0.5)
        elapsed = time.monotonic() - started
        self.assertLess(elapsed, 5.0, "stalled request was not bounded by timeout")
        self.assertTrue(str(ctx.exception), "error message must be actionable")

    def test_many_pages_respects_page_ceiling(self):
        url = self._url("many_pages")
        result = self.module.fetch_events(url, max_pages=3, timeout=2.0)
        events, truncated = _unpack(result)
        self.assertTrue(truncated, "page ceiling must report truncation")
        self.assertLessEqual(len(events), 30)

    def test_many_pages_respects_record_ceiling(self):
        url = self._url("many_pages")
        result = self.module.fetch_events(url, max_records=15, timeout=2.0)
        events, truncated = _unpack(result)
        self.assertTrue(truncated, "record ceiling must report truncation")
        self.assertLessEqual(len(events), 15)

    def test_repeated_cursor_is_detected(self):
        url = self._url("repeat_cursor")
        result = self.module.fetch_events(url, max_pages=10, timeout=2.0)
        events, truncated = _unpack(result)
        self.assertTrue(truncated, "repeated cursor must report truncation")

    def test_error_after_partial_progress_is_not_complete(self):
        url = self._url("error_after_partial")
        with self.assertRaises(Exception):
            self.module.fetch_events(url, timeout=2.0)

    def test_complete_export_is_not_truncated(self):
        url = self._url("empty")
        result = self.module.fetch_events(url, timeout=2.0)
        events, truncated = _unpack(result)
        self.assertEqual(events, [])
        self.assertFalse(truncated, "complete export must not be reported as truncated")


def _unpack(result):
    """Accept either ``(events, truncated)`` or an object with those attributes."""
    if isinstance(result, tuple):
        return result[0], result[1]
    return result.events, result.truncated


if __name__ == "__main__":
    unittest.main()
