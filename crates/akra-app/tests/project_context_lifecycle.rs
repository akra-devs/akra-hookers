use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use akra_app::spool::Spool;
use serde_json::{Value, json};
use tempfile::TempDir;

#[tokio::test]
async fn captured_envelope_recovers_into_authenticated_project_context_apis() {
    let data_dir = TempDir::new().expect("data directory");
    let home = TempDir::new().expect("home directory");
    let project_dir = data_dir.path().join("recovered-project");
    std::fs::create_dir(&project_dir).expect("project directory");
    let cwd = project_dir.to_string_lossy().into_owned();
    let prompt = "recover this immutable prompt";

    capture(data_dir.path(), &cwd, prompt);
    assert!(
        !data_dir.path().join("akra-hookers.sqlite").exists(),
        "capture while the daemon is unavailable must not open SQLite"
    );
    let spool = Spool::open(&data_dir.path().join("spool")).expect("spool");
    let pending = spool.pending().expect("pending envelope");
    assert_eq!(pending.len(), 1, "capture must durably spool one envelope");
    let bytes = spool.read(&pending[0]).expect("pending payload");
    let envelope: Value = serde_json::from_slice(&bytes).expect("v1 envelope");
    assert_eq!(envelope["schema_version"], 1);
    assert_eq!(envelope["provider"], "codex");
    assert_eq!(envelope["payload"]["prompt"], prompt);
    assert_eq!(envelope["origin"]["display_path"], cwd);
    assert_eq!(envelope["origin"]["kind"], "directory");
    let captured_at_us = envelope["captured_at_us"].as_i64().expect("capture time");
    assert!(captured_at_us > 0);

    let server = start_server(data_dir.path(), home.path());
    assert!(
        data_dir.path().join("akra-hookers.sqlite").exists(),
        "serve must recover into its temporary database before reporting ready"
    );
    assert!(
        Spool::open(&data_dir.path().join("spool"))
            .expect("spool")
            .pending()
            .expect("pending spool")
            .is_empty(),
        "successful recovery must acknowledge the envelope"
    );

    assert_eq!(api(&server, "GET", "/v1/projects", None, false).0, 401);
    let (status, projects) = api(&server, "GET", "/v1/projects", None, true);
    assert_eq!(status, 200);
    assert_eq!(projects.as_array().expect("projects").len(), 1);
    let project_id = projects[0]["id"].as_i64().expect("project id");
    assert_eq!(projects[0]["name"], "recovered-project");

    let (status, origins) = api(&server, "GET", "/v1/origins", None, true);
    assert_eq!(status, 200);
    let origin_id = origins[0]["id"].as_i64().expect("origin id");
    assert_eq!(origins[0]["display_path"], cwd);
    assert_eq!(origins[0]["kind"], "directory");
    assert_eq!(origins[0]["resolution_source"], "captured");
    assert_eq!(origins[0]["routing_mode"], "dedicated");
    assert_eq!(origins[0]["setup_state"], "unconfirmed");
    assert_eq!(origins[0]["default_project_id"], project_id);

    let (status, activities) = api(&server, "GET", "/v1/activities?scope=all", None, true);
    assert_eq!(status, 200);
    let activity_id = activities[0]["id"].as_i64().expect("activity id");
    assert_eq!(activities[0]["prompt"], prompt);
    assert_eq!(activities[0]["project"]["id"], project_id);

    let detail_path = format!("/v1/activities/{activity_id}");
    let (status, detail) = api(&server, "GET", &detail_path, None, true);
    assert_eq!(status, 200);
    assert_eq!(detail["prompt"], prompt);
    assert_eq!(detail["submitted_cwd"], cwd);
    assert_eq!(detail["origin"]["resolution_source"], "captured");
    assert_eq!(detail["captured_at"]["provenance"], "captured");
    assert_eq!(detail["technical"]["session_id"], "recovered-session");
    let immutable = detail.clone();

    let shared = json!({"mode": "shared", "confirm": true});
    let (status, origin) = api(
        &server,
        "PATCH",
        &format!("/v1/origins/{origin_id}/routing"),
        Some(shared),
        true,
    );
    assert_eq!(status, 200);
    assert_eq!(origin["routing_mode"], "shared");
    assert!(origin["default_project_id"].is_null());
    let (_, shared_activities) = api(&server, "GET", "/v1/activities?scope=all", None, true);
    assert_eq!(shared_activities[0]["project"]["id"], project_id);

    let dedicated = json!({
        "mode": "dedicated",
        "destination": {"project_id": project_id},
        "confirm": true,
    });
    let (status, origin) = api(
        &server,
        "PATCH",
        &format!("/v1/origins/{origin_id}/routing"),
        Some(dedicated),
        true,
    );
    assert_eq!(status, 200);
    assert_eq!(origin["routing_mode"], "dedicated");
    assert_eq!(origin["default_project_id"], project_id);
    let (_, detail_after_routing) = api(&server, "GET", &detail_path, None, true);
    assert_eq!(
        detail_after_routing, immutable,
        "routing must not rewrite recovered activity content or provenance"
    );
}

fn capture(data_dir: &std::path::Path, cwd: &str, prompt: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("capture starts");
    let payload = json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "recovered-session",
        "turn_id": "recovered-turn",
        "cwd": cwd,
        "prompt": prompt,
        "model": "test",
    });
    child
        .stdin
        .as_mut()
        .expect("capture stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("capture payload writes");
    let output = child.wait_with_output().expect("capture exits");
    assert!(output.status.success(), "capture failed: {output:?}");
    assert!(output.stdout.is_empty(), "capture must remain silent");
}

struct Server {
    child: Child,
    address: String,
    token: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn start_server(data_dir: &std::path::Path, home: &std::path::Path) -> Server {
    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(data_dir)
        .args(["--home"])
        .arg(home)
        .stdout(Stdio::piped())
        .spawn()
        .expect("server starts");
    let stdout = child.stdout.take().expect("server stdout");
    let (ready_sender, ready_receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if line.starts_with("ready ") {
                let _ = ready_sender.send(line);
                break;
            }
        }
    });
    let ready = ready_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("server recovery readiness");
    let mut fields = ready.split_whitespace();
    assert_eq!(fields.next(), Some("ready"));
    let address = fields
        .next()
        .and_then(|field| field.strip_prefix("url=http://"))
        .expect("server address")
        .to_owned();
    let token = fields
        .next()
        .and_then(|field| field.strip_prefix("token="))
        .expect("server token")
        .to_owned();
    Server {
        child,
        address,
        token,
    }
}

fn api(
    server: &Server,
    method: &str,
    path: &str,
    body: Option<Value>,
    authorized: bool,
) -> (u16, Value) {
    let payload = body.map(|value| value.to_string()).unwrap_or_default();
    let authorization = if authorized {
        format!("Authorization: Bearer {}\r\n", server.token)
    } else {
        String::new()
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        server.address,
        payload.len(),
    );
    let mut stream = TcpStream::connect(&server.address).expect("server connection");
    stream
        .write_all(request.as_bytes())
        .expect("request writes");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("response reads");
    let (head, body) = response.split_at(
        response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("HTTP headers")
            + 4,
    );
    let status = std::str::from_utf8(head)
        .expect("HTTP header encoding")
        .split_whitespace()
        .nth(1)
        .expect("HTTP status")
        .parse()
        .expect("numeric HTTP status");
    let json = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(body).expect("JSON response")
    };
    (status, json)
}
