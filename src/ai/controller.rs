//! GUI-facing AI connection state and bounded event channels.

use crate::ai::acp::{
    AcpCommand, AcpConnection, AcpEvent, AuthenticationMethod, PendingPermission,
};
use crate::ai::mcp::{GuiOperationCall, GuiOperationReceiver, McpEndpoint, gui_operation_channel};
use crate::ai::profile::{AgentProfile, CODEX_PROFILE_ID, ProfilePreferences, ProfileStore};
use crate::ai::transcript::Transcript;
use crate::app_operation::agent_prompt_snapshot;
use crate::app_operation::{HostApproval, OperationError, confirmation_for, dispatch};
use crate::session::Session;
use crate::viewport::Viewport;
use eframe::egui;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

const EVENTS_PER_FRAME: usize = 64;
const OPERATIONS_PER_FRAME: usize = 8;
const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Starting,
    AuthenticationRequired,
    Ready,
    PromptActive,
    Failed(String),
}

impl ConnectionState {
    pub fn label(&self) -> &str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Starting => "Connecting",
            Self::AuthenticationRequired => "Authentication required",
            Self::Ready => "Ready",
            Self::PromptActive => "Working",
            Self::Failed(_) => "Disconnected",
        }
    }

    pub fn connected(&self) -> bool {
        matches!(
            self,
            Self::AuthenticationRequired | Self::Ready | Self::PromptActive
        )
    }
}

pub struct PendingHostConfirmation {
    pub operation: String,
    pub detail: String,
    call: GuiOperationCall,
    started: Instant,
}

pub struct AiController {
    runtime: Runtime,
    store: Option<ProfileStore>,
    preferences: ProfilePreferences,
    selected_profile: Option<String>,
    profile_warning: Option<String>,
    application_dir: std::path::PathBuf,
    connected_cwd: Option<std::path::PathBuf>,
    mcp_endpoint: Option<String>,
    state: ConnectionState,
    agent_name: Option<String>,
    agent_version: Option<String>,
    agent_capabilities: Option<serde_json::Value>,
    authentication: Vec<AuthenticationMethod>,
    connection: Option<AcpConnection>,
    endpoint: Option<McpEndpoint>,
    events_tx: mpsc::Sender<AcpEvent>,
    events_rx: mpsc::Receiver<AcpEvent>,
    operations_rx: GuiOperationReceiver,
    pending_permission: Option<PendingPermission>,
    pending_confirmation: Option<PendingHostConfirmation>,
    clear_on_session_ready: bool,
    transcript: Transcript,
    prompt: String,
    stderr: Vec<String>,
}

impl AiController {
    pub fn new(_ctx: &egui::Context) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("oxiprep-ai")
            .build()
            .expect("AI runtime");
        let store = ProfileStore::platform().ok();
        let (preferences, profile_warning) =
            store.as_ref().map(ProfileStore::load).unwrap_or_else(|| {
                (
                    ProfilePreferences::default(),
                    Some("The agent profile directory is unavailable.".to_owned()),
                )
            });
        let selected_profile = preferences
            .effective_profiles()
            .first()
            .map(|profile| profile.id.clone());
        let (events_tx, events_rx) = mpsc::channel(crate::ai::acp::ACP_CHANNEL_CAPACITY);
        let (_operations_tx, operations_rx) = gui_operation_channel();
        let application_dir = std::env::current_dir()
            .ok()
            .and_then(|path| path.canonicalize().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        Self {
            runtime,
            store,
            preferences,
            selected_profile,
            profile_warning,
            application_dir,
            connected_cwd: None,
            mcp_endpoint: None,
            state: ConnectionState::Disconnected,
            agent_name: None,
            agent_version: None,
            agent_capabilities: None,
            authentication: Vec::new(),
            connection: None,
            endpoint: None,
            events_tx,
            events_rx,
            operations_rx,
            pending_permission: None,
            pending_confirmation: None,
            clear_on_session_ready: false,
            transcript: Transcript::default(),
            prompt: String::new(),
            stderr: Vec::new(),
        }
    }

    pub fn profiles(&self) -> Vec<AgentProfile> {
        self.preferences.effective_profiles()
    }

    pub fn selected_profile_id(&self) -> Option<&str> {
        self.selected_profile.as_deref()
    }

