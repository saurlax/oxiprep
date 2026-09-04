# AI Agent Sidebar Specification

## Purpose

Provide an in-product, capability-aware client for conversing with locally installed ACP agents and allowing them to operate Oxiprep through injected MCP tools without coupling the user interface or command path to individual agent implementations.

## Requirements

### Requirement: Built-in docked AI workspace
Oxiprep SHALL include an AI tab in the default dock layout that can be docked and resized like the existing tool panels. The tab SHALL contain agent selection, connection state, conversation history, a prompt editor, send and cancel controls, and a new-conversation action.

#### Scenario: Default layout
- **WHEN** a user starts Oxiprep with the default dock layout
- **THEN** an AI tab is available in the right-side dock without replacing Outliner, Viewport, Properties, or Console

#### Scenario: Empty state
- **WHEN** no agent is connected
- **THEN** the AI tab shows a concise connection action and does not show implementation notes or placeholder controls

#### Scenario: Long update details
- **WHEN** agent capabilities, reasoning, plan, tool, or extension details exceed their display limit
- **THEN** the detail region has a bounded height and scrolls vertically inside the panel instead of expanding the whole sidebar
- **AND** neither the AI tab nor the detail region shows horizontal scrolling

#### Scenario: Composer remains visible
- **WHEN** the conversation is longer than the available AI tab height or the tab is resized shorter
- **THEN** the conversation scrolls within the remaining space while the prompt editor and Send or Cancel action remain visible at the bottom
- **AND** the dock does not add an outer horizontal or vertical scrollbar around the AI tab

### Requirement: Generic local ACP profiles
Oxiprep SHALL let the user create, edit, select, and remove local agent profiles defined by a display name, executable command, argument list, environment overrides, and working-directory policy. Connection and conversation code SHALL select behavior from negotiated ACP capabilities and SHALL NOT require agent-name-specific application logic.

#### Scenario: Connect an ACP-native agent
- **WHEN** the user selects a valid profile whose process implements the supported ACP version and chooses Connect
- **THEN** Oxiprep launches the configured process, completes ACP initialization, displays the implementation information reported by the process, and enables creation of a conversation

#### Scenario: Connect an agent through an adapter
- **WHEN** the user configures an ACP adapter command for an agent that does not implement ACP itself
- **THEN** Oxiprep connects to the adapter through the same profile and protocol flow used for an ACP-native agent

#### Scenario: Unsupported protocol version
- **WHEN** initialization cannot negotiate a supported stable ACP protocol version
- **THEN** Oxiprep closes the child process and reports a factual compatibility error without starting a session

#### Scenario: Executable is unavailable across supported platforms
- **WHEN** the configured executable cannot be started on Windows, macOS, or Linux
- **THEN** Oxiprep remains responsive, keeps the profile editable, and displays the operating-system launch error

### Requirement: Codex default profile
Oxiprep SHALL provide a built-in, resettable Codex profile that starts the ACP adapter for Codex using an editable command and arguments. The preset SHALL use the same generic ACP client as every other profile and SHALL NOT embed credentials or silently install software.

#### Scenario: Codex adapter is installed
- **WHEN** the Codex ACP adapter is available through the built-in profile and the user connects
- **THEN** Oxiprep negotiates an ACP connection and exposes the capabilities and authentication methods advertised by that adapter

#### Scenario: Codex adapter is missing
- **WHEN** the built-in Codex profile command is not installed or not discoverable
- **THEN** Oxiprep reports that the adapter could not be launched and presents copyable installation guidance without modifying the machine

#### Scenario: Restore the Codex preset
- **WHEN** the user resets the built-in Codex profile after editing it
- **THEN** Oxiprep restores the shipped command, arguments, and working-directory policy

### Requirement: ACP session lifecycle
After a successful connection, Oxiprep SHALL create ACP sessions with an absolute working directory and the Oxiprep MCP server configuration, send text prompts, stream session updates, allow the active prompt to be cancelled, and support starting a fresh conversation. Oxiprep SHALL call optional session operations only when the agent advertises the corresponding capability.

#### Scenario: Send and stream a prompt
- **WHEN** the user submits non-empty text in an active session
- **THEN** Oxiprep sends one ACP prompt request, disables duplicate submission for that active turn, and incrementally displays agent text, reasoning, plan, and tool-call updates received for the session

