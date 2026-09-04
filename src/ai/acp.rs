//! ACP process and session runtime.

use crate::ai::mcp::McpInjection;
use crate::ai::profile::{ResolvedProfile, is_secret_name};
use agent_client_protocol::schema::{ProtocolVersion, v1::*};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo, LineDirection};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

pub const ACP_CHANNEL_CAPACITY: usize = 64;
pub const MAX_STDERR_EVENT_CHARS: usize = 4096;

/// Application-owned guidance sent before every user prompt. ACP v1 has no
/// system/developer instruction field, so this separate content block grounds
/// the agent in its desktop host without changing the user's visible message.
pub const OXIPREP_HOST_CONTEXT: &str = r#"<oxiprep-host-context>
You are responding inside the AI panel of the currently running Oxiprep desktop application.

Interpret references such as "this", "current", or "open" model, object, body, selection, mesh, and view as live state in the Oxiprep GUI. They do not refer to source files or model files in the ACP working directory.

An MCP server named `oxiprep` is injected into this ACP session. Its tools may be deferred and absent from your initial tool list. For every request that depends on Oxiprep application state or asks you to operate on it:
1. Search/discover deferred tools for `oxiprep`, `context.get`, or an equivalent normalized tool name such as `mcp__oxiprep__context_get`.
2. Call `context.get` before reasoning about the current GUI state.
3. Use Oxiprep MCP operations, with the revision and target references returned by `context.get`, to perform application actions.
4. Refresh `context.get` immediately before a mutation if the state may have changed.

Do not use shell commands, filesystem or repository search, process inspection, or computer-use to infer Oxiprep GUI state. Use those capabilities only when the user explicitly asks about source code, workspace files, or an external operation not provided by Oxiprep. Do not claim that the current model or Oxiprep tools are unavailable until you have attempted deferred-tool discovery and `context.get`.
</oxiprep-host-context>"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpPrompt {
    pub user_text: String,
    pub gui_snapshot: Option<serde_json::Value>,
}

impl AcpPrompt {
    pub fn new(user_text: impl Into<String>) -> Self {
        Self {
            user_text: user_text.into(),
            gui_snapshot: None,
        }
    }

