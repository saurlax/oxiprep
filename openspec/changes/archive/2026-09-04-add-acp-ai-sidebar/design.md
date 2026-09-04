## Context

See `proposal.md` for motivation and `specs/ai-agent-sidebar/spec.md` for the behavior contract. Oxiprep currently keeps dock UI and application-owned transient state in `src/app.rs`, while persistent CAE state and mutations flow through `Session` and undoable commands. Existing commands cover import, create/add body, close, delete, and mesh; project lifecycle and undo/redo are exposed by `Session`, while view actions intentionally remain outside undo history. There is no serializable operation registry, async runtime, preference store, Markdown renderer, MCP service, or external-process service.

ACP is a bidirectional JSON-RPC protocol. For local agents, the client owns a child process and communicates over stdin/stdout. ACP v1 session setup lets the client inject MCP server configurations, and every conforming ACP v1 agent supports stdio MCP. ACP v1 is stable; ACP v2 remains draft and is not a production target for this change.

This user-requested capability is additive and outside the current P0 sequence. The AI conversation is not project data, but agent-invoked application actions must enter the same one-way dependency and command path as GUI actions.

## Goals / Non-Goals

**Goals:**

- Use one ACP implementation and one Oxiprep MCP tool surface for every compatible agent.
- Let agents inspect live application context and execute agent-callable operations through the same dispatcher, `Session`, commands, and viewport actions used by the GUI.
- Keep agent, protocol, and MCP proxy I/O off the egui thread while executing application state access serially on that thread.
- Make the security boundary visible: broad filesystem/terminal client methods are absent, MCP schemas are bounded, and risky project/file operations require application confirmation.
- Make the integration diagnosable with structured states, bounded logs, and deterministic ACP/MCP fixtures.

**Non-Goals:**

- Giving agent processes direct memory access to geometry, meshes, selection, viewport, or command objects.
- Implementing per-agent Oxiprep tool adapters, bypassing shared application handlers, or inventing custom ACP methods for CAE operations.
- Supporting remote ACP transports, ACP v2 draft behavior, conversation persistence, simultaneous agent processes, rich prompt attachments, or client-hosted terminals.
- Bundling Node.js, Codex credentials, or third-party agent binaries, or silently installing packages.

## Decisions

### 1. Separate agent launch profiles, ACP, MCP, and application dispatch

Add an `ai` subsystem with this dependency flow:

```text
AI dock UI -> AI controller/state -> ACP runtime -> agent process
       |              |                         -> injected stdio MCP proxy
       |              +-> local agent profiles              |
       +-> shared application dispatcher <- in-process MCP tool service
                         |
                         +-> Session/Command or viewport action
```

The ACP runtime accepts a resolved launch profile and never branches on agent name. The built-in Codex profile defaults to display name `Codex`, command `codex-acp`, no arguments, and the saved-project directory policy. Missing-command UI provides manual `npm install -g @agentclientprotocol/codex-acp` guidance. Users may instead configure `npx -y @agentclientprotocol/codex-acp` or `CODEX_PATH`; Oxiprep does not run an installer.

An agent with native ACP support needs only a profile. An agent without ACP support needs an external adapter that presents ACP over stdio. Every compatible agent receives the same `oxiprep` MCP server. A brand-specific Rust trait or tool layer is rejected because it recreates the integration matrix ACP and MCP remove.

### 2. Target stable ACP v1 and inject MCP during session setup

Use the official Rust ACP SDK and Tokio transport utilities with unstable features disabled. Advertise ACP v1, reject unsupported majors, and record implementation information, capabilities, authentication methods, modes, models/config options, and optional session operations.

Include the session-scoped `oxiprep` stdio MCP configuration in `session/new`, `session/load`, and `session/resume`. Calls for optional ACP operations remain capability-gated. The wire boundary retains extension metadata and maps unknown extension updates to a visible fallback rather than failing a valid connection.

