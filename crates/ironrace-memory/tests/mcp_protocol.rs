//! Integration tests for the MCP JSON-RPC protocol layer.
//!
//! These tests call `dispatch` directly with an in-memory App (noop embedder,
//! no ONNX model required) and assert on the JSON-RPC response shape.

use ironrace_memory::mcp::app::App;
use ironrace_memory::mcp::protocol::JsonRpcRequest;
use ironrace_memory::mcp::server::dispatch;
use serde_json::json;

fn request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    }))
    .expect("request fixture must deserialize")
}

fn call_tool(app: &App, name: &str, args: serde_json::Value) -> serde_json::Value {
    let req = request("tools/call", json!({ "name": name, "arguments": args }));
    let resp = dispatch(app, &req).expect("tools/call must return a response");
    assert!(resp.error.is_none(), "unexpected RPC error for tool {name}");
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"]
        .as_str()
        .expect("content[0].text must be a string");
    serde_json::from_str(text).expect("tool response text must be valid JSON")
}

#[test]
fn initialize_returns_capabilities() {
    let app = App::open_for_test().unwrap();
    let req = request("initialize", json!({}));
    let resp = dispatch(&app, &req).expect("initialize must return a response");

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert!(result["capabilities"]["tools"].is_object());
    assert_eq!(result["serverInfo"]["name"], "ironrace-memory");
}

#[test]
fn tools_list_contains_required_tools() {
    let app = App::open_for_test().unwrap();
    let req = request("tools/list", json!({}));
    let resp = dispatch(&app, &req).expect("tools/list must return a response");

    assert!(resp.error.is_none());
    let tools = resp.result.unwrap()["tools"]
        .as_array()
        .cloned()
        .expect("result.tools must be an array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for required in &[
        "ironmem_status",
        "ironmem_search",
        "ironmem_list_wings",
        "ironmem_kg_stats",
        "ironmem_add_drawer",
        "ironmem_diary_write",
    ] {
        assert!(
            names.contains(required),
            "missing required tool: {required}"
        );
    }
}

#[test]
fn tools_list_read_only_mode_excludes_write_tools() {
    use ironrace_memory::config::McpAccessMode;

    let app = App::open_for_test_with_mode(McpAccessMode::ReadOnly).unwrap();
    let req = request("tools/list", json!({}));
    let resp = dispatch(&app, &req).unwrap();

    let tools = resp.result.unwrap()["tools"].as_array().cloned().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for blocked in &[
        "ironmem_add_drawer",
        "ironmem_delete_drawer",
        "ironmem_diary_write",
    ] {
        assert!(
            !names.contains(blocked),
            "write tool should be absent in read-only mode: {blocked}"
        );
    }
    // Read tools still present
    assert!(names.contains(&"ironmem_status"));
    assert!(names.contains(&"ironmem_search"));
}

#[test]
fn ironmem_status_returns_expected_shape() {
    let app = App::open_for_test().unwrap();
    let status = call_tool(&app, "ironmem_status", json!({}));

    assert!(
        status["total_drawers"].is_number(),
        "total_drawers must be a number"
    );
    assert!(status["wings"].is_object(), "wings must be an object");
    assert!(
        status["knowledge_graph"].is_object(),
        "knowledge_graph must be an object"
    );
    let protocol = status["memory_protocol"].as_str().unwrap_or("");
    assert!(
        !protocol.is_empty(),
        "memory_protocol must be a non-empty string"
    );
}

#[test]
fn unknown_method_returns_method_not_found() {
    let app = App::open_for_test().unwrap();
    let req = request("nonexistent/method", json!({}));
    let resp = dispatch(&app, &req).expect("unknown method must return a response");

    let err = resp.error.expect("unknown method must return an error");
    assert_eq!(err.code, -32601);
}

#[test]
fn kg_add_and_query_round_trip() {
    let app = App::open_for_test().unwrap();

    // Add a triple
    let add = call_tool(
        &app,
        "ironmem_kg_add",
        json!({ "subject": "rust", "predicate": "is-a", "object": "language" }),
    );
    assert_eq!(add["success"], true);

    // Query it back
    let query = call_tool(&app, "ironmem_kg_query", json!({ "entity": "rust" }));
    let triples = query["triples"]
        .as_array()
        .expect("triples must be an array");
    assert!(
        !triples.is_empty(),
        "query should return the inserted triple"
    );
}
