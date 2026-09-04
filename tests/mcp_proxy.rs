use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::process::{Command, Stdio};

const TOKEN: &str = "integration-test-token";

fn round_trip(input: &mut impl Write, output: &mut impl BufRead, request: Value) -> Value {
    writeln!(input, "{request}").unwrap();
    input.flush().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn stdio_proxy_forwards_initialize_list_call_and_shuts_down() {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim_end(), TOKEN);

        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let request: Value = serde_json::from_str(&line).unwrap();
            let Some(id) = request.get("id").cloned() else {
                continue;
            };
            let result = match request["method"].as_str().unwrap() {
                "initialize" => json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "oxiprep-test", "version": "1"}
                }),
                "tools/list" => json!({
                    "tools": [{
                        "name": "context.get",
                        "description": "Live context",
                        "inputSchema": {"type": "object", "properties": {}}
                    }]
                }),
                "tools/call" => json!({
                    "content": [{"type": "text", "text": "revision 7"}],
                    "structuredContent": {"status": "ok", "revision": 7},
                    "isError": false
                }),
                method => panic!("unexpected MCP method: {method}"),
            };
            writeln!(
                stream,
                "{}",
                json!({"jsonrpc": "2.0", "id": id, "result": result})
            )
            .unwrap();
            stream.flush().unwrap();
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_oxiprep"))
        .arg("--oxiprep-mcp-proxy")
        .arg(address.to_string())
        .env("OXIPREP_MCP_TOKEN", TOKEN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());

    let initialized = round_trip(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "proxy-test", "version": "1"}}
        }),
    );
    assert_eq!(initialized["result"]["serverInfo"]["name"], "oxiprep-test");
    writeln!(
        input,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    input.flush().unwrap();
    let tools = round_trip(
        &mut input,
        &mut output,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    );
    assert_eq!(tools["result"]["tools"][0]["name"], "context.get");
    let called = round_trip(
        &mut input,
        &mut output,
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "context.get", "arguments": {}}}),
    );
    assert_eq!(called["result"]["structuredContent"]["revision"], 7);

    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "proxy failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap();
}