    pub fn with_gui_snapshot(
        user_text: impl Into<String>,
        gui_snapshot: serde_json::Value,
    ) -> Self {
        Self {
            user_text: user_text.into(),
            gui_snapshot: Some(gui_snapshot),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticationMethod {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct PendingPermission {
    pub tool_call_id: String,
    pub title: Option<String>,
    pub options: Vec<PermissionChoice>,
    pub response: oneshot::Sender<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionChoice {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug)]
pub enum AcpEvent {
    Initialized {
        agent_name: String,
        agent_version: Option<String>,
        capabilities: serde_json::Value,
        authentication: Vec<AuthenticationMethod>,
    },
    AuthenticationRequired,
    SessionReady,
    PromptStarted,
    SessionUpdate(Box<SessionUpdate>),
    PromptFinished(String),
    Permission(PendingPermission),
    Stderr(String),
    Error(String),
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcpCommand {
    Authenticate(String),
    Prompt(AcpPrompt),
    Cancel,
    NewConversation,
    Disconnect,
}

pub struct AcpConnection {
    commands: mpsc::Sender<AcpCommand>,
    task: Option<JoinHandle<()>>,
}

impl AcpConnection {
    pub fn spawn(
        handle: &tokio::runtime::Handle,
        profile: ResolvedProfile,
        injection: McpInjection,
        events: mpsc::Sender<AcpEvent>,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(ACP_CHANNEL_CAPACITY);
        let repaint = Arc::new(repaint);
        let task = handle.spawn(run(profile, injection, receiver, events, repaint));
        Self {
            commands,
            task: Some(task),
        }
    }

    pub fn try_send(&self, command: AcpCommand) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| format!("ACP command could not be queued: {error}"))
    }

    pub fn disconnect(&self) {
        let _ = self.try_send(AcpCommand::Disconnect);
    }

    pub fn shutdown(mut self, handle: &tokio::runtime::Handle) {
        self.disconnect();
        if let Some(mut task) = self.task.take() {
            handle.spawn(async move {
                if tokio::time::timeout(std::time::Duration::from_secs(1), &mut task)
                    .await
                    .is_err()
                {
                    task.abort();
                }
            });
        }
    }
}

impl Drop for AcpConnection {
    fn drop(&mut self) {
        self.disconnect();
        if let Some(task) = self.task.as_mut() {
            task.abort();
        }
    }
}

async fn run(
    profile: ResolvedProfile,
    injection: McpInjection,
    commands: mpsc::Receiver<AcpCommand>,
    events: mpsc::Sender<AcpEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let secret_values = profile
        .environment
        .iter()
        .filter(|(name, _)| is_secret_name(name))
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let mut config = AcpAgentConfig::new(&profile.command)
        .args(profile.args.clone())
        .envs(profile.environment.clone());
    // The SDK inherits the parent environment and passes each override as a
    // separate OS argument/environment entry; no shell parses this config.
    config = config.env("PWD", profile.working_directory.to_string_lossy());

    let stderr_events = events.clone();
    let stderr_repaint = repaint.clone();
    let agent = AcpAgent::new(config).with_debug(move |line, direction| {
        if direction == LineDirection::Stderr {
            let mut line = line.to_owned();
            for value in &secret_values {
                line = line.replace(value, "[redacted]");
            }
            if line.chars().count() > MAX_STDERR_EVENT_CHARS {
                line = format!(
                    "{}\n[stderr event truncated]",
                    line.chars()
                        .take(MAX_STDERR_EVENT_CHARS)
                        .collect::<String>()
                );
            }
            let _ = stderr_events.try_send(AcpEvent::Stderr(line));
            stderr_repaint();
        }
    });

    let notification_events = events.clone();
    let notification_repaint = repaint.clone();
    let permission_events = events.clone();
    let permission_repaint = repaint.clone();
    let connection_events = events.clone();
    let connection_repaint = repaint.clone();
    let result = agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                let _ = notification_events
                    .send(AcpEvent::SessionUpdate(Box::new(notification.update)))
                    .await;
                notification_repaint();
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                let (response, selected) = oneshot::channel();
                let pending = PendingPermission {
                    tool_call_id: request.tool_call.tool_call_id.0.to_string(),
                    title: request.tool_call.fields.title.clone(),
                    options: request
                        .options
                        .iter()
                        .map(|option| PermissionChoice {
                            id: option.option_id.0.to_string(),
                            name: option.name.clone(),
                            kind: permission_kind(option.kind).to_owned(),
                        })
                        .collect(),
                    response,
                };
                let choice = if permission_events
                    .send(AcpEvent::Permission(pending))
                    .await
                    .is_ok()
                {
                    permission_repaint();
                    selected.await.ok().flatten()
                } else {
                    None
                };
                let outcome = choice.map_or(RequestPermissionOutcome::Cancelled, |id| {
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id))
                });
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| {
            let events = connection_events.clone();
            let repaint = connection_repaint.clone();
            async move {
                connection_loop(connection, profile, injection, commands, events, repaint).await
            }
        })
        .await;

    if let Err(error) = result {
        send_event(&events, &repaint, AcpEvent::Error(error.to_string())).await;
    }
    send_event(&events, &repaint, AcpEvent::Disconnected).await;
}

