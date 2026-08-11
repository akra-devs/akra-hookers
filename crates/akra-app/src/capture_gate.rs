use std::{
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
};

use akra_adapters::codex::CodexHookLifecycleSet;
use fs2::FileExt;
use tempfile::NamedTempFile;
use thiserror::Error;

const GATE_FILE: &str = "capture-enabled";

#[derive(Clone, Debug)]
pub struct CaptureGate {
    path: PathBuf,
}

impl CaptureGate {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(GATE_FILE),
        }
    }

    pub fn is_enabled(&self) -> Result<bool, CaptureGateError> {
        match fs::read_to_string(&self.path) {
            Ok(value) => match value.trim() {
                "true" => Ok(true),
                "false" => Ok(false),
                value => Err(CaptureGateError::InvalidState(value.to_owned())),
            },
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
            Err(error) => Err(CaptureGateError::Io(error)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), CaptureGateError> {
        self.set_enabled_with(enabled, |temporary, path| {
            temporary
                .persist(path)
                .map(|_| ())
                .map_err(|error| error.error)
        })
    }

    fn set_enabled_with(
        &self,
        enabled: bool,
        persist: impl FnOnce(NamedTempFile, &Path) -> io::Result<()>,
    ) -> Result<(), CaptureGateError> {
        let parent = self.path.parent().ok_or(CaptureGateError::MissingParent)?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(if enabled { b"true" } else { b"false" })?;
        temporary.as_file().sync_all()?;
        persist(temporary, &self.path)?;
        Ok(())
    }
}

pub fn enable_codex_capture(
    gate: &CaptureGate,
    lifecycle: &CodexHookLifecycleSet,
    command: &str,
) -> Result<(), CaptureLifecycleError> {
    update_gate_then_hooks(gate, true, || {
        lifecycle.enable(command).map_err(|error| error.to_string())
    })
}

pub fn disable_codex_capture(
    gate: &CaptureGate,
    lifecycle: &CodexHookLifecycleSet,
) -> Result<(), CaptureLifecycleError> {
    update_gate_then_hooks(gate, false, || {
        lifecycle.disable().map_err(|error| error.to_string())
    })
}

fn update_gate_then_hooks(
    gate: &CaptureGate,
    enabled: bool,
    update_hooks: impl FnOnce() -> Result<(), String>,
) -> Result<(), CaptureLifecycleError> {
    let _transition = lock_lifecycle(gate)?;
    let previous = gate.is_enabled()?;
    gate.set_enabled(enabled)?;
    if let Err(source) = update_hooks() {
        return match gate.set_enabled(previous) {
            Ok(()) => Err(CaptureLifecycleError::Hook(source)),
            Err(rollback) => Err(CaptureLifecycleError::Rollback {
                hook_error: source,
                rollback,
            }),
        };
    }
    Ok(())
}

pub fn reconcile_codex_capture(
    gate: &CaptureGate,
    lifecycle: &CodexHookLifecycleSet,
    command: &str,
) -> Result<(), CaptureLifecycleError> {
    let _transition = lock_lifecycle(gate)?;
    if gate.is_enabled()? {
        lifecycle
            .enable(command)
            .map_err(|error| CaptureLifecycleError::Hook(error.to_string()))?;
    } else {
        lifecycle
            .disable()
            .map_err(|error| CaptureLifecycleError::Hook(error.to_string()))?;
    }
    Ok(())
}

fn lock_lifecycle(gate: &CaptureGate) -> Result<File, CaptureGateError> {
    let parent = gate.path.parent().ok_or(CaptureGateError::MissingParent)?;
    fs::create_dir_all(parent)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join("capture-lifecycle.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}

#[derive(Debug, Error)]
pub enum CaptureGateError {
    #[error("capture gate has no parent directory")]
    MissingParent,
    #[error("capture gate filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("capture gate state is invalid: {0}")]
    InvalidState(String),
}

#[derive(Debug, Error)]
pub enum CaptureLifecycleError {
    #[error(transparent)]
    Gate(#[from] CaptureGateError),
    #[error("Codex hook update failed: {0}")]
    Hook(String),
    #[error("Codex hook update failed: {hook_error}; capture gate rollback failed: {rollback}")]
    Rollback {
        hook_error: String,
        rollback: CaptureGateError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, fs};
    use tempfile::TempDir;

    #[test]
    fn interrupted_persist_preserves_the_previous_gate_state() {
        let directory = TempDir::new().expect("data directory");
        let gate = CaptureGate::new(directory.path());
        gate.set_enabled(true).expect("initial gate");

        let error = gate
            .set_enabled_with(false, |_temporary, _path| {
                Err(io::Error::other("injected persist failure"))
            })
            .expect_err("persist must fail");

        assert!(error.to_string().contains("injected persist failure"));
        assert!(gate.is_enabled().expect("previous state remains"));
    }

    #[test]
    fn hook_update_observes_new_gate_and_failure_restores_previous_state() {
        let directory = TempDir::new().expect("data directory");
        let gate = CaptureGate::new(directory.path());
        gate.set_enabled(false).expect("initial gate");
        let observed_enabled = Cell::new(false);

        let error = update_gate_then_hooks(&gate, true, || {
            observed_enabled.set(gate.is_enabled().expect("gate visible to hook update"));
            Err("injected hook failure".to_owned())
        })
        .expect_err("hook update must fail");

        assert!(observed_enabled.get());
        assert!(error.to_string().contains("injected hook failure"));
        assert!(!gate.is_enabled().expect("previous gate restored"));
    }

    #[test]
    fn enabled_gate_reconciles_a_partially_published_multi_home_hook() {
        let directory = TempDir::new().expect("data directory");
        let homes = TempDir::new().expect("Codex homes");
        let first = homes.path().join("first");
        let second = homes.path().join("second");
        let gate = CaptureGate::new(directory.path());
        gate.set_enabled(true).expect("enabled intent");
        akra_adapters::codex::CodexHookLifecycle::from_codex_home(&first)
            .enable("akra-hookers capture --data-dir state")
            .expect("partial first-home publish");
        let lifecycle = CodexHookLifecycleSet::from_codex_homes([first.clone(), second.clone()]);

        reconcile_codex_capture(&gate, &lifecycle, "akra-hookers capture --data-dir state")
            .expect("reconciliation");

        assert!(lifecycle.is_enabled().expect("all homes enabled"));
        assert!(second.join("hooks.json").is_file());
    }

    #[test]
    fn disabled_gate_reconciles_stale_hooks_after_partial_removal() {
        let directory = TempDir::new().expect("data directory");
        let homes = TempDir::new().expect("Codex homes");
        let first = homes.path().join("first");
        let second = homes.path().join("second");
        let lifecycle = CodexHookLifecycleSet::from_codex_homes([first.clone(), second.clone()]);
        lifecycle
            .enable("akra-hookers capture --data-dir state")
            .expect("initial hooks");
        fs::write(second.join("hooks.json"), br#"{ "hooks": {} }"#)
            .expect("partial second-home removal");
        let gate = CaptureGate::new(directory.path());
        gate.set_enabled(false).expect("disabled intent");

        reconcile_codex_capture(&gate, &lifecycle, "akra-hookers capture --data-dir state")
            .expect("reconciliation");

        assert!(!lifecycle.is_enabled().expect("all homes disabled"));
        assert!(
            !fs::read_to_string(first.join("hooks.json"))
                .expect("first manifest")
                .contains("akra-hookers")
        );
    }
}