#### Scenario: Cancel an active prompt
- **WHEN** the user chooses Cancel while a prompt is active
- **THEN** Oxiprep sends the ACP cancellation notification for that session and keeps all conversation content received before cancellation visible

#### Scenario: Start a new conversation
- **WHEN** the user chooses New conversation while connected
- **THEN** Oxiprep creates a new ACP session and clears the visible conversation only after the new session succeeds

#### Scenario: Agent process exits
- **WHEN** the agent process exits or its protocol stream becomes unusable
- **THEN** Oxiprep marks the connection disconnected, preserves the visible conversation, reports the failure, and permits reconnection

### Requirement: Oxiprep MCP injection
Oxiprep SHALL provide an agent-neutral MCP server for application context and operations and SHALL include that server in the `mcpServers` supplied to every new, loaded, or resumed ACP session. The injected configuration SHALL use stdio MCP so every conforming ACP v1 agent can connect without agent-specific application code. Server and tool instructions SHALL identify the tools as operations on the currently running Oxiprep desktop application and direct references to the current or open model, object, selection, or view through live application context rather than filesystem search or computer-use tools. Because an agent may defer injected MCP tools, every ACP prompt SHALL also carry application-owned host guidance and a bounded GUI-state routing snapshot before the verbatim user content. The guidance SHALL direct the agent to discover the `oxiprep` tools and call `context.get`, identify the snapshot as potentially stale and non-authoritative for execution, and SHALL NOT appear as a user message in the visible transcript.

#### Scenario: New ACP session receives tools
- **WHEN** Oxiprep creates a session with any configured ACP-compatible agent
- **THEN** the session receives an `oxiprep` stdio MCP server configuration and the agent can discover its tools through MCP `tools/list`

#### Scenario: Codex receives the same MCP server
- **WHEN** a session is created through the built-in Codex ACP adapter profile
- **THEN** Codex receives the same `oxiprep` MCP configuration and tool schemas used for every other ACP agent

#### Scenario: Agent refers to the current model
- **WHEN** the user asks an injected agent to inspect or modify the current or open model or object
- **THEN** the per-prompt host guidance and MCP instructions direct the agent to discover deferred `oxiprep` tools, call `context.get`, and use Oxiprep operations against the live GUI state instead of searching the session working directory or operating the GUI through computer use
- **AND** the visible transcript retains only the user's verbatim request

#### Scenario: MCP readiness follows the initialized client
- **WHEN** the session-scoped endpoint is listening but no authenticated MCP client has completed initialization
- **THEN** the AI sidebar reports that Oxiprep MCP is waiting for the agent
- **AND WHEN** an authenticated MCP client completes initialization
- **THEN** the sidebar reports Oxiprep MCP ready until the last initialized client disconnects

#### Scenario: MCP bridge cannot connect
- **WHEN** the injected MCP process cannot establish its session-scoped connection to the running Oxiprep instance
- **THEN** tool calls fail with a factual connection error, the application remains responsive, and no document operation is executed

#### Scenario: ACP session ends
- **WHEN** an ACP session is closed or its agent disconnects
- **THEN** the MCP connection created for that session is revoked and can no longer invoke Oxiprep operations

### Requirement: Agent-visible application context
The Oxiprep MCP server SHALL provide read-only tools that describe the current project, models, bodies, body shape and meshability, meshes, selection, undo/redo availability, active project path, the suggested default mesh size, and available agent-callable operations using structured results. Context queries SHALL reflect the live application state at the time each tool is called.

#### Scenario: Query the current document
- **WHEN** an agent calls the document context tool after a project is opened or modified
- **THEN** the result describes the current models, bodies, body shape and meshability, mesh presence, suggested mesh size, dirty state, selection, and operation availability rather than a snapshot captured when the ACP session started

#### Scenario: Query after undo
- **WHEN** an agent executes a mutating tool, invokes Undo, and queries the document again
- **THEN** the returned context reflects the undone document and current undo/redo availability

#### Scenario: Stale target
- **WHEN** a tool targets an entity reference that is no longer valid in the current document state
- **THEN** Oxiprep rejects the call with a structured stale-or-missing-target error and performs no partial mutation

