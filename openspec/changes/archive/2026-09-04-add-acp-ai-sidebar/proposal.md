## Why

Oxiprep has no in-product way to consult or run a local AI agent while preparing a model. A built-in, dockable AI sidebar backed by the Agent Client Protocol (ACP), with Oxiprep operations injected as Model Context Protocol (MCP) tools, gives users one consistent conversation and command surface while allowing them to choose among independently installed, ACP-compatible agents.

This is an explicit user-requested departure from the current P0-only priority: AI assistance is not present in the brownfield baseline and overlaps the later reserved scripting/extension direction. The change is additive and must not displace P0 commands or bypass the existing command write path.

## What Changes

- Add a dockable AI sidebar for agent selection, connection state, streamed conversation content, tool activity, permission decisions, prompt submission, and cancellation, with bounded update details and a composer that remains visible at the bottom.
- Add a generic local ACP client that launches configured agents as child processes and communicates over JSON-RPC via stdio, with capability negotiation rather than agent-name checks.
- Add an agent-neutral Oxiprep MCP tool service and inject it into every ACP session through `mcpServers`, using the universally required stdio MCP transport.
- Expose current document and selection context plus tool equivalents of shipped GUI operations. Tool instructions explicitly ground agents in the currently running Oxiprep application, live context identifies meshable bodies and the normal suggested mesh size, and mesh tools may omit size to use that default. Mutating tools execute on the GUI thread through the same `Session` and undoable `Command` path as GUI actions; view-only tools use the same viewport action path.
- Add persistent local agent launch profiles containing a command, arguments, environment overrides, and working-directory policy.
- Ship a Codex profile that invokes the ACP adapter for Codex, while keeping Codex-specific installation and launch details outside the protocol/UI core.
- Allow any locally installed ACP-compatible agent to be added without application code changes; agents without native ACP support require an external ACP adapter.
- Limit the first release to local stdio agents and text conversations. Remote ACP transports are not included.
- Route agent permission requests through an explicit user decision in the sidebar. Do not expose client-side filesystem write or terminal capabilities; Oxiprep domain actions are available only through the injected MCP service and its validation/confirmation policy.
- Report MCP as ready only after an authenticated agent-side MCP client has completed initialization, and return to waiting when the last initialized connection closes.
- Contain third-party CAD projection panics during shared GUI/agent mesh generation, avoid unnecessary face projection while labeling volume-mesh boundaries, and return a normal operation error without partially changing the document when the kernel cannot complete a mesh.

## Capabilities

### New Capabilities

- `ai-agent-sidebar`: Docked AI conversation UX, local agent profile management, generic ACP session lifecycle, injected Oxiprep MCP tools, command-path execution, capability-aware rendering, permissions, and the default Codex profile.

### Modified Capabilities

None.

## Impact

- Affected baseline sources: `spec/README.md`, `spec/phasing.md`, `spec/product.md`, `spec/architecture.md`, `spec/ui.md`, and `spec/commands.md`.
- Affected code will include the egui dock shell, new AI/ACP and MCP bridge modules, application preferences/profile persistence, command/view dispatch, and process lifecycle cleanup.
- New dependencies are expected for the official Rust ACP and MCP SDKs, async process/I/O execution, local IPC, and Markdown rendering compatible with egui.
- The Codex preset depends on a locally available `npx`/Node runtime or an installed `codex-acp` executable; Oxiprep will not bundle credentials or silently install third-party software.
- Existing project files and document serialization remain unchanged. No breaking change is planned.
