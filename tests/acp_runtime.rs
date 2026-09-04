use oxiprep::ai::acp::{ACP_FIXTURE_MODE, AcpCommand, AcpConnection, AcpEvent, AcpPrompt};
use oxiprep::ai::mcp::{GuiOperationReceiver, McpEndpoint, gui_operation_channel};
use oxiprep::ai::profile::ResolvedProfile;
use oxiprep::app_operation::{HostApproval, dispatch};
use oxiprep::session::Session;
use oxiprep::viewport::Viewport;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

fn fixture_profile(directory: &Path, record: &Path, mode: &str) -> ResolvedProfile {
    let mut environment = BTreeMap::from([
        (
            "OXIPREP_ACP_FIXTURE_RECORD".to_owned(),
            record.display().to_string(),
        ),
        ("OXIPREP_ACP_FIXTURE_MODE".to_owned(), mode.to_owned()),
        ("OXIPREP_FIXTURE_VALUE".to_owned(), "a b".to_owned()),
    ]);
    if mode == "normal" {
        environment.insert("OXIPREP_ACP_FIXTURE_CALL_MCP".to_owned(), "1".to_owned());
    }
    ResolvedProfile {
        id: "fixture".to_owned(),
        name: "Fixture".to_owned(),
        command: env!("CARGO_BIN_EXE_oxiprep").to_owned(),
        args: vec![
            ACP_FIXTURE_MODE.to_owned(),
            "two words".to_owned(),
            "*.rs".to_owned(),
        ],
        environment,
        working_directory: directory.to_path_buf(),
    }
}