### Requirement: Shared GUI and agent operation path
Every shipped application operation marked agent-callable SHALL have an MCP tool schema generated from the same operation definition used by the GUI. MCP tools SHALL validate explicit parameters and dispatch through the same application service used by the corresponding GUI action. Document mutations SHALL execute through `Session` and undoable commands; view-only actions SHALL execute through the same viewport action path as the GUI.

#### Scenario: Create geometry through the agent
- **WHEN** an agent calls an available geometry creation tool with valid parameters
- **THEN** Oxiprep creates the same geometry, selection, console message, dirty state, and undo history entry as the equivalent GUI action

#### Scenario: Mesh through the agent
- **WHEN** an agent calls an available surface or volume mesh tool with valid targets and either a positive finite size or no size
- **THEN** Oxiprep applies the same validation and command implementation as the equivalent GUI action, uses the normal target-based default size when size is omitted, reports the actual size used, and the result can be undone once through the shared history

#### Scenario: CAD projection fails during meshing
- **WHEN** the CAD kernel cannot project a point while generating a surface or volume mesh
- **THEN** Oxiprep either uses a valid tessellation-based fallback or returns a factual mesh-operation error
- **AND** the application remains running, the document and command history are unchanged, and the same behavior applies to GUI and agent requests

#### Scenario: Import through the agent
- **WHEN** an agent calls an import tool with a supported absolute path
- **THEN** Oxiprep uses the same format handling and command path as GUI import without opening a native file dialog

#### Scenario: Delete and undo through the agent
- **WHEN** an agent deletes the current valid selection and subsequently calls Undo
- **THEN** the same deletion is reversed through the application history and the GUI reflects the restored state

#### Scenario: View-only action through the agent
- **WHEN** an agent invokes an available fit, standard-view, display, or clip operation
- **THEN** Oxiprep updates the active viewport through the same non-undoable view path used by the GUI

#### Scenario: Invalid tool arguments
- **WHEN** an MCP tool receives missing, malformed, out-of-range, or unsupported arguments
- **THEN** Oxiprep returns a structured validation error and changes neither the document nor the viewport

### Requirement: Serialized GUI-thread tool execution
Oxiprep SHALL enqueue MCP operation requests and execute application/document/view access on the GUI thread. Tool requests SHALL return only after the operation succeeds, fails, or is cancelled, and concurrent mutating calls SHALL be serialized in arrival order.

#### Scenario: Tool call during agent streaming
- **WHEN** the agent invokes an Oxiprep tool while ACP output is streaming
- **THEN** the operation is processed on the GUI thread without blocking continued viewport repainting or protocol I/O

#### Scenario: Concurrent mutations
- **WHEN** two mutating MCP calls arrive concurrently
- **THEN** Oxiprep executes them one at a time in a deterministic order and each call observes the result of the preceding completed mutation

#### Scenario: Operation result
- **WHEN** an enqueued command completes
- **THEN** its MCP result includes success or structured error information plus the resulting document revision and relevant affected entity references

### Requirement: MCP operation confirmation policy
Read-only context tools, view-only operations, and undoable document commands SHALL be eligible for direct execution. An operation that can discard unsaved work, replace the current project, write to a new external path, or terminate the application SHALL wait for explicit confirmation in the AI sidebar before execution.

#### Scenario: Undoable command executes directly
- **WHEN** an authorized agent invokes an undoable create, import, mesh, close, delete, undo, or redo operation with valid arguments
- **THEN** Oxiprep executes it through the command path without adding a second host confirmation prompt

#### Scenario: Destructive project lifecycle operation
- **WHEN** an agent requests New or Open while the current project has unsaved changes
- **THEN** Oxiprep presents the exact proposed operation and waits for user confirmation before changing the project

#### Scenario: External write target
- **WHEN** an agent requests Save As or another available write operation targeting a new external path
- **THEN** Oxiprep presents the resolved path and waits for user confirmation before writing

#### Scenario: Confirmation rejected or connection lost
- **WHEN** the user rejects a pending MCP operation or its ACP/MCP connection closes before a decision
- **THEN** Oxiprep returns cancellation or closes the request and does not execute the operation

### Requirement: Working-directory policy
Each profile SHALL support using the saved Oxiprep project directory, the Oxiprep process working directory, or a user-selected fixed directory as the ACP session working directory. Oxiprep SHALL resolve and validate the directory before session creation and SHALL send an absolute path on all supported platforms.

#### Scenario: Saved-project directory
- **WHEN** a profile uses the saved-project policy and the current document has a project path
- **THEN** the session working directory is the absolute parent directory of that project file

