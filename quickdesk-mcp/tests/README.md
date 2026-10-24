# MCP compatibility regression tests

From the repository root:

```sh
cargo build --locked --manifest-path quickdesk-mcp/Cargo.toml
cargo test --locked --manifest-path quickdesk-mcp/Cargo.toml
python3 -m pip install 'websockets>=14,<17'
python3 quickdesk-mcp/tests/test_protocol.py
```

The Python suite launches the real bridge against a local fake QuickDesk
WebSocket server. It uses temporary storage and ephemeral loopback ports; it
does not connect to a desktop, OpenAI, or an external service.

Coverage:

- HTTP `initialize` for MCP 2024-11-05 and 2025-11-25, with and without sessions.
- HTTP `server/discover` for MCP 2026-07-28, always without sessions.
- stdio initialization and method dispatch.
- Tools list/schema, a tool call, resources list/read/templates, and prompts list/get.
- Version-specific result shapes (`resultType` is omitted for older clients).
- Configured public Host acceptance, loopback access, and rejection of other hosts.

Set `QUICKDESK_MCP_BINARY` to test another build. Live OpenAI integration still
requires a reachable, authenticated MCP endpoint and is not covered by this suite.
