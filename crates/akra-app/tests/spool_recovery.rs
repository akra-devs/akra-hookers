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