async fn connection_loop(
    connection: ConnectionTo<Agent>,
    profile: ResolvedProfile,
    injection: McpInjection,
    mut commands: mpsc::Receiver<AcpCommand>,
    events: mpsc::Sender<AcpEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) -> Result<(), agent_client_protocol::Error> {
    let initialize = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(ClientCapabilities::default())
        .client_info(Implementation::new("oxiprep", env!("CARGO_PKG_VERSION")).title("Oxiprep"));
    let initialized = connection.send_request(initialize).block_task().await?;
    if initialized.protocol_version != ProtocolVersion::V1 {
        return Err(agent_client_protocol::util::internal_error(format!(
            "Unsupported ACP protocol version: {:?}; Oxiprep supports stable ACP v1.",
            initialized.protocol_version
        )));
    }
    let authentication = initialized
        .auth_methods
        .iter()
        .map(|method| AuthenticationMethod {
            id: method.id().0.to_string(),
            name: method.name().to_owned(),
            description: method.description().map(str::to_owned),
        })
        .collect::<Vec<_>>();
    let agent_name = initialized
        .agent_info
        .as_ref()
        .and_then(|info| info.title.clone())
        .or_else(|| {
            initialized
                .agent_info
                .as_ref()
                .map(|info| info.name.clone())
        })
        .unwrap_or_else(|| profile.name.clone());
    let agent_version = initialized
        .agent_info
        .as_ref()
        .map(|info| info.version.clone());
    let capabilities =
        serde_json::to_value(&initialized.agent_capabilities).unwrap_or(serde_json::Value::Null);
    send_event(
        &events,
        &repaint,
        AcpEvent::Initialized {
            agent_name,
            agent_version,
            capabilities,
            authentication: authentication.clone(),
        },
    )
    .await;

    let mcp_server = injected_server(injection);
    let mut session_id = match new_session(&connection, &profile, &mcp_server).await {
        Ok(session) => {
            send_event(&events, &repaint, AcpEvent::SessionReady).await;
            Some(session)
        }
        Err(error) if !authentication.is_empty() => {
            send_event(
                &events,
                &repaint,
                AcpEvent::Error(format!("Authentication is required: {error}")),
            )
            .await;
            send_event(&events, &repaint, AcpEvent::AuthenticationRequired).await;
            None
        }
        Err(error) => return Err(error),
    };

    let (completed_tx, mut completed_rx) = mpsc::channel::<Result<String, String>>(4);
    let mut prompt_active = false;
    loop {
        tokio::select! {
            Some(done) = completed_rx.recv(), if prompt_active => {
                prompt_active = false;
                match done {
                    Ok(reason) => send_event(&events, &repaint, AcpEvent::PromptFinished(reason)).await,
                    Err(error) => send_event(&events, &repaint, AcpEvent::Error(error)).await,
                }
            }
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    AcpCommand::Disconnect => break,
                    AcpCommand::Authenticate(method) => {
                        if !authentication.iter().any(|candidate| candidate.id == method) {
                            send_event(&events, &repaint, AcpEvent::Error("The selected authentication method is unavailable.".to_owned())).await;
                            continue;
                        }
                        match connection.send_request(AuthenticateRequest::new(method)).block_task().await {
                            Ok(_) => match new_session(&connection, &profile, &mcp_server).await {
                                Ok(session) => { session_id = Some(session); send_event(&events, &repaint, AcpEvent::SessionReady).await; }
                                Err(error) => send_event(&events, &repaint, AcpEvent::Error(error.to_string())).await,
                            },
                            Err(error) => send_event(&events, &repaint, AcpEvent::Error(format!("Authentication failed: {error}"))).await,
                        }
                    }
                    AcpCommand::NewConversation if !prompt_active => {
                        match new_session(&connection, &profile, &mcp_server).await {
                            Ok(session) => { session_id = Some(session); send_event(&events, &repaint, AcpEvent::SessionReady).await; }
                            Err(error) => send_event(&events, &repaint, AcpEvent::Error(format!("Could not create a new conversation: {error}"))).await,
                        }
                    }
                    AcpCommand::Prompt(prompt) if !prompt_active => {
                        let Some(session) = session_id.clone() else {
                            send_event(&events, &repaint, AcpEvent::Error("No ACP session is ready.".to_owned())).await;
                            continue;
                        };
                        if prompt.user_text.trim().is_empty() { continue; }
                        prompt_active = true;
                        send_event(&events, &repaint, AcpEvent::PromptStarted).await;
                        let request_connection = connection.clone();
                        let completed = completed_tx.clone();
                        connection.spawn(async move {
                            let result = request_connection
                                .send_request(prompt_request(session, prompt))
                                .block_task()
                                .await;
                            let mapped = result.map(|response| format!("{:?}", response.stop_reason)).map_err(|error| error.to_string());
                            let _ = completed.send(mapped).await;
                            Ok(())
                        })?;
                    }
                    AcpCommand::Cancel if prompt_active => {
                        if let Some(session) = session_id.clone() {
                            connection.send_notification(CancelNotification::new(session))?;
                        }
                    }
                    AcpCommand::Prompt(_) | AcpCommand::NewConversation | AcpCommand::Cancel => {}
                }
            }
        }
    }
    Ok(())
}

pub fn prompt_request(session: SessionId, prompt: AcpPrompt) -> PromptRequest {
    let mut host_context = OXIPREP_HOST_CONTEXT.to_owned();
    if let Some(snapshot) = prompt.gui_snapshot {
        host_context.push_str(
            "\n<oxiprep-gui-snapshot>\nThis bounded snapshot was captured when the user sent the prompt. It is a routing hint that confirms what the GUI displayed; it may become stale and never replaces `context.get`.\n",
        );
        host_context.push_str(
            &serde_json::to_string(&snapshot)
                .unwrap_or_else(|_| "{\"snapshot_error\":true}".to_owned()),
        );
        host_context.push_str("\n</oxiprep-gui-snapshot>");
    }
    host_context.push_str(
        "\nThe next content block is the user's verbatim request. Follow it within the live Oxiprep context above.",
    );
    PromptRequest::new(
        session,
        vec![
            ContentBlock::Text(TextContent::new(host_context)),
            ContentBlock::Text(TextContent::new(prompt.user_text)),
        ],
    )
}

