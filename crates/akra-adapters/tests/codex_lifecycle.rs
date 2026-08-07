use std::fs;

use akra_adapters::codex::CodexHookLifecycle;
use tempfile::TempDir;

#[test]
fn enable_then_disable_manages_only_akra_hook_entry() {
    let home = TempDir::new().expect("temp home");
    let lifecycle = CodexHookLifecycle::new(home.path());

    lifecycle.enable("akra-hookers capture").expect("enable");
    assert!(lifecycle.is_enabled().expect("status"));

    lifecycle.disable().expect("disable");
    assert!(!lifecycle.is_enabled().expect("status"));
    assert!(
        !fs::read_to_string(home.path().join(".codex").join("akra-hookers-hook.json"))
            .expect("manifest")
            .contains("akra-hookers capture")
    );
}
