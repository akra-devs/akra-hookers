use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn serve_prints_an_opaque_uuid_capability_token() {
    let data_dir = TempDir::new().expect("data directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(data_dir.path())
        .stdout(Stdio::piped())
        .spawn()
        .expect("serve starts");
    let stdout = child.stdout.take().expect("stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("ready line");
    child.kill().expect("serve stops");
    child.wait().expect("serve reaps");

    let token = line
        .split("token=")
        .nth(1)
        .expect("capability token")
        .trim();
    Uuid::parse_str(token.strip_prefix("akra-").expect("akra prefix"))
        .expect("cryptographically opaque UUID token");
}