#### Scenario: Unsaved project fallback
- **WHEN** a profile uses the saved-project policy and the current document has not been saved
- **THEN** Oxiprep uses its absolute process working directory and identifies that fallback in the connection details

#### Scenario: Invalid fixed directory
- **WHEN** a profile's fixed directory is missing or is not a directory
- **THEN** Oxiprep does not create the session and lets the user correct the path

### Requirement: Capability-aware content rendering
Oxiprep SHALL render ACP text content as safe Markdown, render resource links as links, show tool calls and their status, and retain unsupported or unknown update types as a compact factual entry instead of failing the connection. Rich prompt attachments and remote transports SHALL remain unavailable unless a later change adds them.

#### Scenario: Markdown response
- **WHEN** the agent streams Markdown text
- **THEN** Oxiprep renders headings, lists, emphasis, links, and code blocks without executing embedded HTML or scripts

#### Scenario: Tool call progress
- **WHEN** the agent creates and later updates a tool call
- **THEN** the AI tab updates one stable tool-call entry rather than appending duplicate entries

#### Scenario: Unknown extension update
- **WHEN** the agent sends a valid ACP extension update that Oxiprep does not recognize
- **THEN** Oxiprep keeps the session usable and displays a compact unsupported-update entry

### Requirement: Explicit permission decisions
Oxiprep SHALL surface each ACP permission request in the AI tab with the agent-provided context and exactly the response options supplied by the agent. It SHALL send no approval until the user chooses an option and SHALL NOT persist or automatically reuse permission decisions in this release.

#### Scenario: Approve a request
- **WHEN** an agent requests permission and the user selects one of its approval options
- **THEN** Oxiprep returns that exact option to the requesting ACP session and records the decision in the visible conversation

#### Scenario: Reject a request
- **WHEN** an agent requests permission and the user selects a rejection option
- **THEN** Oxiprep returns that exact option and leaves the session available for the agent to continue or stop

#### Scenario: Disconnect with a pending request
- **WHEN** the user disconnects or the agent exits while permission is pending
- **THEN** Oxiprep dismisses the request without treating it as approved

### Requirement: Bounded client capabilities
In the first release, Oxiprep SHALL advertise no ACP client filesystem read, filesystem write, terminal execution, or terminal-authentication capability. Agent-side activity SHALL remain subject to the agent's own sandbox and ACP permission requests. Oxiprep application access SHALL be limited to the injected MCP tools and their declared schemas.

#### Scenario: Capability negotiation
- **WHEN** Oxiprep initializes any configured agent
- **THEN** the client capability declaration marks filesystem, terminal, and terminal authentication as unsupported

#### Scenario: Unsupported client request
- **WHEN** an agent nevertheless requests an unadvertised client filesystem or terminal operation
- **THEN** Oxiprep returns an ACP method-not-supported error and keeps the connection usable

#### Scenario: MCP access remains available
- **WHEN** filesystem and terminal client capabilities are disabled for an active ACP session
- **THEN** the agent can still discover and invoke the injected Oxiprep MCP tools

### Requirement: Profile persistence is separate from project data
Oxiprep SHALL persist agent profiles as local application preferences and SHALL NOT write profiles, environment overrides, credentials, or conversation content into `.oxiprep` project files. Profile displays SHALL obscure environment values whose names indicate secrets.

#### Scenario: Reopen the application
- **WHEN** the user restarts Oxiprep after saving a custom agent profile
- **THEN** the profile is available for selection while previous conversation content is not restored

#### Scenario: Save and share a project
- **WHEN** the user saves an Oxiprep project while an agent profile is configured
- **THEN** the project archive contains no agent profile, environment override, credential, or conversation data

### Requirement: Non-blocking and cleaned-up process operation
Agent connection, protocol I/O, streaming, and shutdown SHALL run without blocking viewport or dock interaction. Oxiprep SHALL cancel active prompts and terminate child processes it launched when the user disconnects or exits the application.

#### Scenario: Agent is generating
- **WHEN** an agent streams a long-running response
- **THEN** the user can continue interacting with the viewport and other dock panels

#### Scenario: Application exit
- **WHEN** the application closes with an agent process or prompt active
- **THEN** Oxiprep requests cancellation where possible and ensures its launched process is not left running