Manual JSON-RPC/schema implementation is rejected because it increases protocol drift. ACP v2 can later be added beside v1 after stabilization.

### 3. Run one ACP connection worker and exchange bounded events with egui

Create one Tokio runtime/worker owned by the AI controller. It owns the agent child stdin/stdout, protocol tasks, stderr capture, and MCP bridge listener. Bounded channels carry UI commands and ordered events. Each event requests repaint, and the UI drains a bounded amount per frame so a noisy agent cannot starve the viewport.

The ACP connection state is explicit: `Disconnected -> Starting -> AuthenticationRequired/Ready -> PromptActive -> Ready`, with failure transitions from connected states. MCP readiness is tracked separately by counting authenticated connections that have completed MCP initialization: the listener alone is `waiting for agent`, the first initialized connection makes it `ready`, and the last disconnect returns it to waiting. One process may back successive sessions, but only one session is active in the UI. Switching profile or disconnecting closes the current connection first.

Agent spawn uses separate command/argument fields rather than a shell string, inherits the parent environment plus explicit overrides, pipes stdin/stdout, bounds stderr, and enables kill-on-drop. Shutdown cancels active work, closes protocol I/O, waits briefly, then terminates the child. Reading the child or waiting for it on the egui thread is rejected.

### 4. Provide an in-process MCP service through a universal stdio proxy

Use the stable Rust MCP SDK for the agent-facing tool service. The service accepts MCP traffic over a session-scoped local loopback endpoint, authenticates a one-time random token, converts tool calls into typed application requests, and awaits one-shot results. A small stdio proxy supplied in `mcpServers` forwards MCP JSON-RPC between the agent and this endpoint.

Ship the proxy as an internal mode of the Oxiprep executable or a companion binary chosen during packaging. Oxiprep generates its command path, endpoint, and short-lived token; they are not part of an agent profile. Bind only to loopback, accept only the expected token, bound frame size/concurrency, and revoke the endpoint with the ACP session. Direct HTTP MCP was considered, but stdio is the only transport required of every ACP v1 agent.

The initial MCP surface contains:

- live context: project/document summary, models/bodies, body shape and meshability, mesh presence, selection, dirty state, undo/redo availability, document revision, suggested default mesh size, and agent-callable operations;
- document/project actions: New, Open, Save, Save As, Import, shipped geometry primitives, surface/volume mesh, Close, Delete, Undo, and Redo;
- view actions: Fit All/Selection, standard views, display toggles, and axis clip controls.

Tools use explicit structured arguments instead of native dialogs. Mesh size is optional; after applying validated target references, omission computes the same target-based bounding-box-diagonal default used by the GUI and the result reports the actual value. A central `AppOperationSpec` registry identifies the operation, schema, result, effect class, and whether it is agent-callable. GUI actions and MCP registration reference the same operation definition and typed dispatcher. Operations such as Quit can remain non-agent-callable.

MCP server instructions and each generated tool description explicitly state that they operate the currently running Oxiprep desktop application. They tell agents to resolve phrases such as current/open model, object, selection, or view through `context.get` and Oxiprep tools, not through filesystem discovery or computer use. This grounding is protocol-neutral and applies to Codex and every other ACP agent.

Some agents, including Codex configurations that defer MCP tools, do not expose those descriptions until the model actively searches for a tool. Therefore each ACP prompt contains two text content blocks: an application-owned host-context block followed by the user's verbatim request. The host block identifies the live Oxiprep GUI as the meaning of deictic model references, explicitly requires deferred-tool discovery and `context.get`, and forbids using filesystem, process, or computer-use inspection as a substitute for GUI state. It also contains a bounded routing snapshot with the current revision, project state, model/body summaries, selection, suggested mesh size, and basic view state so the agent immediately knows that GUI context exists. The snapshot is explicitly non-authoritative and potentially stale; targeted operations still require a fresh `context.get`. The visible transcript records only the user block. This remains agent-neutral and uses standard ACP v1 prompt content because ACP v1 has no system/developer-instruction field.

