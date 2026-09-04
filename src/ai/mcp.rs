//! Session-scoped Oxiprep MCP service and stdio proxy.

use crate::app_operation::{
    AppOperationRequest, AppOperationResult, EffectClass, OperationError, agent_operation_specs,
};
use rand::RngCore;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorData as McpError, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer, ServiceExt};
use rmcp::{ServerHandler, transport::async_rw::AsyncRwTransport};
use serde_json::{Map, Value, json};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf,
};
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub const MCP_PROXY_MODE: &str = "--oxiprep-mcp-proxy";
pub const MCP_TOKEN_ENV: &str = "OXIPREP_MCP_TOKEN";
pub const MAX_MCP_FRAME: usize = 1024 * 1024;
pub const MAX_MCP_CLIENTS: usize = 4;
pub const GUI_QUEUE_CAPACITY: usize = 32;
pub const MCP_CALL_TIMEOUT: Duration = Duration::from_secs(120);

const MCP_INSTRUCTIONS: &str = "These tools operate the live state of the currently running Oxiprep desktop application. References to the current or open model, object, selection, or view mean Oxiprep GUI state: call context.get before targeted operations and pass its document revision with target references. Use these Oxiprep tools instead of searching the filesystem or using computer-use tools. mesh.generate operates CAD solid bodies; omit size to use Oxiprep's normal target-based default.";
const TOOL_DESCRIPTION_PREFIX: &str =
    "Operate the live state of the currently running Oxiprep desktop application. ";

pub struct GuiOperationCall {
    pub request: AppOperationRequest,
    pub tool_call_id: Option<String>,
    pub response: oneshot::Sender<Result<AppOperationResult, OperationError>>,
}

pub type GuiOperationSender = mpsc::SyncSender<GuiOperationCall>;
pub type GuiOperationReceiver = mpsc::Receiver<GuiOperationCall>;

pub fn gui_operation_channel() -> (GuiOperationSender, GuiOperationReceiver) {
    mpsc::sync_channel(GUI_QUEUE_CAPACITY)
}

#[derive(Clone)]
struct OxiprepMcpService {
    operations: GuiOperationSender,
    repaint: Arc<dyn Fn() + Send + Sync>,
}

impl OxiprepMcpService {
    fn tools() -> Vec<Tool> {
        agent_operation_specs()
            .into_iter()
            .map(|spec| {
                let schema = spec
                    .parameter_schema
                    .as_object()
                    .cloned()
                    .unwrap_or_else(Map::new);
                Tool::new(
                    spec.id.to_owned(),
                    format!("{TOOL_DESCRIPTION_PREFIX}{}", spec.description),
                    schema,
                )
                .with_title(spec.title)
                .with_annotations(
                    ToolAnnotations::new()
                        .read_only(spec.effect == EffectClass::Query)
                        .destructive(matches!(
                            spec.effect,
                            EffectClass::UndoableMutation | EffectClass::ProjectMutation
                        ))
                        .open_world(matches!(spec.effect, EffectClass::ProjectMutation)),
                )
            })
            .collect()
    }
}

impl ServerHandler for OxiprepMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("oxiprep", env!("CARGO_PKG_VERSION")))
            .with_instructions(MCP_INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: Self::tools(),
            ..Default::default()
        }))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        Self::tools().into_iter().find(|tool| tool.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if self.get_tool(request.name.as_ref()).is_none() {
            return Err(McpError::method_not_found::<
                rmcp::model::CallToolRequestMethod,
            >());
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let tool_call_id = request.meta.as_ref().and_then(|meta| {
            ["toolCallId", "tool_call_id", "acp/toolCallId"]
                .into_iter()
                .find_map(|key| meta.get(key).and_then(Value::as_str).map(str::to_owned))
        });
        let operation = AppOperationRequest::new(request.name.into_owned(), arguments);
        let (tx, rx) = oneshot::channel();
        if self
            .operations
            .try_send(GuiOperationCall {
                request: operation,
                tool_call_id,
                response: tx,
            })
            .is_err()
        {
            return Ok(CallToolResult::structured_error(json!({
                "code": "busy",
                "message": "Oxiprep's application-operation queue is full."
            }))
            .into());
        }
        (self.repaint)();
        let result = match tokio::time::timeout(MCP_CALL_TIMEOUT, rx).await {
            Ok(Ok(Ok(result))) => {
                CallToolResult::structured(serde_json::to_value(result).unwrap_or_else(
                    |error| json!({"status": "error", "message": error.to_string()}),
                ))
            }
            Ok(Ok(Err(error))) => CallToolResult::structured_error(operation_error(&error)),
            Ok(Err(_)) => CallToolResult::structured_error(json!({
                "code": "disconnected",
                "message": "Oxiprep disconnected before completing the operation."
            })),
            Err(_) => CallToolResult::structured_error(json!({
                "code": "timeout",
                "message": "The Oxiprep operation timed out."
            })),
        };
        Ok(result.into())
    }
}