    pub fn select_profile(&mut self, id: impl Into<String>) {
        if !self.state.connected() && !matches!(self.state, ConnectionState::Starting) {
            self.selected_profile = Some(id.into());
        }
    }

    pub fn save_profile(&mut self, profile: AgentProfile) -> Result<(), String> {
        self.preferences
            .upsert(profile.clone())
            .map_err(|e| e.to_string())?;
        self.selected_profile = Some(profile.id);
        self.persist()
    }

    pub fn delete_profile(&mut self, id: &str) -> Result<(), String> {
        self.preferences.delete(id);
        self.selected_profile = self.profiles().first().map(|profile| profile.id.clone());
        self.persist()
    }

    pub fn reset_codex(&mut self) -> Result<(), String> {
        self.preferences.reset_codex();
        self.selected_profile = Some(CODEX_PROFILE_ID.to_owned());
        self.persist()
    }

    fn persist(&mut self) -> Result<(), String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "The agent profile directory is unavailable.".to_owned())?;
        store.save(&self.preferences).map_err(|e| e.to_string())
    }

    pub fn connect(
        &mut self,
        ctx: &egui::Context,
        project_path: Option<&Path>,
    ) -> Result<(), String> {
        self.disconnect();
        let id = self
            .selected_profile
            .clone()
            .ok_or_else(|| "Select an agent profile.".to_owned())?;
        let profile = self
            .profiles()
            .into_iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| "The selected agent profile no longer exists.".to_owned())?;
        let resolved = profile
            .resolved(project_path, &self.application_dir)
            .map_err(|e| e.to_string())?;
        self.connected_cwd = Some(resolved.working_directory.clone());
        let (operations_tx, operations_rx) = gui_operation_channel();
        self.operations_rx = operations_rx;
        let repaint_ctx = ctx.clone();
        let endpoint = McpEndpoint::start(self.runtime.handle(), operations_tx, move || {
            repaint_ctx.request_repaint()
        })
        .map_err(|e| format!("Could not start the Oxiprep MCP bridge: {e}"))?;
        let injection = endpoint
            .injection()
            .map_err(|e| format!("Could not configure the Oxiprep MCP proxy: {e}"))?;
        self.mcp_endpoint = Some(endpoint.address().to_string());
        let repaint_ctx = ctx.clone();
        let connection = AcpConnection::spawn(
            self.runtime.handle(),
            resolved,
            injection,
            self.events_tx.clone(),
            move || repaint_ctx.request_repaint(),
        );
        self.endpoint = Some(endpoint);
        self.connection = Some(connection);
        self.state = ConnectionState::Starting;
        self.agent_name = None;
        self.agent_version = None;
        self.agent_capabilities = None;
        self.authentication.clear();
        Ok(())
    }

    pub fn disconnect(&mut self) {
        if let Some(permission) = self.pending_permission.take() {
            let _ = permission.response.send(None);
        }
        if let Some(confirmation) = self.pending_confirmation.take() {
            let _ = confirmation.call.response.send(Err(OperationError::Failed(
                "The agent disconnected before confirmation.".to_owned(),
            )));
        }
        if let Some(connection) = self.connection.take() {
            connection.shutdown(self.runtime.handle());
        }
        self.endpoint.take();
        self.connected_cwd = None;
        self.mcp_endpoint = None;
        self.state = ConnectionState::Disconnected;
    }

    pub fn send_prompt(&mut self, session: &Session, viewport: &Viewport) -> Result<(), String> {
        let text = self.prompt.trim().to_owned();
        if text.is_empty() {
            return Err("Enter a prompt.".to_owned());
        }
        if self.state != ConnectionState::Ready {
            return Err("The agent is not ready.".to_owned());
        }
        self.connection
            .as_ref()
            .ok_or_else(|| "The agent is disconnected.".to_owned())?
            .try_send(AcpCommand::Prompt(
                crate::ai::acp::AcpPrompt::with_gui_snapshot(
                    text.clone(),
                    agent_prompt_snapshot(session, viewport),
                ),
            ))?;
        self.transcript.push_user(text);
        self.prompt.clear();
        Ok(())
    }

    pub fn cancel_prompt(&mut self) -> Result<(), String> {
        self.connection
            .as_ref()
            .ok_or_else(|| "The agent is disconnected.".to_owned())?
            .try_send(AcpCommand::Cancel)
    }

    pub fn new_conversation(&mut self) -> Result<(), String> {
        if self.state != ConnectionState::Ready {
            return Err("The agent is not ready.".to_owned());
        }
        self.connection
            .as_ref()
            .ok_or_else(|| "The agent is disconnected.".to_owned())?
            .try_send(AcpCommand::NewConversation)?;
        self.clear_on_session_ready = true;
        self.state = ConnectionState::Starting;
        Ok(())
    }

    pub fn authenticate(&self, method_id: &str) -> Result<(), String> {
        self.connection
            .as_ref()
            .ok_or_else(|| "The agent is disconnected.".to_owned())?
            .try_send(AcpCommand::Authenticate(method_id.to_owned()))
    }

    pub fn resolve_permission(&mut self, option_id: Option<String>) {
        if let Some(permission) = self.pending_permission.take() {
            let valid = option_id
                .as_ref()
                .is_none_or(|id| permission.options.iter().any(|option| option.id == *id));
            let _ = permission
                .response
                .send(if valid { option_id } else { None });
        }
    }

    pub fn resolve_confirmation(
        &mut self,
        approve: bool,
        session: &mut Session,
        viewport: &mut Viewport,
        console: &mut Vec<String>,
    ) {
        let Some(pending) = self.pending_confirmation.take() else {
            return;
        };
        let result = if approve {
            dispatch(
                &pending.call.request,
                session,
                viewport,
                HostApproval::Approved,
            )
        } else {
            Err(OperationError::Rejected)
        };
        log_result(&result, console);
        record_mcp_result(&mut self.transcript, &pending.call, &result);
        let _ = pending.call.response.send(result);
    }

    pub fn update(
        &mut self,
        session: &mut Session,
        viewport: &mut Viewport,
        console: &mut Vec<String>,
    ) {
        for _ in 0..EVENTS_PER_FRAME {
            let Ok(event) = self.events_rx.try_recv() else {
                break;
            };
            match event {
                AcpEvent::Initialized {
                    agent_name,
                    agent_version,
                    capabilities,
                    authentication,
                } => {
                    self.agent_name = Some(agent_name);
                    self.agent_version = agent_version;
                    self.agent_capabilities = Some(capabilities);
                    self.authentication = authentication;
                }
                AcpEvent::AuthenticationRequired => {
                    self.state = ConnectionState::AuthenticationRequired
                }
                AcpEvent::SessionReady => {
                    if self.clear_on_session_ready {
                        self.transcript.clear();
                        self.clear_on_session_ready = false;
                    }
                    self.state = ConnectionState::Ready;
                }
                AcpEvent::PromptStarted => self.state = ConnectionState::PromptActive,
                AcpEvent::SessionUpdate(update) => self.transcript.apply_update(*update),
                AcpEvent::PromptFinished(reason) => {
                    self.transcript.push_status("Prompt finished", reason);
                    self.state = ConnectionState::Ready;
                }
                AcpEvent::Permission(permission) => {
                    if self.pending_permission.is_none() {
                        self.pending_permission = Some(permission);
                    } else {
                        let _ = permission.response.send(None);
                    }
                }
                AcpEvent::Stderr(line) => {
                    if self.stderr.len() == 100 {
                        self.stderr.remove(0);
                    }
                    self.stderr.push(line);
                }
                AcpEvent::Error(error) => {
                    self.transcript.push_warning(error.clone());
                    self.state = ConnectionState::Failed(error);
                }
                AcpEvent::Disconnected => {
                    self.endpoint.take();
                    self.connection.take();
                    if !matches!(self.state, ConnectionState::Failed(_)) {
                        self.state = ConnectionState::Disconnected;
                    }
                }
            }
        }

        if self
            .pending_confirmation
            .as_ref()
            .is_some_and(|pending| pending.started.elapsed() >= CONFIRMATION_TIMEOUT)
            && let Some(pending) = self.pending_confirmation.take()
        {
            let _ = pending.call.response.send(Err(OperationError::Failed(
                "Host confirmation timed out.".to_owned(),
            )));
        }
        if self.pending_confirmation.is_none() {
            for _ in 0..OPERATIONS_PER_FRAME {
                let Ok(call) = self.operations_rx.try_recv() else {
                    break;
                };
                match confirmation_for(&call.request, session) {
                    Ok(Some(detail)) => {
                        self.pending_confirmation = Some(PendingHostConfirmation {
                            operation: call.request.id.clone(),
                            detail,
                            call,
                            started: Instant::now(),
                        });
                        break;
                    }
                    Ok(None) => {
                        let result =
                            dispatch(&call.request, session, viewport, HostApproval::NotRequired);
                        log_result(&result, console);
                        record_mcp_result(&mut self.transcript, &call, &result);
                        let _ = call.response.send(result);
                    }
                    Err(error) => {
                        let _ = call.response.send(Err(error));
                    }
                }
            }
        }
    }

    pub fn state(&self) -> &ConnectionState {
        &self.state
    }
    pub fn agent_name(&self) -> Option<&str> {
        self.agent_name.as_deref()
    }
    pub fn agent_version(&self) -> Option<&str> {
        self.agent_version.as_deref()
    }
    pub fn authentication(&self) -> &[AuthenticationMethod] {
        &self.authentication
    }
    pub fn agent_capabilities(&self) -> Option<&serde_json::Value> {
        self.agent_capabilities.as_ref()
    }
    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }
    pub fn prompt_mut(&mut self) -> &mut String {
        &mut self.prompt
    }
    pub fn pending_permission(&self) -> Option<&PendingPermission> {
        self.pending_permission.as_ref()
    }
    pub fn pending_confirmation(&self) -> Option<&PendingHostConfirmation> {
        self.pending_confirmation.as_ref()
    }
    pub fn profile_warning(&self) -> Option<&str> {
        self.profile_warning.as_deref()
    }
    pub fn stderr(&self) -> &[String] {
        &self.stderr
    }
    pub fn connected_cwd(&self) -> Option<&Path> {
        self.connected_cwd.as_deref()
    }
    pub fn mcp_endpoint(&self) -> Option<&str> {
        self.mcp_endpoint.as_deref()
    }
    pub fn mcp_ready(&self) -> bool {
        self.endpoint.as_ref().is_some_and(McpEndpoint::is_ready)
    }
}

