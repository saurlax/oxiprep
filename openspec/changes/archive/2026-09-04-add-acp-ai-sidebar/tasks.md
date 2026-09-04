## 1. Dependencies and Boundaries

- [x] 1.1 Add stable official Rust ACP and MCP SDK/transport dependencies, Tokio process/runtime support, platform configuration storage, randomness/atomic test utilities, and safe egui Markdown rendering with unstable protocol features disabled; verify `cargo check` resolves on Rust 1.97.
- [x] 1.2 Create module boundaries for agent profiles, ACP runtime, MCP bridge/proxy, shared application operations, AI controller/transcript, and AI UI; verify the crate builds and domain modules do not depend on AI/ACP/MCP modules.

## 2. Agent Profiles and Preferences

- [x] 2.1 Implement the versioned profile model, tagged working-directory policies, stable IDs, validation, and shell-free process configuration; verify unit tests cover empty/NUL commands, argument preservation, all directory policies, unsaved-project fallback, invalid fixed paths, and absolute resolution.
- [x] 2.2 Implement platform-local profile persistence with atomic replacement and recovery from missing/invalid JSON; verify isolated round-trip/corrupt-file tests and confirm `.oxiprep` archives are untouched.
- [x] 2.3 Add the resettable Codex `codex-acp` profile plus manual global-install and editable `npx -y @agentclientprotocol/codex-acp` guidance; verify initial presence, override, delete, Reset, and no automatic installer execution.
- [x] 2.4 Mask secret-like environment values and remove them from diagnostics; verify tests cover token/key/password names and redacted launch failures.

## 3. Shared Application Operation Path

- [x] 3.1 Define `AppOperationSpec`/typed request/result structures with parameter schema, effect class, agent-callable flag, and confirmation class; verify registry tests reject duplicate IDs and validate every registered schema/result classification.
- [x] 3.2 Register live context and shipped project/document operations: document/selection/history queries, New, Open, Save, Save As, Import, geometry primitives, surface/volume mesh, Close, Delete, Undo, and Redo; verify registry coverage tests compare agent-callable entries with the shipped application operations intended for AI access.
- [x] 3.3 Register shipped view operations for Fit All/Selection, standard views, display toggles, and axis clip controls while keeping Quit non-agent-callable; verify view registry tests cover parameters and non-undoable classification.
- [x] 3.4 Refactor GUI actions to invoke the typed dispatcher and route mutating requests through the existing `Session`/`Command` methods and view requests through existing viewport methods; verify GUI regression tests and command history tests show no duplicate implementation path.
- [x] 3.5 Add a transient document revision and structured target references with pre-execution validation; verify tests cover increments after mutation/undo/redo/New/Open, stable reads, stale revisions, missing targets, and zero partial mutation on errors.
- [x] 3.6 Return structured operation results containing status, revision, command message, and affected selection/entity references while preserving Console logging; verify result tests compare GUI and dispatcher outcomes for create, mesh, delete/undo, import, and view actions.

## 4. Oxiprep MCP Service and Proxy

- [x] 4.1 Implement MCP `tools/list` from agent-callable operation specs and read-only live context tools; verify an MCP client sees valid JSON schemas and document queries reflect changes made after connection.
- [x] 4.2 Implement bounded MCP-to-GUI request queuing with one-shot results and serialized mutation execution on the egui thread; verify concurrent-call tests preserve arrival order, continued repainting, and live state visibility.
- [x] 4.3 Implement tool argument validation and typed dispatch for every registered document/project/view operation; verify MCP tests cover successful operations plus missing, malformed, out-of-range, unsupported, and stale arguments.
- [x] 4.4 Implement the session-scoped loopback MCP endpoint with random one-time authentication, frame/concurrency limits, and revocation; verify valid-token, invalid-token, oversized-message, concurrent-client, disconnect, and endpoint-cleanup tests.
- [x] 4.5 Implement and package the stdio MCP proxy that forwards only to the generated endpoint; verify a subprocess MCP client completes initialize, `tools/list`, context query, command call, and clean shutdown on Windows/macOS/Linux-compatible paths.
- [x] 4.6 Implement host confirmation for dirty New/Open, new external write targets, and future termination-class operations while allowing query, view, and undoable commands to execute directly; verify approve, reject, disconnect, timeout, and no-duplicate-confirmation scenarios.

## 5. ACP Connection Runtime and MCP Injection

- [x] 5.1 Implement shell-free agent spawning with piped ACP stdin/stdout, inherited environment plus overrides, bounded stderr, kill-on-drop, and structured errors; verify argument, environment, missing-executable, and stderr-bound tests.
- [x] 5.2 Implement ACP v1 initialization with Oxiprep implementation information, no client filesystem/terminal/terminal-auth capabilities, negotiated metadata, and unsupported-major rejection; verify the exact capability declaration and shutdown after failed negotiation.
- [x] 5.3 Generate the session-scoped `oxiprep` stdio MCP configuration and include it in ACP `session/new`, supported load, and resume requests; verify fake-agent tests inspect the command/args/env and use the injected server to discover and invoke a live Oxiprep tool.
- [x] 5.4 Implement capability-gated protocol authentication and ACP session creation using an absolute working directory; verify no-auth, authentication-required, success/failure, unavailable terminal auth, and directory-policy cases.
- [x] 5.5 Implement prompt, streamed updates, cancel, new conversation, disconnect, and failure handling over bounded channels with repaint notifications; verify ordering, duplicate-send prevention, transcript preservation, and replacement only after successful session creation.
- [x] 5.6 Implement staged shutdown for active prompts, ACP process, MCP proxy/listener, and pending ACP permission/MCP confirmation requests; verify normal exit, hung components, explicit disconnect, and controller drop leave no launched process or usable endpoint.