async fn new_session(
    connection: &ConnectionTo<Agent>,
    profile: &ResolvedProfile,
    server: &McpServer,
) -> Result<SessionId, agent_client_protocol::Error> {
    let response = connection
        .send_request(new_session_request(profile, server))
        .block_task()
        .await?;
    Ok(response.session_id)
}

pub fn new_session_request(profile: &ResolvedProfile, server: &McpServer) -> NewSessionRequest {
    NewSessionRequest::new(profile.working_directory.clone()).mcp_servers(vec![server.clone()])
}

pub fn load_session_request(
    session_id: impl Into<SessionId>,
    profile: &ResolvedProfile,
    server: &McpServer,
) -> LoadSessionRequest {
    LoadSessionRequest::new(session_id, profile.working_directory.clone())
        .mcp_servers(vec![server.clone()])
}

pub fn resume_session_request(
    session_id: impl Into<SessionId>,
    profile: &ResolvedProfile,
    server: &McpServer,
) -> ResumeSessionRequest {
    ResumeSessionRequest::new(session_id, profile.working_directory.clone())
        .mcp_servers(vec![server.clone()])
}

pub fn injected_server(injection: McpInjection) -> McpServer {
    McpServer::Stdio(
        McpServerStdio::new(injection.name, injection.command)
            .args(injection.args)
            .env(
                injection
                    .environment
                    .into_iter()
                    .map(|(name, value)| EnvVariable::new(name, value))
                    .collect(),
            ),
    )
}

fn permission_kind(kind: PermissionOptionKind) -> &'static str {
    match kind {
        PermissionOptionKind::AllowOnce => "allow_once",
        PermissionOptionKind::AllowAlways => "allow_always",
        PermissionOptionKind::RejectOnce => "reject_once",
        PermissionOptionKind::RejectAlways => "reject_always",
        _ => "unknown",
    }
}

async fn send_event(
    events: &mpsc::Sender<AcpEvent>,
    repaint: &Arc<dyn Fn() + Send + Sync>,
    event: AcpEvent,
) {
    let _ = events.send(event).await;
    repaint();
}

#[cfg(debug_assertions)]
pub const ACP_FIXTURE_MODE: &str = "--oxiprep-acp-fixture";

