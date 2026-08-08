use std::fs;

use akra_app::spool::Spool;
use tempfile::TempDir;

#[test]
fn spool_round_trips_a_pending_payload() {
    let directory = TempDir::new().expect("temp directory");
    let spool = Spool::open(directory.path()).expect("spool opens");
    spool
        .enqueue(br#"{"prompt":"recover me"}"#)
        .expect("payload spools");

    assert_eq!(
        spool.drain().expect("payload drains"),
        vec![br#"{"prompt":"recover me"}"#.to_vec()]
    );
}

#[test]
fn reading_pending_payload_does_not_acknowledge_it() {
    let directory = TempDir::new().expect("temp directory");
    let spool = Spool::open(directory.path()).expect("spool opens");
    spool
        .enqueue(br#"{"prompt":"retain me"}"#)
        .expect("payload spools");

    let payloads = spool.drain().expect("payload reads");
    assert_eq!(payloads, vec![br#"{"prompt":"retain me"}"#.to_vec()]);
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("spool directory")
            .filter_map(Result::ok)
            .count(),
        1,
        "a payload is only removed after durable storage acknowledges it"
    );
}