fn records(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn recv_event(
    events: &mut mpsc::Receiver<AcpEvent>,
    operations: &GuiOperationReceiver,
    session: &mut Session,
    viewport: &mut Viewport,
) -> AcpEvent {
    recv_event_with_timeout(
        events,
        operations,
        session,
        viewport,
        Duration::from_secs(5),
    )
    .await
}

async fn recv_event_with_timeout(
    events: &mut mpsc::Receiver<AcpEvent>,
    operations: &GuiOperationReceiver,
    session: &mut Session,
    viewport: &mut Viewport,
    timeout: Duration,
) -> AcpEvent {
    tokio::time::timeout(timeout, async {
        loop {
            while let Ok(call) = operations.try_recv() {
                let result = dispatch(&call.request, session, viewport, HostApproval::NotRequired);
                let _ = call.response.send(result);
            }
            if let Ok(event) = events.try_recv() {
                return event;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("ACP event timed out")
}

fn app_injection(endpoint: &McpEndpoint) -> oxiprep::ai::mcp::McpInjection {
    let mut injection = endpoint.injection().unwrap();
    injection.command = PathBuf::from(env!("CARGO_BIN_EXE_oxiprep"));
    injection
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixture_verifies_spawn_initialize_injection_stream_and_new_conversation() {
    let directory = tempfile::tempdir().unwrap();
    let record = directory.path().join("events.jsonl");
    let (operation_tx, operation_rx) = gui_operation_channel();
    let endpoint =
        McpEndpoint::start(&tokio::runtime::Handle::current(), operation_tx, || {}).unwrap();
    let injection = app_injection(&endpoint);
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let connection = AcpConnection::spawn(
        &tokio::runtime::Handle::current(),
        fixture_profile(directory.path(), &record, "normal"),
        injection,
        event_tx,
        || {},
    );
    let mut session = Session::new();
    let mut viewport = Viewport::new(None);

    loop {
        if matches!(
            recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
            AcpEvent::SessionReady
        ) {
            break;
        }
    }
    let values = records(&record);
    let process = values
        .iter()
        .find(|value| value["event"] == "process")
        .unwrap();
    assert_eq!(
        process["value"]["args"],
        serde_json::json!(["two words", "*.rs"])
    );
    assert_eq!(process["value"]["environment"], "a b");
    assert_eq!(
        process["value"]["pwd"],
        directory.path().display().to_string()
    );
    let initialize = values
        .iter()
        .find(|value| value["event"] == "initialize")
        .unwrap();
    assert_eq!(initialize["value"]["protocolVersion"], 1);
    assert_eq!(
        initialize["value"]["clientCapabilities"]["fs"]["readTextFile"],
        false
    );
    assert_eq!(
        initialize["value"]["clientCapabilities"]["fs"]["writeTextFile"],
        false
    );
    assert_eq!(initialize["value"]["clientCapabilities"]["terminal"], false);
    let new_session = values
        .iter()
        .find(|value| value["event"] == "session/new")
        .unwrap();
    assert_eq!(
        new_session["value"]["cwd"],
        directory.path().display().to_string()
    );
    assert_eq!(new_session["value"]["mcpServers"][0]["name"], "oxiprep");
    assert_eq!(
        new_session["value"]["mcpServers"][0]["command"],
        env!("CARGO_BIN_EXE_oxiprep")
    );
    assert!(
        values
            .iter()
            .any(|value| value["event"] == "mcp/context.get")
    );

    connection
        .try_send(AcpCommand::Prompt(AcpPrompt::new("hello")))
        .unwrap();
    let mut streamed = false;
    loop {
        match recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await {
            AcpEvent::SessionUpdate(_) => streamed = true,
            AcpEvent::PromptFinished(reason) => {
                assert!(reason.contains("EndTurn"));
                break;
            }
            _ => {}
        }
    }
    assert!(streamed);
    let prompt = records(&record)
        .into_iter()
        .find(|value| value["event"] == "session/prompt")
        .unwrap();
    let prompt_blocks = prompt["value"]["prompt"].as_array().unwrap();
    assert_eq!(prompt_blocks.len(), 2);
    assert!(
        prompt_blocks[0]["text"]
            .as_str()
            .unwrap()
            .contains("tools may be deferred")
    );
    assert_eq!(prompt_blocks[1]["text"], "hello");
    connection.try_send(AcpCommand::NewConversation).unwrap();
    loop {
        if matches!(
            recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
            AcpEvent::SessionReady
        ) {
            break;
        }
    }
    assert_eq!(
        records(&record)
            .iter()
            .filter(|value| value["event"] == "session/new")
            .count(),
        2
    );
    connection.shutdown(&tokio::runtime::Handle::current());
    drop(endpoint);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authentication_and_unsupported_protocol_are_handled() {
    let directory = tempfile::tempdir().unwrap();
    for mode in ["auth", "auth_fail", "unsupported"] {
        let record = directory.path().join(format!("{mode}.jsonl"));
        let (operation_tx, operation_rx) = gui_operation_channel();
        let endpoint =
            McpEndpoint::start(&tokio::runtime::Handle::current(), operation_tx, || {}).unwrap();
        let (event_tx, mut event_rx) = mpsc::channel(64);
        let connection = AcpConnection::spawn(
            &tokio::runtime::Handle::current(),
            fixture_profile(directory.path(), &record, mode),
            app_injection(&endpoint),
            event_tx,
            || {},
        );
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        if mode.starts_with("auth") {
            loop {
                if matches!(
                    recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
                    AcpEvent::AuthenticationRequired
                ) {
                    break;
                }
            }
            if mode == "auth" {
                connection
                    .try_send(AcpCommand::Authenticate("unavailable".to_owned()))
                    .unwrap();
                loop {
                    if matches!(recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await, AcpEvent::Error(error) if error.contains("unavailable"))
                    {
                        break;
                    }
                }
            }
            connection
                .try_send(AcpCommand::Authenticate("fixture-auth".to_owned()))
                .unwrap();
            loop {
                match recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await {
                    AcpEvent::SessionReady if mode == "auth" => break,
                    AcpEvent::Error(error)
                        if mode == "auth_fail" && error.contains("Authentication failed") =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        } else {
            let mut saw_error = false;
            loop {
                match recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await {
                    AcpEvent::Error(error) => {
                        assert!(error.contains("Unsupported ACP protocol version"));
                        saw_error = true;
                    }
                    AcpEvent::Disconnected if saw_error => break,
                    _ => {}
                }
            }
        }
        connection.shutdown(&tokio::runtime::Handle::current());
        drop(endpoint);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_disconnect_and_missing_executable_clean_up() {
    let directory = tempfile::tempdir().unwrap();
    let record = directory.path().join("cancel.jsonl");
    let (operation_tx, operation_rx) = gui_operation_channel();
    let endpoint =
        McpEndpoint::start(&tokio::runtime::Handle::current(), operation_tx, || {}).unwrap();
    let address = endpoint.address();
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let connection = AcpConnection::spawn(
        &tokio::runtime::Handle::current(),
        fixture_profile(directory.path(), &record, "hang"),
        app_injection(&endpoint),
        event_tx,
        || {},
    );
    let mut session = Session::new();
    let mut viewport = Viewport::new(None);
    loop {
        if matches!(
            recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
            AcpEvent::SessionReady
        ) {
            break;
        }
    }
    connection
        .try_send(AcpCommand::Prompt(AcpPrompt::new("wait")))
        .unwrap();
    loop {
        if matches!(
            recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
            AcpEvent::SessionUpdate(_)
        ) {
            break;
        }
    }
    connection.try_send(AcpCommand::Cancel).unwrap();
    loop {
        if matches!(recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await, AcpEvent::PromptFinished(reason) if reason.contains("Cancelled"))
        {
            break;
        }
    }
    connection.shutdown(&tokio::runtime::Handle::current());
    drop(endpoint);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(tokio::net::TcpStream::connect(address).await.is_err());

    let (event_tx, mut event_rx) = mpsc::channel(8);
    let missing = ResolvedProfile {
        id: "missing".to_owned(),
        name: "Missing".to_owned(),
        command: directory
            .path()
            .join("does-not-exist")
            .display()
            .to_string(),
        args: vec![],
        environment: BTreeMap::from([("API_TOKEN".to_owned(), "do-not-leak".to_owned())]),
        working_directory: directory.path().to_path_buf(),
    };
    let connection = AcpConnection::spawn(
        &tokio::runtime::Handle::current(),
        missing,
        oxiprep::ai::mcp::McpInjection {
            name: "oxiprep".to_owned(),
            command: PathBuf::from(env!("CARGO_BIN_EXE_oxiprep")),
            args: vec![],
            environment: vec![],
        },
        event_tx,
        || {},
    );
    let error = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(AcpEvent::Error(error)) = event_rx.recv().await {
                return error;
            }
        }
    })
    .await
    .unwrap();
    assert!(!error.contains("do-not-leak"));
    connection.shutdown(&tokio::runtime::Handle::current());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_stderr_crash_and_hung_shutdown_are_contained() {
    let directory = tempfile::tempdir().unwrap();
    for mode in ["malformed", "stderr", "crash", "hang"] {
        let record = directory.path().join(format!("failure-{mode}.jsonl"));
        let (operation_tx, operation_rx) = gui_operation_channel();
        let endpoint =
            McpEndpoint::start(&tokio::runtime::Handle::current(), operation_tx, || {}).unwrap();
        let (event_tx, mut event_rx) = mpsc::channel(64);
        let connection = AcpConnection::spawn(
            &tokio::runtime::Handle::current(),
            fixture_profile(directory.path(), &record, mode),
            app_injection(&endpoint),
            event_tx,
            || {},
        );
        let mut session = Session::new();
        let mut viewport = Viewport::new(None);
        let mut saw_stderr = false;
        loop {
            match recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await {
                AcpEvent::Stderr(line) => {
                    saw_stderr = true;
                    assert!(line.chars().count() <= oxiprep::ai::acp::MAX_STDERR_EVENT_CHARS + 30);
                }
                AcpEvent::SessionReady => break,
                _ => {}
            }
        }
        if mode == "stderr" {
            assert!(saw_stderr);
        }
        if mode == "crash" {
            connection
                .try_send(AcpCommand::Prompt(AcpPrompt::new("crash")))
                .unwrap();
            loop {
                if matches!(
                    recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
                    AcpEvent::Error(_) | AcpEvent::Disconnected
                ) {
                    break;
                }
            }
        } else if mode == "hang" {
            connection
                .try_send(AcpCommand::Prompt(AcpPrompt::new("hang")))
                .unwrap();
            loop {
                if matches!(
                    recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
                    AcpEvent::SessionUpdate(_)
                ) {
                    break;
                }
            }
            connection.shutdown(&tokio::runtime::Handle::current());
            loop {
                if matches!(
                    recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
                    AcpEvent::Disconnected
                ) {
                    break;
                }
            }
            drop(endpoint);
            continue;
        }
        connection.shutdown(&tokio::runtime::Handle::current());
        drop(endpoint);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_permission_waits_for_and_returns_the_exact_option() {
    let directory = tempfile::tempdir().unwrap();
    let record = directory.path().join("permission.jsonl");
    let (operation_tx, operation_rx) = gui_operation_channel();
    let endpoint =
        McpEndpoint::start(&tokio::runtime::Handle::current(), operation_tx, || {}).unwrap();
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let connection = AcpConnection::spawn(
        &tokio::runtime::Handle::current(),
        fixture_profile(directory.path(), &record, "permission"),
        app_injection(&endpoint),
        event_tx,
        || {},
    );
    let mut session = Session::new();
    let mut viewport = Viewport::new(None);
    loop {
        if matches!(
            recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
            AcpEvent::SessionReady
        ) {
            break;
        }
    }
    connection
        .try_send(AcpCommand::Prompt(AcpPrompt::new("permission")))
        .unwrap();
    loop {
        if let AcpEvent::Permission(permission) =
            recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await
        {
            assert_eq!(permission.tool_call_id, "fixture-tool");
            assert_eq!(permission.options[0].id, "allow-once");
            assert_eq!(permission.options[1].id, "reject-once");
            permission
                .response
                .send(Some("allow-once".to_owned()))
                .unwrap();
            break;
        }
    }
    loop {
        if matches!(
            recv_event(&mut event_rx, &operation_rx, &mut session, &mut viewport).await,
            AcpEvent::PromptFinished(_)
        ) {
            break;
        }
    }
    let permission = records(&record)
        .into_iter()
        .find(|value| value["event"] == "permission/result")
        .unwrap();
    assert_eq!(permission["value"]["outcome"]["optionId"], "allow-once");
    connection.shutdown(&tokio::runtime::Handle::current());
    drop(endpoint);
}