/// Internal deterministic ACP process used by integration tests in debug builds.
#[cfg(debug_assertions)]
pub fn maybe_run_fixture_mode() -> Option<Result<(), String>> {
    use agent_client_protocol::{Client, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    let mut args = std::env::args_os();
    let _program = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new(ACP_FIXTURE_MODE)) {
        return None;
    }
    let mode = std::env::var("OXIPREP_ACP_FIXTURE_MODE").unwrap_or_else(|_| "normal".to_owned());
    record_fixture(
        "process",
        serde_json::json!({
            "args": args.map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "environment": std::env::var("OXIPREP_FIXTURE_VALUE").ok(),
            "pwd": std::env::var("PWD").ok(),
            "current_dir": std::env::current_dir().ok(),
        }),
    );
    if mode == "stderr" {
        eprintln!("{}", "diagnostic".repeat(20_000));
    }

    let authenticated = Arc::new(AtomicBool::new(!mode.starts_with("auth")));
    let cancelled = Arc::new(AtomicBool::new(false));
    let next_session = Arc::new(AtomicU64::new(0));
    let initialize_mode = mode.clone();
    let initialize = Agent.builder().name("oxiprep-fixture").on_receive_request(
        async move |request: InitializeRequest, responder, _connection| {
            record_fixture(
                "initialize",
                serde_json::to_value(&request).unwrap_or_default(),
            );
            if initialize_mode == "malformed" {
                println!("this is not json");
            }
            let version = if initialize_mode == "unsupported" {
                serde_json::from_value(serde_json::json!(0)).unwrap()
            } else {
                request.protocol_version
            };
            let mut response = InitializeResponse::new(version)
                .agent_capabilities(AgentCapabilities::new())
                .agent_info(Implementation::new("fixture-agent", "1.0").title("Fixture Agent"));
            if initialize_mode.starts_with("auth") {
                response = response.auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                    "fixture-auth",
                    "Fixture authentication",
                ))]);
            }
            responder.respond(response)
        },
        agent_client_protocol::on_receive_request!(),
    );

    let auth_state = authenticated.clone();
    let auth_mode = mode.clone();
    let agent = initialize.on_receive_request(
        async move |request: AuthenticateRequest, responder, _connection| {
            record_fixture(
                "authenticate",
                serde_json::to_value(&request).unwrap_or_default(),
            );
            if auth_mode == "auth_fail" {
                responder.respond_with_internal_error("fixture authentication failed")
            } else if request.method_id.0.as_ref() == "fixture-auth" {
                auth_state.store(true, Ordering::SeqCst);
                responder.respond(AuthenticateResponse::new())
            } else {
                responder.respond_with_internal_error("unknown authentication method")
            }
        },
        agent_client_protocol::on_receive_request!(),
    );

    let new_auth = authenticated.clone();
    let new_counter = next_session.clone();
    let agent = agent.on_receive_request(
        async move |request: NewSessionRequest, responder, _connection| {
            record_fixture(
                "session/new",
                serde_json::to_value(&request).unwrap_or_default(),
            );
            if !new_auth.load(Ordering::SeqCst) {
                return responder.respond_with_internal_error("authentication required");
            }
            let id = new_counter.fetch_add(1, Ordering::SeqCst) + 1;
            if id == 1
                && std::env::var_os("OXIPREP_ACP_FIXTURE_CALL_MCP").is_some()
                && let Err(error) = fixture_call_mcp(&request).await
            {
                return responder.respond_with_internal_error(error);
            }
            responder.respond(NewSessionResponse::new(format!("fixture-session-{id}")))
        },
        agent_client_protocol::on_receive_request!(),
    );

    let prompt_cancelled = cancelled.clone();
    let prompt_mode = mode.clone();
    let agent = agent.on_receive_request(
        async move |request: PromptRequest, responder, connection: ConnectionTo<Client>| {
            record_fixture(
                "session/prompt",
                serde_json::to_value(&request).unwrap_or_default(),
            );
            if prompt_mode == "crash" {
                std::process::exit(7);
            }
            prompt_cancelled.store(false, Ordering::SeqCst);
            let task_mode = prompt_mode.clone();
            let task_cancelled = prompt_cancelled.clone();
            let task_connection = connection.clone();
            connection.spawn(async move {
                task_connection.send_notification(SessionNotification::new(
                    request.session_id.clone(),
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                        TextContent::new("fixture response"),
                    ))),
                ))?;
                if task_mode == "permission" {
                    let response = task_connection
                        .send_request(RequestPermissionRequest::new(
                            request.session_id.clone(),
                            ToolCallUpdate::new(
                                "fixture-tool",
                                ToolCallUpdateFields::new().title("Fixture permission"),
                            ),
                            vec![
                                PermissionOption::new(
                                    "allow-once",
                                    "Allow once",
                                    PermissionOptionKind::AllowOnce,
                                ),
                                PermissionOption::new(
                                    "reject-once",
                                    "Reject once",
                                    PermissionOptionKind::RejectOnce,
                                ),
                            ],
                        ))
                        .block_task()
                        .await?;
                    record_fixture(
                        "permission/result",
                        serde_json::to_value(response).unwrap_or_default(),
                    );
                }
                if task_mode == "hang" || task_mode == "delay" {
                    for _ in 0..200 {
                        if task_cancelled.load(Ordering::SeqCst) {
                            responder.respond(PromptResponse::new(StopReason::Cancelled))?;
                            return Ok(());
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }
                responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                Ok(())
            })?;
            Ok(())
        },
        agent_client_protocol::on_receive_request!(),
    );

    let cancel_state = cancelled;
    let agent = agent.on_receive_notification(
        async move |notification: CancelNotification, _connection| {
            record_fixture(
                "session/cancel",
                serde_json::to_value(&notification).unwrap_or_default(),
            );
            cancel_state.store(true, Ordering::SeqCst);
            Ok(())
        },
        agent_client_protocol::on_receive_notification!(),
    );

    Some(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                runtime
                    .block_on(agent.connect_to(Stdio::new()))
                    .map_err(|error| error.to_string())
            }),
    )
}