## 6. Transcript, Permissions, and UI

- [x] 6.1 Implement transcript reducers keyed by message/tool-call IDs for text, reasoning, plans, links, tool updates, MCP execution/results, warnings, and unknown extensions; verify chunk ordering, in-place updates, ACP/MCP correlation without duplicates, and fallback rendering.
- [x] 6.2 Implement ACP permission requests using exact agent-supplied options and one-shot responses, separately from MCP host confirmations; verify approve, reject, disconnect, duplicate-click, and unresolved-until-input tests.
- [x] 6.3 Implement safe Markdown with raw HTML disabled and explicit-click links; verify renderer snapshots cover supported Markdown and inert HTML/script content.
- [x] 6.4 Add `AI` beside `Properties` in the right dock without replacing baseline panels; verify a default-layout smoke test or captured inspection shows Outliner, Viewport, Properties, Console, and movable/resizable AI.
- [x] 6.5 Build connection/profile/authentication controls, factual agent/cwd/MCP state, Connect/Disconnect, New conversation, and profile editing/reset; verify UI-state tests cover every connection state, custom profile round-trip, and Codex guidance without installer execution.
- [x] 6.6 Build the streamed transcript, prompt Send/Cancel, ACP permission, and MCP confirmation UI; verify a fake agent can stream and invoke tools while viewport/dock interaction remains responsive.

## 7. Integration and Regression Verification

- [x] 7.1 Add deterministic fake ACP agent and MCP client/proxy fixtures for authentication, injection, tool discovery/calls, permissions, malformed/unknown messages, delays, cancellation, crashes, and hangs; verify the integration suite runs without network or credentials.
- [x] 7.2 Run an end-to-end agent scenario that queries the document, creates geometry, meshes it, changes the view, deletes it, and undoes the deletion; verify every mutation, selection, Console message, dirty flag, revision, and history result matches equivalent GUI operations.
- [x] 7.3 Verify the Codex preset configuration, missing-adapter guidance, ACP initialization, injected MCP discovery, prompt delivery, cancellation, and disconnect through deterministic tests without requiring an installed adapter, credentials, network access, or model output.
- [x] 7.4 Save/reopen representative projects before and after agent actions and compare archives; verify project format, geometry/mesh ownership, selection behavior, and undo/redo remain compatible and no profiles, tokens, conversations, or bridge data enter `.oxiprep`.
- [x] 7.5 Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets`; fix failures and record Windows/macOS/Linux process/IPC coverage required in CI.

## 8. Bounded AI Panel Layout

- [x] 8.1 Bound expanded agent capabilities and reasoning, plan, tool, or extension detail regions with internal vertical scrolling; verify long detail content cannot grow those regions past the configured maximum height.
- [x] 8.2 Reserve the prompt composer at the bottom and give only the conversation the remaining flexible height; verify long transcripts and short resized tabs keep the input and Send or Cancel action visible while the transcript scrolls.

## 9. Live MCP Grounding and Mesh Defaults

- [x] 9.1 Strengthen MCP server and generated tool instructions so agents treat current/open model, object, selection, and view as live Oxiprep GUI state and use `context.get` plus Oxiprep operations instead of filesystem search or computer use; verify instruction and tool-list tests.
- [x] 9.2 Extend live context with body shape, meshability, and suggested default mesh size; make mesh size optional, compute the normal target-based default after target selection, and report the actual size used; verify context, validation, target, default-size, and result tests.
- [x] 9.3 Track authenticated MCP clients after initialization and show waiting or ready based on the active initialized-connection count; verify valid, concurrent, and disconnected client transitions.
- [x] 9.4 Verify natural-language live-context grounding and volume-mesh tool behavior through deterministic prompt, MCP, and mesh regression tests.
- [x] 9.5 Run formatting, clippy, full deterministic tests, and strict OpenSpec validation.

## 10. Recoverable CAD Projection Failures

- [x] 10.1 Replace unnecessary CAD face projections in tessellation reuse and volume-boundary face labeling with existing surface mesh data; make remaining optional face projections recover locally by falling back or skipping refinement.
- [x] 10.2 Add a final panic boundary around each mesh generation before command state is committed, returning a factual `CommandError` and preserving document, revision, selection, and undo history on failure.
- [x] 10.3 Add regression tests for projection fallback, panic-to-error conversion, and zero partial mutation through the shared GUI/agent command path.
- [x] 10.4 Run formatting, clippy, full tests, strict OpenSpec validation, and a manual or representative volume-mesh reproduction.

## 11. AI Panel Scroll Containment

- [x] 11.1 Disable the dock's default outer scrollbars for the AI tab and constrain transcript/detail content to the available width while retaining their internal vertical scrolling.
- [x] 11.2 Add a regression test for the AI tab scrollbar policy and narrow-panel content width, then run formatting, clippy, full tests, strict OpenSpec validation, and diff checks.

## 12. Per-Prompt Oxiprep Grounding

- [x] 12.1 Prepend an application-owned host-context block and bounded GUI routing snapshot to every ACP prompt that identifies live GUI references, requires discovery of deferred `oxiprep` tools and `context.get`, and prevents filesystem or computer-use inspection from substituting for application context while keeping the user transcript verbatim.
- [x] 12.2 Add deterministic natural-request prompt-content regression coverage and run focused tests, formatting, clippy, full tests, and strict OpenSpec validation.

## 13. Test Value Audit

- [x] 13.1 Remove the ignored, authenticated, model-dependent Codex smoke and redundant tests that only exercise third-party getters or prove an unrelated adjacent file remains unchanged; retain deterministic cross-process ACP/MCP and command-path regressions.
- [x] 13.2 Verify there are no ignored tests or stale smoke-test references, then run formatting, clippy, the full test suite, and strict OpenSpec validation.
