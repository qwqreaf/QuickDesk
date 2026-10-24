"""Wire-level regression tests against the real MCP bridge and a fake GUI.

Build with `cargo build --locked`, install `websockets>=14,<17`, then run
`python3 tests/test_protocol.py` from quickdesk-mcp. No desktop or API key is needed.
"""

import contextlib
import http.client
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import tempfile
import threading
import time
import unittest

from websockets.exceptions import ConnectionClosed
from websockets.sync.server import serve


BINARY = Path(os.environ.get(
    "QUICKDESK_MCP_BINARY",
    Path(__file__).resolve().parents[1] / "target" / "debug" /
    ("quickdesk-mcp.exe" if os.name == "nt" else "quickdesk-mcp"),
)).resolve()
MODERN = "2026-07-28"


def fake_gui(connection):
    with contextlib.suppress(ConnectionClosed):
        for message in connection:
            request = json.loads(message)
            results = {
                "getHostInfo": {"deviceId": "123456789", "accessCode": "fixture"},
                "listConnections": {"connections": []},
            }
            result = results.get(request["method"], {})
            connection.send(json.dumps({"id": request["id"], "result": result}))


@contextlib.contextmanager
def bridge(*args):
    with serve(fake_gui, "127.0.0.1", 0) as gui, tempfile.TemporaryDirectory() as data:
        thread = threading.Thread(target=gui.serve_forever, daemon=True)
        thread.start()
        env = os.environ.copy()
        # Exercise the stores' current-directory fallback in an isolated temp dir.
        for key in ("HOME", "USERPROFILE", "QUICKDESK_TOKEN", "QUICKDESK_READONLY_TOKEN",
                    "QUICKDESK_CORS_ORIGIN", "QUICKDESK_ALLOWED_HOSTS",
                    "QUICKDESK_ALLOWED_DEVICES"):
            env.pop(key, None)
        with tempfile.TemporaryFile() as errors:
            process = subprocess.Popen(
                [str(BINARY), "--ws-url", f"ws://127.0.0.1:{gui.socket.getsockname()[1]}",
                 *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=errors, cwd=data, env=env,
            )
            try:
                yield process
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
                process.stdin.close()
                process.stdout.close()
                gui.shutdown()
                thread.join(timeout=5)
                if process.returncode not in (0, -15, 1):
                    errors.seek(0)
                    print(errors.read().decode())


class HttpClient:
    def __init__(self, port, version):
        self.port = port
        self.version = version
        self.session = None
        self.request_id = 0

    def rpc(self, method, params=None, notification=False, host=None):
        self.request_id += 1
        params = dict(params or {})
        headers = {"Content-Type": "application/json",
                   "Accept": "application/json, text/event-stream",
                   "MCP-Protocol-Version": self.version}
        if self.session:
            headers["Mcp-Session-Id"] = self.session
        if host:
            headers["Host"] = host
        if self.version == MODERN:
            headers["Mcp-Method"] = method
            if "name" in params or "uri" in params:
                headers["Mcp-Name"] = params.get("name", params.get("uri"))
            params["_meta"] = {
                "io.modelcontextprotocol/protocolVersion": MODERN,
                "io.modelcontextprotocol/clientInfo": {"name": "regression", "version": "1"},
                "io.modelcontextprotocol/clientCapabilities": {},
            }
        body = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notification:
            body["id"] = self.request_id
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        try:
            connection.request("POST", "/mcp", json.dumps(body), headers)
            response = connection.getresponse()
            self.session = response.getheader("Mcp-Session-Id") or self.session
            if response.status != 200:
                payload = response.read().decode()
                if notification and response.status == 202:
                    return None
                raise AssertionError(f"{method}: HTTP {response.status}: {payload}")
            if "text/event-stream" in response.getheader("Content-Type", ""):
                while line := response.readline():
                    if line.startswith(b"data:") and line[5:].strip():
                        message = json.loads(line[5:])
                        if message.get("id") == self.request_id:
                            break
                else:
                    raise AssertionError(f"{method}: SSE closed without a response")
            else:
                message = json.loads(response.read())
            assert message.get("id") == self.request_id, message
            assert "error" not in message, message
            return message["result"]
        finally:
            connection.close()


class StdioClient:
    def __init__(self, process, version):
        self.process = process
        self.version = version
        self.request_id = 0
        self.messages = queue.Queue()
        threading.Thread(target=self.read_messages, daemon=True).start()

    def read_messages(self):
        for line in self.process.stdout:
            self.messages.put(json.loads(line))

    def rpc(self, method, params=None, notification=False):
        self.request_id += 1
        message = {"jsonrpc": "2.0", "method": method, "params": params or {}}
        if not notification:
            message["id"] = self.request_id
        self.process.stdin.write(json.dumps(message).encode() + b"\n")
        self.process.stdin.flush()
        if notification:
            return None
        while True:
            response = self.messages.get(timeout=5)
            if response.get("id") == self.request_id:
                assert "error" not in response, response
                return response["result"]


@contextlib.contextmanager
def http_bridge(version, stateless=False, *extra_args):
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    args = ["--transport", "http", "--port", str(port), *extra_args]
    if stateless:
        args.append("--stateless")
    with bridge(*args) as process:
        deadline = time.monotonic() + 10
        while True:
            try:
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
                connection.request("GET", "/health")
                assert connection.getresponse().read() == b"ok"
                connection.close()
                break
            except OSError:
                if process.poll() is not None or time.monotonic() > deadline:
                    raise AssertionError("MCP HTTP server did not start")
                time.sleep(0.05)
        yield HttpClient(port, version)


class ProtocolTests(unittest.TestCase):
    def check_methods(self, client):
        tools = client.rpc("tools/list")
        self.assertIn("get_host_info", [t["name"] for t in tools["tools"]])
        self.assertTrue(all(t["inputSchema"]["type"] == "object" for t in tools["tools"]))
        result = client.rpc("tools/call", {"name": "get_host_info", "arguments": {}})
        self.assertFalse(result.get("isError", False))
        self.assertEqual(json.loads(result["content"][0]["text"])["deviceId"], "123456789")
        resources = client.rpc("resources/list")
        self.assertIn("quickdesk://host", [r["uri"] for r in resources["resources"]])
        templates = client.rpc("resources/templates/list")
        self.assertTrue(templates["resourceTemplates"])
        resource = client.rpc("resources/read", {"uri": "quickdesk://host"})
        self.assertEqual(json.loads(resource["contents"][0]["text"])["deviceId"], "123456789")
        prompts = client.rpc("prompts/list")
        self.assertTrue(prompts["prompts"])
        prompt = client.rpc("prompts/get", {"name": "operate_remote_desktop"})
        self.assertTrue(prompt["messages"])
        for response in (tools, result, resources, templates, resource, prompts, prompt):
            if client.version == MODERN:
                self.assertEqual(response["resultType"], "complete")
            else:
                self.assertNotIn("resultType", response)

    def test_legacy_http(self):
        for version in ("2024-11-05", "2025-11-25"):
            for stateless in (False, True):
                with self.subTest(version=version, stateless=stateless):
                    with http_bridge(version, stateless) as client:
                        result = client.rpc("initialize", {
                            "protocolVersion": version, "capabilities": {},
                            "clientInfo": {"name": "regression", "version": "1"},
                        })
                        self.assertEqual(result["protocolVersion"], version)
                        self.assertEqual(bool(client.session), not stateless)
                        client.rpc("notifications/initialized", notification=True)
                        self.check_methods(client)

    def test_modern_http(self):
        for stateless in (False, True):
            with self.subTest(stateless=stateless):
                with http_bridge(MODERN, stateless) as client:
                    result = client.rpc("server/discover")
                    self.assertIn(MODERN, result["supportedVersions"])
                    self.assertIsNone(client.session)
                    self.check_methods(client)

    def test_stdio(self):
        version = "2025-11-25"
        with bridge("--transport", "stdio") as process:
            client = StdioClient(process, version)
            result = client.rpc("initialize", {
                "protocolVersion": version, "capabilities": {},
                "clientInfo": {"name": "regression", "version": "1"},
            })
            self.assertEqual(result["protocolVersion"], version)
            client.rpc("notifications/initialized", notification=True)
            self.check_methods(client)

    def test_public_host_allowlist(self):
        with http_bridge(MODERN) as client:
            with self.assertRaisesRegex(AssertionError, "HTTP 403"):
                client.rpc("server/discover", host="mcp.example.com")
        with http_bridge(MODERN, False, "--allowed-hosts", "mcp.example.com") as client:
            self.assertIn(MODERN, client.rpc("server/discover", host="mcp.example.com")["supportedVersions"])
            self.assertIn(MODERN, client.rpc("server/discover")["supportedVersions"])
            with self.assertRaisesRegex(AssertionError, "HTTP 403"):
                client.rpc("server/discover", host="untrusted.example.com")


if __name__ == "__main__":
    unittest.main(verbosity=2)