#[cfg(debug_assertions)]
fn record_fixture(event: &str, value: serde_json::Value) {
    use std::io::Write;

    let Some(path) = std::env::var_os("OXIPREP_ACP_FIXTURE_RECORD") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            file,
            "{}",
            serde_json::json!({"event": event, "value": value})
        );
    }
}

#[cfg(debug_assertions)]
async fn fixture_call_mcp(request: &NewSessionRequest) -> Result<(), String> {
    use rmcp::ServiceExt;
    use rmcp::model::CallToolRequestParams;
    use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};

    let server = request
        .mcp_servers
        .iter()
        .find_map(|server| match server {
            McpServer::Stdio(server) if server.name == "oxiprep" => Some(server),
            _ => None,
        })
        .ok_or_else(|| "oxiprep MCP server was not injected".to_owned())?;
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(&server.command).configure(|command| {
            command.args(&server.args);
            for variable in &server.env {
                command.env(&variable.name, &variable.value);
            }
        }),
    )
    .map_err(|error| error.to_string())?;
    let client = ().serve(transport).await.map_err(|error| error.to_string())?;
    let tools = client
        .list_tools(Default::default())
        .await
        .map_err(|error| error.to_string())?;
    if !tools.tools.iter().any(|tool| tool.name == "context.get") {
        return Err("context.get was not exposed by the injected MCP server".to_owned());
    }
    let result = client
        .call_tool(CallToolRequestParams::new("context.get"))
        .await
        .map_err(|error| error.to_string())?;
    record_fixture(
        "mcp/context.get",
        serde_json::to_value(result).unwrap_or_default(),
    );
    drop(client);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::mcp::{MCP_PROXY_MODE, MCP_TOKEN_ENV};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn client_capabilities_expose_no_filesystem_or_terminal() {
        let capabilities = ClientCapabilities::default();
        assert!(!capabilities.fs.read_text_file);
        assert!(!capabilities.fs.write_text_file);
        assert!(!capabilities.terminal);
        assert!(capabilities.session.is_none());
    }

    #[test]
    fn injection_is_a_stable_stdio_mcp_server() {
        let server = injected_server(McpInjection {
            name: "oxiprep".to_owned(),
            command: PathBuf::from("/absolute/oxiprep"),
            args: vec![MCP_PROXY_MODE.to_owned(), "127.0.0.1:1234".to_owned()],
            environment: vec![(MCP_TOKEN_ENV.to_owned(), "secret".to_owned())],
        });
        let value = serde_json::to_value(&server).unwrap();
        assert_eq!(value["name"], "oxiprep");
        assert_eq!(value["command"], "/absolute/oxiprep");
        assert_eq!(value["env"][0]["name"], MCP_TOKEN_ENV);
        assert!(value.get("type").is_none());

        let profile = ResolvedProfile {
            id: "test".to_owned(),
            name: "Test".to_owned(),
            command: "agent".to_owned(),
            args: vec![],
            environment: BTreeMap::new(),
            working_directory: PathBuf::from("/absolute/workspace"),
        };
        for request in [
            serde_json::to_value(new_session_request(&profile, &server)).unwrap(),
            serde_json::to_value(load_session_request("saved", &profile, &server)).unwrap(),
            serde_json::to_value(resume_session_request("saved", &profile, &server)).unwrap(),
        ] {
            assert_eq!(request["cwd"], "/absolute/workspace");
            assert_eq!(request["mcpServers"][0], value);
        }
    }

    #[test]
    fn every_prompt_precedes_verbatim_user_text_with_host_grounding() {
        let request = prompt_request(
            SessionId::new("session-1"),
            AcpPrompt::with_gui_snapshot(
                "please surface mesh this object",
                serde_json::json!({"revision": 7, "model_count": 1}),
            ),
        );
        let value = serde_json::to_value(request).unwrap();
        let blocks = value["prompt"].as_array().unwrap();

        assert_eq!(blocks.len(), 2);
        let guidance = blocks[0]["text"].as_str().unwrap();
        assert!(guidance.contains("currently running Oxiprep desktop application"));
        assert!(guidance.contains("tools may be deferred"));
        assert!(guidance.contains("mcp__oxiprep__context_get"));
        assert!(guidance.contains("Do not use shell commands"));
        assert!(guidance.contains("<oxiprep-gui-snapshot>"));
        assert!(guidance.contains("\"revision\":7"));
        assert!(guidance.contains("never replaces `context.get`"));
        assert_eq!(blocks[1]["text"], "please surface mesh this object");
    }
}