fn operation_error(error: &OperationError) -> Value {
    let code = match error {
        OperationError::UnknownOperation(_) => "unsupported",
        OperationError::InvalidArguments(_) => "invalid_arguments",
        OperationError::StaleRevision { .. } => "stale_revision",
        OperationError::MissingTarget(_) => "missing_target",
        OperationError::ConfirmationRequired { .. } => "confirmation_required",
        OperationError::Rejected => "rejected",
        OperationError::Failed(_) => "operation_failed",
    };
    json!({"code": code, "message": error.to_string()})
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpInjection {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
}

pub struct McpEndpoint {
    address: SocketAddr,
    token: String,
    cancel: CancellationToken,
    task: JoinHandle<()>,
    initialized_clients: Arc<AtomicUsize>,
}

impl McpEndpoint {
    pub fn start(
        handle: &tokio::runtime::Handle,
        operations: GuiOperationSender,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let listener = StdTcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let mut bytes = [0_u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = hex(&bytes);
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let listener_token = token.clone();
        let service = OxiprepMcpService {
            operations,
            repaint: Arc::new(repaint),
        };
        let initialized_clients = Arc::new(AtomicUsize::new(0));
        let listener_initialized_clients = initialized_clients.clone();
        let task = handle.spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            let permits = Arc::new(Semaphore::new(MAX_MCP_CLIENTS));
            loop {
                let accepted = tokio::select! {
                    _ = task_cancel.cancelled() => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((stream, _)) = accepted else { continue };
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    continue;
                };
                let service = service.clone();
                let expected = token_for_task(&listener_token);
                let connection_cancel = task_cancel.child_token();
                let initialized_clients = listener_initialized_clients.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if authenticate(
                        stream,
                        &expected,
                        connection_cancel,
                        service,
                        initialized_clients,
                    )
                    .await
                    .is_err()
                    {
                        // Authentication and transport failures deliberately close the socket.
                    }
                });
            }
        });
        Ok(Self {
            address,
            token,
            cancel,
            task,
            initialized_clients,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn is_ready(&self) -> bool {
        self.initialized_clients.load(Ordering::Acquire) > 0
    }

    pub fn injection(&self) -> io::Result<McpInjection> {
        Ok(McpInjection {
            name: "oxiprep".to_owned(),
            command: std::env::current_exe()?,
            args: vec![MCP_PROXY_MODE.to_owned(), self.address.to_string()],
            environment: vec![(MCP_TOKEN_ENV.to_owned(), self.token.clone())],
        })
    }

    pub fn revoke(&self) {
        self.cancel.cancel();
        self.task.abort();
    }
}

impl Drop for McpEndpoint {
    fn drop(&mut self) {
        self.revoke();
    }
}

async fn authenticate(
    mut stream: tokio::net::TcpStream,
    expected: &str,
    cancel: CancellationToken,
    service: OxiprepMcpService,
    initialized_clients: Arc<AtomicUsize>,
) -> io::Result<()> {
    let mut token = Vec::with_capacity(expected.len());
    loop {
        if token.len() > 128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "authentication token is too long",
            ));
        }
        let mut byte = [0_u8; 1];
        let count = stream.read(&mut byte).await?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "missing authentication token",
            ));
        }
        if byte[0] == b'\n' {
            break;
        }
        if byte[0] != b'\r' {
            token.push(byte[0]);
        }
    }
    if token != expected.as_bytes() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid authentication token",
        ));
    }
    let (read, write) = stream.into_split();
    let transport = AsyncRwTransport::<RoleServer, _, _>::new_server(
        FrameLimitedRead::new(read, MAX_MCP_FRAME),
        write,
    );
    let repaint = service.repaint.clone();
    let running = service
        .serve_with_ct(transport, cancel)
        .await
        .map_err(|error| io::Error::new(io::ErrorKind::ConnectionAborted, error.to_string()))?;
    initialized_clients.fetch_add(1, Ordering::AcqRel);
    repaint();
    let result = running
        .waiting()
        .await
        .map_err(|error| io::Error::other(error.to_string()));
    initialized_clients.fetch_sub(1, Ordering::AcqRel);
    repaint();
    result.map(|_| ())
}