impl Drop for AiController {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn log_result(
    result: &Result<crate::app_operation::AppOperationResult, OperationError>,
    console: &mut Vec<String>,
) {
    match result {
        Ok(result) => {
            if let Some(message) = &result.message {
                console.push(message.clone());
            }
        }
        Err(error) => console.push(error.to_string()),
    }
}

fn record_mcp_result(
    transcript: &mut Transcript,
    call: &GuiOperationCall,
    result: &Result<crate::app_operation::AppOperationResult, OperationError>,
) {
    let value = match result {
        Ok(result) => serde_json::to_value(result)
            .unwrap_or_else(|error| serde_json::json!({"message": error.to_string()})),
        Err(error) => serde_json::json!({"message": error.to_string()}),
    };
    transcript.apply_mcp_result(
        call.tool_call_id.as_deref(),
        &call.request.id,
        value,
        result.is_err(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::acp::PermissionChoice;
    use crate::ai::mcp::{GuiOperationSender, gui_operation_channel};
    use crate::app_operation::{AppOperationRequest, HostApproval};
    use agent_client_protocol::schema::v1::{
        ContentBlock, ContentChunk, MessageId, SessionUpdate, TextContent,
    };
    use serde_json::json;
    use tokio::sync::oneshot;

    fn controller_with_operations() -> (AiController, GuiOperationSender) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (events_tx, events_rx) = mpsc::channel(crate::ai::acp::ACP_CHANNEL_CAPACITY);
        let (operations_tx, operations_rx) = gui_operation_channel();
        (
            AiController {
                runtime,
                store: None,
                preferences: ProfilePreferences::default(),
                selected_profile: Some(CODEX_PROFILE_ID.to_owned()),
                profile_warning: None,
                application_dir: std::env::current_dir().unwrap(),
                connected_cwd: None,
                mcp_endpoint: None,
                state: ConnectionState::Disconnected,
                agent_name: None,
                agent_version: None,
                agent_capabilities: None,
                authentication: Vec::new(),
                connection: None,
                endpoint: None,
                events_tx,
                events_rx,
                operations_rx,
                pending_permission: None,
                pending_confirmation: None,
                clear_on_session_ready: false,
                transcript: Transcript::default(),
                prompt: String::new(),
                stderr: Vec::new(),
            },
            operations_tx,
        )
    }

    fn enqueue(
        sender: &GuiOperationSender,
        request: AppOperationRequest,
    ) -> oneshot::Receiver<Result<crate::app_operation::AppOperationResult, OperationError>> {
        let (response, receiver) = oneshot::channel();
        sender
            .try_send(GuiOperationCall {
                request,
                tool_call_id: None,
                response,
            })
            .unwrap();
        receiver
    }

    #[test]
    fn gui_queue_serializes_mutations_in_arrival_order_and_records_results() {
        let (mut controller, sender) = controller_with_operations();
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let mut console = Vec::new();
        let mut first = enqueue(
            &sender,
            AppOperationRequest::new("geometry.create", json!({"kind": "point"})),
        );
        let mut second = enqueue(
            &sender,
            AppOperationRequest::new("geometry.create", json!({"kind": "sphere"})),
        );

        controller.update(&mut session, &mut viewport, &mut console);

        assert_eq!(first.try_recv().unwrap().unwrap().revision, 1);
        assert_eq!(second.try_recv().unwrap().unwrap().revision, 2);
        assert_eq!(session.document.models.len(), 2);
        assert_eq!(console, ["Created Point.", "Created Sphere."]);
        assert_eq!(controller.transcript.items().len(), 2);
    }

    #[test]
    fn host_confirmation_approves_rejects_times_out_and_never_duplicates() {
        let (mut controller, sender) = controller_with_operations();
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let mut console = Vec::new();
        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "point"})),
            &mut session,
            &mut viewport,
            HostApproval::NotRequired,
        )
        .unwrap();

        let mut rejected = enqueue(&sender, AppOperationRequest::new("project.new", json!({})));
        controller.update(&mut session, &mut viewport, &mut console);
        assert!(controller.pending_confirmation().is_some());
        controller.update(&mut session, &mut viewport, &mut console);
        assert!(matches!(
            rejected.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        controller.resolve_confirmation(false, &mut session, &mut viewport, &mut console);
        assert!(matches!(
            rejected.try_recv().unwrap(),
            Err(OperationError::Rejected)
        ));
        assert_eq!(session.document.models.len(), 1);

        let mut approved = enqueue(&sender, AppOperationRequest::new("project.new", json!({})));
        controller.update(&mut session, &mut viewport, &mut console);
        controller.resolve_confirmation(true, &mut session, &mut viewport, &mut console);
        assert_eq!(approved.try_recv().unwrap().unwrap().revision, 2);
        assert!(session.document.models.is_empty());

        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "point"})),
            &mut session,
            &mut viewport,
            HostApproval::NotRequired,
        )
        .unwrap();
        let mut timed_out = enqueue(&sender, AppOperationRequest::new("project.new", json!({})));
        controller.update(&mut session, &mut viewport, &mut console);
        controller.pending_confirmation.as_mut().unwrap().started =
            Instant::now() - CONFIRMATION_TIMEOUT;
        controller.update(&mut session, &mut viewport, &mut console);
        assert!(
            matches!(timed_out.try_recv().unwrap(), Err(OperationError::Failed(message)) if message.contains("timed out"))
        );
        assert_eq!(session.document.models.len(), 1);
    }

    #[test]
    fn disconnect_cancels_host_confirmation_and_permission_without_approval() {
        let (mut controller, sender) = controller_with_operations();
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let mut console = Vec::new();
        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "point"})),
            &mut session,
            &mut viewport,
            HostApproval::NotRequired,
        )
        .unwrap();
        let mut confirmation = enqueue(&sender, AppOperationRequest::new("project.new", json!({})));
        controller.update(&mut session, &mut viewport, &mut console);

        let (permission_response, mut permission_result) = oneshot::channel();
        controller.pending_permission = Some(PendingPermission {
            tool_call_id: "call-1".to_owned(),
            title: Some("Permission".to_owned()),
            options: vec![PermissionChoice {
                id: "allow".to_owned(),
                name: "Allow once".to_owned(),
                kind: "allow_once".to_owned(),
            }],
            response: permission_response,
        });
        assert!(matches!(
            permission_result.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        controller.disconnect();

        assert!(
            matches!(confirmation.try_recv().unwrap(), Err(OperationError::Failed(message)) if message.contains("disconnected"))
        );
        assert_eq!(permission_result.try_recv().unwrap(), None);
        assert_eq!(session.document.models.len(), 1);
    }

    #[test]
    fn permission_returns_only_an_exact_agent_option_once() {
        let (mut controller, _sender) = controller_with_operations();
        let (response, mut result) = oneshot::channel();
        controller.pending_permission = Some(PendingPermission {
            tool_call_id: "call-2".to_owned(),
            title: None,
            options: vec![PermissionChoice {
                id: "reject-once".to_owned(),
                name: "Reject once".to_owned(),
                kind: "reject_once".to_owned(),
            }],
            response,
        });
        controller.resolve_permission(Some("reject-once".to_owned()));
        controller.resolve_permission(Some("reject-once".to_owned()));
        assert_eq!(result.try_recv().unwrap(), Some("reject-once".to_owned()));

        let (response, mut result) = oneshot::channel();
        controller.pending_permission = Some(PendingPermission {
            tool_call_id: "call-3".to_owned(),
            title: None,
            options: vec![],
            response,
        });
        controller.resolve_permission(Some("invented".to_owned()));
        assert_eq!(result.try_recv().unwrap(), None);
    }

    #[test]
    fn conversation_is_replaced_only_after_success_and_duplicate_prompt_is_blocked() {
        let (mut controller, _sender) = controller_with_operations();
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let mut console = Vec::new();
        controller.transcript.push_user("keep me");
        controller.clear_on_session_ready = true;
        controller.state = ConnectionState::Starting;
        controller
            .events_tx
            .try_send(AcpEvent::Error("new session failed".to_owned()))
            .unwrap();
        controller.update(&mut session, &mut viewport, &mut console);
        assert!(
            controller
                .transcript
                .items()
                .iter()
                .any(|item| item.text == "keep me")
        );

        controller
            .events_tx
            .try_send(AcpEvent::SessionReady)
            .unwrap();
        controller.update(&mut session, &mut viewport, &mut console);
        assert!(controller.transcript.items().is_empty());

        controller.state = ConnectionState::PromptActive;
        controller.prompt = "duplicate".to_owned();
        assert!(
            controller
                .send_prompt(&session, &viewport)
                .unwrap_err()
                .contains("not ready")
        );
        assert!(controller.transcript.items().is_empty());
    }

    #[test]
    fn connection_states_have_factual_labels_and_capability_gates() {
        let states = [
            (ConnectionState::Disconnected, "Disconnected", false),
            (ConnectionState::Starting, "Connecting", false),
            (
                ConnectionState::AuthenticationRequired,
                "Authentication required",
                true,
            ),
            (ConnectionState::Ready, "Ready", true),
            (ConnectionState::PromptActive, "Working", true),
            (
                ConnectionState::Failed("failure".to_owned()),
                "Disconnected",
                false,
            ),
        ];
        for (state, label, connected) in states {
            assert_eq!(state.label(), label);
            assert_eq!(state.connected(), connected);
        }
    }

    #[test]
    fn streamed_events_and_tools_are_bounded_per_frame_and_both_make_progress() {
        let (mut controller, sender) = controller_with_operations();
        let (events_tx, events_rx) = mpsc::channel(EVENTS_PER_FRAME + 2);
        controller.events_tx = events_tx;
        controller.events_rx = events_rx;
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let mut console = Vec::new();
        for index in 0..=EVENTS_PER_FRAME {
            controller
                .events_tx
                .try_send(AcpEvent::SessionUpdate(Box::new(
                    SessionUpdate::AgentMessageChunk(
                        ContentChunk::new(ContentBlock::Text(TextContent::new("chunk")))
                            .message_id(MessageId::new(format!("message-{index}"))),
                    ),
                )))
                .unwrap();
        }
        let mut operation = enqueue(
            &sender,
            AppOperationRequest::new("geometry.create", json!({"kind": "box"})),
        );

        controller.update(&mut session, &mut viewport, &mut console);

        assert_eq!(controller.transcript.items().len(), EVENTS_PER_FRAME + 1);
        assert_eq!(controller.events_rx.len(), 1);
        assert_eq!(operation.try_recv().unwrap().unwrap().revision, 1);
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(console, ["Created Box."]);

        controller.update(&mut session, &mut viewport, &mut console);
        assert_eq!(controller.transcript.items().len(), EVENTS_PER_FRAME + 2);
        assert!(controller.events_rx.is_empty());
    }
}
