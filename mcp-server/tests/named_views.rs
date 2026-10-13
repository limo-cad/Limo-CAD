//! Exercise the shipped stdio protocol and native OCCT, without script playback.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

struct Client {
    child: Child,
    input: ChildStdin,
    output: Receiver<Value>,
    id: u64,
}

impl Client {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_limo-cad-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, output) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let reply = serde_json::from_str(&line.unwrap()).unwrap();
                if send.send(reply).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            id: 0,
        };
        let init = client.rpc(
            "initialize",
            json!({"protocolVersion":"2025-06-18",
            "capabilities":{},"clientInfo":{"name":"named-view-regression","version":"1"}}),
        );
        assert!(!init["instructions"].as_str().unwrap().trim().is_empty());
        let catalog = client.call("cad_list_all_tools", json!({}));
        let tools = catalog.as_array().unwrap();
        for name in [
            "cad_interface",
            "upsert_named_view",
            "rename_named_view",
            "delete_named_view",
            "recall_named_view",
            "clear_named_view",
        ] {
            assert!(
                tools.iter().any(|tool| tool["name"] == name),
                "MCP does not advertise {name}"
            );
        }
        client
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let reply = self
                .output
                .recv_timeout(Duration::from_secs(30))
                .expect("MCP exited or timed out before replying");
            if reply["id"] == self.id {
                assert!(reply.get("error").is_none(), "{reply}");
                return reply["result"].clone();
            }
        }
    }
    fn result(&mut self, name: &str, args: Value) -> Value {
        self.rpc("tools/call", json!({"name":name,"arguments":args}))
    }
    fn call(&mut self, name: &str, args: Value) -> Value {
        let result = self.result(name, args);
        assert_ne!(result["isError"], true, "{name}: {result}");
        let text = result["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "text")
            .unwrap()["text"]
            .as_str()
            .unwrap();
        let mut value: Value = serde_json::from_str(text).unwrap();
        if let Some(object) = value.as_object_mut() {
            object.remove("_disclosure");
        }
        value
    }
    fn appearance(&mut self, operation: &str, args: Value) -> Value {
        self.call("cad_interface", json!({"action":"execute","group":"document/appearance","operation":operation,"arguments":args}))
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn named_views_complete_stdio_workflow_without_scripts() {
    let mut cad = Client::new();
    cad.call("cad_new_project", json!({}));
    for name in ["Sketch1", "Sketch2"] {
        cad.call(
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        );
        cad.call(
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":10},"ctrl_held":false}),
        );
        cad.call("sketch_finish", json!({}));
        cad.call("solid_extrude", json!({"sketch_name":name,"profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":8},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}));
    }
    let scene = cad.call("solid_scene", json!({}));
    assert_eq!(scene["errors"], json!([]));
    let bodies = scene["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 2);
    let first = bodies[0]["id"].clone();
    let second = bodies[1]["id"].clone();
    let edited = cad.call("solid_edit_extrude", json!({"feature_id":bodies[0]["feature_id"],
        "extrude":{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":12},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}}));
    assert_eq!(edited["scene"]["errors"], json!([]));
    let scene = cad.call("solid_scene", json!({}));
    let view = json!({"name":"exploded","camera":{"position":[40.0,40.0,40.0],"target":[5.0,5.0,4.0],"up":[0.0,0.0,1.0]},
        "visible_body_ids":[first,first],"part_offsets":[{"body_id":first,"translation":[0.0,20.0,0.0]}]});
    let stored = cad.appearance("upsert_named_view", view.clone());
    let saved_view = stored["views"][0].clone();
    assert_eq!(saved_view["visible_body_ids"], json!([first]));
    let mut other = view.clone();
    other["name"] = json!("detail");
    other["visible_body_ids"] = json!([second]);
    cad.call("upsert_named_view", other.clone());
    assert_eq!(
        cad.appearance("named_views", json!({}))["views"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let recalled = cad.appearance("recall_named_view", json!({"name":"exploded"}));
    assert_eq!(recalled["view"], saved_view);
    assert_eq!(recalled["visibility"]["hidden_body_ids"], json!([second]));
    assert_eq!(cad.call("named_views", json!({}))["active"], "exploded");
    assert_eq!(
        cad.call("solid_scene", json!({})),
        scene,
        "Display offsets must not move native geometry"
    );
    let before = cad.call("cad_project_model", json!({}));
    let mut invalid = view.clone();
    invalid["visible_body_ids"] = json!([999999]);
    for (op, args) in [
        ("upsert_named_view", invalid),
        (
            "rename_named_view",
            json!({"name":"exploded","new_name":"detail"}),
        ),
        (
            "rename_named_view",
            json!({"name":"exploded","new_name":" "}),
        ),
        (
            "rename_named_view",
            json!({"name":"missing","new_name":"new"}),
        ),
        ("delete_named_view", json!({"name":"missing"})),
        ("recall_named_view", json!({"name":"missing"})),
    ] {
        assert_eq!(cad.result(op, args)["isError"], true, "{op} should fail");
        assert_eq!(
            cad.call("cad_project_model", json!({})),
            before,
            "{op} must be atomic"
        );
        assert_eq!(cad.call("named_views", json!({}))["active"], "exploded");
    }
    cad.appearance("clear_named_view", json!({}));
    assert!(cad.call("named_views", json!({}))["active"].is_null());
    assert_eq!(
        cad.call("project_visibility", json!({})),
        recalled["visibility"]
    );
    cad.appearance(
        "rename_named_view",
        json!({"name":"exploded","new_name":"review"}),
    );
    let mut updated = other.clone();
    updated["camera"]["position"] = json!([60.0, 40.0, 40.0]);
    let stored = cad.appearance("upsert_named_view", updated);
    let updated = stored["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["name"] == "detail")
        .unwrap()
        .clone();
    let saved = cad.call("cad_project_model", json!({}));
    cad.call("cad_new_project", json!({}));
    cad.call("cad_load_project_model", json!({"model_json":saved}));
    let listed = cad.call("named_views", json!({}));
    assert_eq!(listed["views"].as_array().unwrap().len(), 2);
    assert!(listed["views"].as_array().unwrap().contains(&updated));
    cad.appearance("recall_named_view", json!({"name":"review"}));
    cad.appearance("delete_named_view", json!({"name":"review"}));
    let listed = cad.call("named_views", json!({}));
    assert_eq!(listed["views"], json!([updated]));
    assert!(listed["active"].is_null());
    cad.call(
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"xy"}}),
    );
    let before_sketch_visibility = cad.appearance("project_visibility", json!({}));
    let before_sketch_views = cad.appearance("named_views", json!({}));
    assert_eq!(
        cad.result("recall_named_view", json!({"name":"detail"}))["isError"],
        true
    );
    assert_eq!(
        cad.appearance("project_visibility", json!({})),
        before_sketch_visibility
    );
    assert_eq!(
        cad.appearance("named_views", json!({})),
        before_sketch_views,
        "Rejecting recall during sketch editing must preserve visibility and view metadata"
    );
    cad.call("sketch_finish", json!({}));
    cad.appearance("set_named_views", json!({"views":[]}));
    assert_eq!(cad.call("named_views", json!({}))["views"], json!([]));
    assert_eq!(cad.call("solid_scene", json!({})), scene);
}