Custom ACP methods or raw callbacks are rejected because they are not portable. Stable ACP v1 MCP injection is the interoperable tool boundary.

### 5. Execute MCP requests on the GUI thread through existing handlers

The MCP service sends bounded `AppOperationRequest` values to the application controller. The egui update loop drains and executes them serially. Read requests borrow live state only during that frame. Mutating requests call the same `Session` function and undoable `Command` as their GUI equivalent. View requests call the same viewport method and remain outside undo history.

Add a transient monotonically increasing document revision, incremented after successful mutation, undo/redo, New, or Open. Context results include the revision and structured entity references. Targeted mutating tools carry the revision associated with those references and revalidate every target immediately before execution; a mismatch returns a stale-reference error without partial mutation. This does not change `.oxiprep` serialization.

Tool results include status, new revision, command message, and relevant selection/entity references. The same message enters Console and the GUI refreshes normally. Long-running geometry/mesh work follows the background task direction rather than transferring kernel ownership to the ACP worker.

Direct `Document`, `Session`, or `Viewport` access from async workers is rejected because it risks races, non-`Send` kernel state, divergent UI state, and bypassed history.

### 6. Keep ACP permissions and MCP host confirmation distinct

ACP permission requests remain agent-originated protocol state. The sidebar shows the agent-provided context/options verbatim and resolves only the option the user clicks; decisions are not remembered.

MCP operations are independently classified by `AppOperationSpec`. Live queries, view-only operations, and undoable document commands execute directly once called. New/Open that would discard dirty state, Save As or another new external write target, and any future termination action pause in the GUI dispatcher and show the exact operation/path. Reject, disconnect, or timeout closes the MCP call without execution. Agent-side ACP permission mode cannot bypass this host safeguard.

Protocol-driven agent authentication is supported. Terminal authentication is not advertised because Oxiprep does not host an interactive terminal. Environment values with secret-like names are masked in the profile UI and omitted from diagnostics.

Confirming every undoable action again is rejected as duplicate friction; trusting only agent-side permission behavior for irreversible host actions is also rejected.

### 7. Persist profiles separately from project data

Store versioned profile JSON under the platform application configuration directory using atomic replacement. The schema contains profile ID, name, executable, arguments, environment overrides, and working-directory policy. The Codex profile is merged by stable built-in ID; edit/delete creates an override and Reset removes it.

Validate executable and paths before launch, never shell-expand arguments, and keep conversation, bridge credentials, connection metadata, and MCP state in memory only. No AI data enters `.oxiprep` archives.

### 8. Render a conservative ACP/MCP transcript

Maintain transcript items keyed by ACP message/tool-call IDs. Text chunks append; tool-call updates patch existing items. MCP bridge events correlate with the ACP tool-call ID when available so one entry shows progress and final Oxiprep result. Unknown valid extensions render compactly rather than breaking the session.

Render Markdown with raw HTML disabled. Open resource links only on explicit click. Present reasoning, plans, tool status, ACP permissions, MCP confirmations, results, and errors as native egui rows or collapsible sections. Prompt input remains text-only.

### 9. Add the AI tab without replacing baseline panels

Add `AI` beside `Properties` in the right dock node, preserving Outliner, Viewport, Properties, and Console. The tab remains movable/resizable through existing docking and uses egui defaults.

Within the tab, connection and pending-decision controls consume space from the top, the composer reserves its natural height at the bottom, and only the conversation receives the remaining flexible height and an internal vertical scrollbar. The AI tab disables `egui_dock`'s default outer horizontal and vertical scrollbars because they duplicate the panel's own layout and can push the composer out of view. Expanded capability JSON and reasoning, plan, tool, or extension bodies use a fixed maximum height with their own vertical scrolling so one verbose update cannot displace the composer. Scroll content is constrained to the available panel width so long text cannot create horizontal scrolling.