struct FrameLimitedRead<R> {
    inner: R,
    frame_length: usize,
    maximum: usize,
}

impl<R> FrameLimitedRead<R> {
    fn new(inner: R, maximum: usize) -> Self {
        Self {
            inner,
            frame_length: 0,
            maximum,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for FrameLimitedRead<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let remaining_in_frame = self.maximum.saturating_sub(self.frame_length);
        let capacity = output.remaining().min(remaining_in_frame.saturating_add(1));
        let mut storage = vec![0_u8; capacity];
        let mut temporary = ReadBuf::new(&mut storage);
        match Pin::new(&mut self.inner).poll_read(context, &mut temporary) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {
                let bytes = temporary.filled();
                for &byte in bytes {
                    if byte == b'\n' {
                        self.frame_length = 0;
                    } else {
                        self.frame_length += 1;
                        if self.frame_length > self.maximum {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "MCP message exceeds the frame limit",
                            )));
                        }
                    }
                }
                output.put_slice(bytes);
                Poll::Ready(Ok(()))
            }
        }
    }
}

fn token_for_task(token: &str) -> String {
    token.to_owned()
}

pub fn maybe_run_proxy_mode() -> Option<Result<(), String>> {
    let mut args = std::env::args_os();
    let _program = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new(MCP_PROXY_MODE)) {
        return None;
    }
    let address = match args.next().and_then(|value| value.into_string().ok()) {
        Some(value) => match value.parse::<SocketAddr>() {
            Ok(value) if value.ip().is_loopback() => value,
            _ => {
                return Some(Err(
                    "The MCP proxy endpoint is not a valid loopback address.".to_owned(),
                ));
            }
        },
        None => return Some(Err("The MCP proxy endpoint is missing.".to_owned())),
    };
    if args.next().is_some() {
        return Some(Err("Unexpected MCP proxy arguments.".to_owned()));
    }
    let token = match std::env::var(MCP_TOKEN_ENV) {
        Ok(token) if !token.is_empty() => token,
        _ => {
            return Some(Err(
                "The MCP proxy authentication token is missing.".to_owned()
            ));
        }
    };
    Some(
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                runtime
                    .block_on(run_stdio_proxy(address, &token))
                    .map_err(|error| error.to_string())
            }),
    )
}

pub async fn run_stdio_proxy(address: SocketAddr, token: &str) -> io::Result<()> {
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "MCP endpoint must be loopback",
        ));
    }
    let mut stream = tokio::net::TcpStream::connect(address).await?;
    stream.write_all(token.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;
    let (socket_read, socket_write) = stream.into_split();
    let to_server = forward_lines(tokio::io::stdin(), socket_write);
    let to_client = forward_lines(socket_read, tokio::io::stdout());
    tokio::pin!(to_server);
    tokio::pin!(to_client);
    tokio::select! {
        result = &mut to_server => result,
        result = &mut to_client => result,
    }
}

