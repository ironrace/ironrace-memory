//! Integration tests for the preference-enrichment ingest pass.

use ironmem::mcp::app::App;
use ironmem::mcp::protocol::JsonRpcRequest;
use ironmem::mcp::server::dispatch;
use serde_json::{json, Value};

fn request(method: &str, params: Value) -> JsonRpcRequest {
    serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    }))
    .expect("request fixture must deserialize")
}

fn call(app: &App, tool: &str, args: Value) -> Value {
    let req = request("tools/call", json!({ "name": tool, "arguments": args }));
    let resp = dispatch(app, &req).expect("tools/call must return a response");
    assert!(
        resp.error.is_none(),
        "unexpected RPC error calling {tool}: {:?}",
        resp.error
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"]
        .as_str()
        .expect("content[0].text must be a string");
    serde_json::from_str(text).expect("tool response must be valid JSON")
}

#[test]
fn deleting_parent_cascades_to_synthetic_sibling() {
    // Insert one parent + one synthetic sibling pointing at it directly via
    // the DB layer (so this test doesn't depend on Task 6's enrichment wiring).
    let app = App::open_for_test().expect("build test app");
    let parent_id = "a".repeat(32);
    let synth_id = "b".repeat(32);
    let zero_vec: Vec<f32> = vec![0.0; 384];

    app.db
        .insert_drawer(&parent_id, "parent", &zero_vec, "w", "r", "", "test")
        .unwrap();
    app.db
        .insert_drawer(
            &synth_id,
            "User has mentioned: thing",
            &zero_vec,
            "w",
            "r",
            &format!("pref:{parent_id}"),
            "test",
        )
        .unwrap();

    let deleted = call(&app, "delete_drawer", json!({ "id": parent_id }));
    assert_eq!(deleted["success"], true);

    // Synthetic sibling must be gone too.
    let got = app.db.get_drawer(&synth_id).unwrap();
    assert!(got.is_none(), "synthetic sibling should cascade-delete");
}