Split UI code into connection/profile controls, transcript, pending ACP permission or MCP confirmation, and composer components so protocol/tool state does not further concentrate unrelated code in `app.rs`.

### 10. Contain CAD projection failures inside the mesh command

The current cadrum `Face::project` API asserts when OCCT `BRepExtrema_ExtPF` fails, even for recoverable geometric inputs. Mesh generation therefore treats face projection as a fallible operation: optional projection steps use a panic boundary and fall back to the already available CAD tessellation or skip refinement, while volume-boundary face IDs are transferred from the generated surface mesh rather than projecting every boundary centroid onto every CAD face.

The shared `MeshBodies` command also places a final unwind boundary around each complete mesh generation. Any remaining panic from third-party CAD/mesh code becomes `CommandError::Failed` before backups, generated meshes, document state, revision, or undo history are committed. This outer boundary is required even when known projection sites have local recovery because future kernel calls may acquire the same fail-fast behavior. GUI and MCP dispatch continue to share this command path and receive the same factual error.

## Risks / Trade-offs

- [ACP agents differ in optional behavior] -> Gate optional calls, retain unknown extensions, and test with strict fake agents plus Codex ACP.
- [Injected tools diverge from GUI behavior] -> Generate schemas from a shared operation registry and route both callers to one typed dispatcher and existing handlers.
- [MCP calls race document mutations or kernel state] -> Execute application access serially on the GUI thread and use revision/stale-reference validation.
- [Another local process invokes the internal MCP endpoint] -> Bind only to loopback, require unguessable session tokens, bound traffic, and revoke listeners promptly.
- [A coding agent modifies files through its own tools] -> Display identity/cwd, preserve ACP permission controls, default to non-full-access modes, and expose no broad client write/terminal capability.
- [Approvals are bypassed or duplicated] -> Keep ACP permission and host MCP confirmation as separate layers; only risky non-undoable host operations require the latter.
- [A hung/noisy process leaks resources or harms responsiveness] -> Use bounded channels/output, staged shutdown, kill-on-drop, and per-frame request/event limits.
- [Environment overrides contain secrets] -> Mask likely secrets, omit them from logs, warn before persistence, and recommend inherited environment or agent-managed authentication.
- [Codex preset is not turnkey without an adapter] -> Provide a stable preset and manual guidance while leaving installation under user control.
- [New dependencies increase binary size] -> Keep ACP/MCP features minimal and measure release artifacts.
- [A third-party CAD call panics for a recoverable shape] -> Avoid unnecessary projections, recover locally where a tessellation fallback exists, and contain the complete mesh calculation at the command boundary before document mutation.
- [The work competes with P0] -> Avoid solver/postprocessing work and preserve project/geometry/mesh ownership and serialization.

## Migration Plan

1. Add dependencies, profile schema/store, shared operation registry/dispatcher, and Codex profile without changing `.oxiprep` serialization.
2. Add the GUI-thread MCP tool service and authenticated stdio proxy; verify context, GUI parity, undo/redo, view actions, confirmation, and stale references with an MCP client.
3. Add the ACP worker, inject the MCP configuration, and verify a fake agent discovers and invokes Oxiprep tools.
4. Add the AI dock UI, content rendering, authentication, ACP permissions, MCP confirmations, cancellation, and cleanup.
5. Run cross-platform deterministic ACP/MCP integration checks, including the shipped Codex profile configuration and a natural-language prompt payload. Validate the installed real adapter manually through the product UI when needed; do not retain an ignored, authenticated, model-dependent test in the automated suite.

Rollback removes the AI tab, ACP runtime, MCP listener/proxy, agent-facing operation metadata, profile store, and dependencies. Existing project files need no migration. Local preference files may remain harmlessly or be removed by the user; rollback never rewrites project archives.