async fn forward_lines<R, W>(read: R, mut write: W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut read = BufReader::new(read);
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = read.read_until(b'\n', &mut line).await?;
        if count == 0 {
            return Ok(());
        }
        if line.len() > MAX_MCP_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP message exceeds the frame limit",
            ));
        }
        write.write_all(&line).await?;
        write.flush().await?;
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_operation::{HostApproval, dispatch};
    use crate::session::Session;
    use crate::viewport::Viewport;
    use cadrum::{DVec3, Solid};
    use rmcp::ServiceExt;
    use rmcp::model::CallToolRequestParams;
    use std::collections::BTreeSet;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn server_and_tool_instructions_ground_agents_in_live_oxiprep_state() {
        let (tx, _rx) = gui_operation_channel();
        let service = OxiprepMcpService {
            operations: tx,
            repaint: Arc::new(|| {}),
        };
        let instructions = service.get_info().instructions.unwrap();
        assert!(instructions.contains("currently running Oxiprep desktop application"));
        assert!(instructions.contains("current or open model"));
        assert!(instructions.contains("context.get"));
        assert!(instructions.contains("instead of searching the filesystem"));
        assert!(instructions.contains("omit size"));
        assert!(OxiprepMcpService::tools().iter().all(|tool| {
            tool.description.as_deref().is_some_and(|description| {
                description.contains("currently running Oxiprep desktop application")
            })
        }));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn official_mcp_client_lists_live_tools_and_calls_context() {
        let (tx, rx) = gui_operation_channel();
        let repaints = Arc::new(AtomicUsize::new(0));
        let repaint_count = repaints.clone();
        let service = OxiprepMcpService {
            operations: tx,
            repaint: Arc::new(move || {
                repaint_count.fetch_add(1, Ordering::Relaxed);
            }),
        };
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let running = service.serve(server_io).await.unwrap();
            running.waiting().await.unwrap()
        });
        let client = ().serve(client_io).await.unwrap();
        let tools = client.list_tools(Default::default()).await.unwrap();
        assert!(tools.tools.iter().any(|tool| tool.name == "context.get"));
        assert!(
            tools
                .tools
                .iter()
                .all(|tool| tool.input_schema.get("type") == Some(&json!("object")))
        );

        let peer = client.peer().clone();
        let call = tokio::spawn(async move {
            peer.call_tool(CallToolRequestParams::new("context.get"))
                .await
        });
        let pending = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let result = dispatch(
            &pending.request,
            &mut session,
            &mut viewport,
            HostApproval::NotRequired,
        );
        pending.response.send(result).unwrap();
        let result = call.await.unwrap().unwrap();
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content.unwrap()["revision"], 0);

        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "box"})),
            &mut session,
            &mut viewport,
            HostApproval::NotRequired,
        )
        .unwrap();
        let peer = client.peer().clone();
        let call = tokio::spawn(async move {
            peer.call_tool(CallToolRequestParams::new("context.get"))
                .await
        });
        let pending = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let result = dispatch(
            &pending.request,
            &mut session,
            &mut viewport,
            HostApproval::NotRequired,
        );
        pending.response.send(result).unwrap();
        let result = call.await.unwrap().unwrap();
        assert_eq!(result.structured_content.unwrap()["revision"], 1);
        assert_eq!(repaints.load(Ordering::Relaxed), 2);
        client.cancel().await.unwrap();
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn end_to_end_agent_operations_match_the_gui_path() {
        let (tx, rx) = gui_operation_channel();
        let service = OxiprepMcpService {
            operations: tx,
            repaint: Arc::new(|| {}),
        };
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let running = service.serve(server_io).await.unwrap();
            running.waiting().await.unwrap()
        });
        let client = ().serve(client_io).await.unwrap();
        let mut agent_session = Session::new();
        let mut agent_viewport = Viewport::new(None);
        let mut agent_console = Vec::new();
        let mut gui_session = Session::new();
        let mut gui_viewport = Viewport::new(None);
        let mut gui_console = Vec::new();

        macro_rules! call_and_compare {
            ($request:expr) => {{
                let request = $request;
                let arguments = request.arguments.as_object().unwrap().clone();
                let peer = client.peer().clone();
                let tool_name = request.id.clone();
                let call = tokio::spawn(async move {
                    peer.call_tool(CallToolRequestParams::new(tool_name).with_arguments(arguments))
                        .await
                });
                let pending = rx.recv_timeout(Duration::from_secs(1)).unwrap();
                assert_eq!(pending.request, request);
                let agent_result = dispatch(
                    &pending.request,
                    &mut agent_session,
                    &mut agent_viewport,
                    HostApproval::NotRequired,
                )
                .unwrap();
                if let Some(message) = &agent_result.message {
                    agent_console.push(message.clone());
                }
                let expected = serde_json::to_value(&agent_result).unwrap();
                pending.response.send(Ok(agent_result)).unwrap();
                let response = call.await.unwrap().unwrap();
                assert_eq!(response.is_error, Some(false));
                assert_eq!(response.structured_content.as_ref(), Some(&expected));

                let gui_result = dispatch(
                    &request,
                    &mut gui_session,
                    &mut gui_viewport,
                    HostApproval::NotRequired,
                )
                .unwrap();
                if let Some(message) = &gui_result.message {
                    gui_console.push(message.clone());
                }
                assert_eq!(serde_json::to_value(gui_result).unwrap(), expected);
                response
            }};
        }

        let initial = call_and_compare!(AppOperationRequest::new("context.get", json!({})));
        assert_eq!(initial.structured_content.unwrap()["data"]["revision"], 0);
        call_and_compare!(AppOperationRequest::new(
            "geometry.create",
            json!({"kind": "box"}),
        ));
        call_and_compare!(AppOperationRequest::new(
            "mesh.generate",
            json!({
                "revision": 1,
                "targets": [{"kind": "model", "model": 0}],
                "kind": "surface",
                "size": 0.5
            }),
        ));
        call_and_compare!(AppOperationRequest::new(
            "view.standard",
            json!({"direction": "+x"}),
        ));
        call_and_compare!(AppOperationRequest::new(
            "document.delete",
            json!({"revision": 2, "targets": [{"kind": "model", "model": 0}]}),
        ));
        call_and_compare!(AppOperationRequest::new("history.undo", json!({})));
        let final_context = call_and_compare!(AppOperationRequest::new("context.get", json!({})))
            .structured_content
            .unwrap();

        assert_eq!(agent_console, gui_console);
        assert_eq!(
            agent_console,
            [
                "Created Box.",
                "Meshed 1 body.",
                "Changed view.",
                "Deleted Box.",
                "Undo Delete Box.",
            ]
        );
        assert!(agent_session.document.dirty);
        assert_eq!(agent_session.revision(), 4);
        assert_eq!(agent_session.document.models.len(), 1);
        assert!(agent_session.document.models[0].bodies[0].has_discrete_mesh());
        assert_eq!(
            agent_session.document.selection,
            gui_session.document.selection
        );
        assert_eq!(agent_session.undo_label(), Some("Mesh surface"));
        assert_eq!(agent_session.redo_label(), Some("Delete Box"));
        assert_eq!(agent_viewport.camera.yaw, gui_viewport.camera.yaw);
        assert_eq!(agent_viewport.camera.pitch, gui_viewport.camera.pitch);
        assert_eq!(final_context["data"]["revision"], 4);
        assert_eq!(final_context["data"]["dirty"], true);
        assert_eq!(
            final_context["data"]["models"][0]["bodies"][0]["has_mesh"],
            true
        );
        assert_eq!(final_context["data"]["selection"][0]["kind"], "model");
        assert_eq!(final_context["data"]["history"]["can_undo"], true);
        assert_eq!(final_context["data"]["history"]["can_redo"], true);

        client.cancel().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn endpoint_rejects_invalid_token_and_revokes() {
        let (tx, _rx) = gui_operation_channel();
        let endpoint = McpEndpoint::start(&tokio::runtime::Handle::current(), tx, || {}).unwrap();
        let address = endpoint.address();
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(b"wrong\n").await.unwrap();
        let mut byte = [0_u8; 1];
        assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
        endpoint.revoke();
        tokio::task::yield_now().await;
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn forwarding_rejects_oversized_frames() {
        let (mut input, read) = tokio::io::duplex(MAX_MCP_FRAME + 32);
        let (write, mut output) = tokio::io::duplex(MAX_MCP_FRAME + 32);
        let task = tokio::spawn(forward_lines(read, write));
        input
            .write_all(&vec![b'x'; MAX_MCP_FRAME + 1])
            .await
            .unwrap();
        input.write_all(b"\n").await.unwrap();
        drop(input);
        assert_eq!(
            task.await.unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut data = Vec::new();
        output.read_to_end(&mut data).await.unwrap();
        assert!(data.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn endpoint_accepts_valid_token_and_concurrent_clients() {
        let (tx, _rx) = gui_operation_channel();
        let endpoint = McpEndpoint::start(&tokio::runtime::Handle::current(), tx, || {}).unwrap();
        assert!(!endpoint.is_ready());
        let address = endpoint.address();
        let token = endpoint.token.clone();
        let clients = (0..MAX_MCP_CLIENTS).map(|_| {
            let token = token.clone();
            async move {
                let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
                stream.write_all(token.as_bytes()).await.unwrap();
                stream.write_all(b"\n").await.unwrap();
                ().serve(stream).await.unwrap()
            }
        });
        let clients = futures::future::join_all(clients).await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while endpoint.initialized_clients.load(Ordering::Acquire) != MAX_MCP_CLIENTS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("all authenticated MCP clients should become ready");
        assert!(endpoint.is_ready());
        for client in &clients {
            let tools = client.list_tools(Default::default()).await.unwrap();
            assert!(tools.tools.iter().any(|tool| tool.name == "context.get"));
        }
        for (index, client) in clients.into_iter().enumerate() {
            client.cancel().await.unwrap();
            let remaining = MAX_MCP_CLIENTS - index - 1;
            tokio::time::timeout(Duration::from_secs(2), async {
                while endpoint.initialized_clients.load(Ordering::Acquire) != remaining {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("MCP readiness count should follow client disconnects");
            assert_eq!(endpoint.is_ready(), remaining > 0);
        }
        endpoint.revoke();
        tokio::task::yield_now().await;
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn endpoint_rejects_oversized_direct_messages_before_dispatch() {
        let (tx, rx) = gui_operation_channel();
        let endpoint = McpEndpoint::start(&tokio::runtime::Handle::current(), tx, || {}).unwrap();
        let mut stream = tokio::net::TcpStream::connect(endpoint.address())
            .await
            .unwrap();
        stream.write_all(endpoint.token.as_bytes()).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        let oversized = vec![b'x'; MAX_MCP_FRAME + 1];
        let _ = stream.write_all(&oversized).await;
        let _ = stream.write_all(b"\n").await;
        let mut byte = [0_u8; 1];
        let closed = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
            .await
            .expect("endpoint should close an oversized client");
        assert!(matches!(closed, Err(_) | Ok(0)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn every_advertised_mcp_operation_accepts_its_valid_typed_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let project_path = directory.path().join("project.oxiprep");
        let import_path = directory.path().join("import.step");
        let mut file = std::fs::File::create(&import_path).unwrap();
        Solid::write_step(
            std::iter::once(&Solid::cube(DVec3::ZERO, DVec3::ONE)),
            &mut file,
        )
        .unwrap();
        file.flush().unwrap();

        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let mut called = BTreeSet::new();
        macro_rules! call {
            ($request:expr, $approval:expr $(,)?) => {{
                let request = $request;
                called.insert(request.id.clone());
                dispatch(&request, &mut session, &mut viewport, $approval).unwrap()
            }};
        }
        call!(
            AppOperationRequest::new("context.get", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("operations.list", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("project.new", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("geometry.create", json!({"kind": "box"})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("view.fit_all", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("view.fit_selection", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("view.standard", json!({"direction": "+z"})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("view.display", json!({"mesh": true})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new(
                "view.clip",
                json!({"enabled": true, "axis": "x", "position": 0.25, "flip": true}),
            ),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("project.save_as", json!({"path": project_path})),
            HostApproval::Approved,
        );
        call!(
            AppOperationRequest::new("project.save", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("document.import", json!({"path": import_path})),
            HostApproval::NotRequired,
        );
        let revision = session.revision();
        call!(
            AppOperationRequest::new(
                "mesh.generate",
                json!({"revision": revision, "targets": [{"kind": "model", "model": 1}], "kind": "surface", "size": 0.5}),
            ),
            HostApproval::NotRequired,
        );
        let revision = session.revision();
        call!(
            AppOperationRequest::new("document.close", json!({"revision": revision, "model": 1})),
            HostApproval::NotRequired,
        );
        let revision = session.revision();
        call!(
            AppOperationRequest::new(
                "document.delete",
                json!({"revision": revision, "targets": [{"kind": "model", "model": 0}]}),
            ),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("history.undo", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("history.redo", json!({})),
            HostApproval::NotRequired,
        );
        call!(
            AppOperationRequest::new("project.open", json!({"path": project_path})),
            HostApproval::Approved,
        );

        let advertised = agent_operation_specs()
            .into_iter()
            .map(|spec| spec.id.to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(called, advertised);
    }
}
