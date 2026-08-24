use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(windows)]
use std::{
    io::Read,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use akra_adapters::codex::{
    CodexHookCommand, CodexHookLifecycleSet, CodexLifecycleError, apply_codex_hook_updates,
};
use akra_store::CaptureClientObservation;
use serde::Serialize;
use thiserror::Error;

use crate::{
    capture_gate::{CaptureGate, CaptureGateError},
    paths,
};

#[derive(Clone, Debug)]
struct CodexTarget {
    id: String,
    label: String,
    environment: &'static str,
    codex_home: Option<String>,
    hook_path: Option<String>,
    physical_codex_home: PathBuf,
    lifecycle: Arc<CodexHookLifecycleSet>,
    command: Option<Arc<CodexHookCommand>>,
    enable_error: Option<String>,
    summary_runtimes: Vec<CodexRuntimeDescriptor>,
}

/// Exact Codex installation used to summarize a captured result. Keeping this
/// alongside the hook target prevents a Windows capture from accidentally using
/// a different PATH installation (and likewise prevents WSL from falling back to
/// the distribution's non-login `codex`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexRuntimeDescriptor {
    Native {
        capture_target: String,
        executable: PathBuf,
        codex_home: PathBuf,
    },
    Wsl {
        capture_target: String,
        distro: String,
        executable: String,
        codex_home: String,
    },
}

impl CodexRuntimeDescriptor {
    fn capture_target(&self) -> &str {
        match self {
            Self::Native { capture_target, .. } | Self::Wsl { capture_target, .. } => {
                capture_target
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CodexTargetRegistry {
    targets: Arc<[CodexTarget]>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CodexTargetStatus {
    pub id: String,
    pub label: String,
    pub environment: String,
    pub codex_home: Option<String>,
    pub hook_path: Option<String>,
    pub enabled: bool,
    pub available: bool,
    pub activation: CodexTargetActivation,
    pub clients: Vec<CodexCaptureClientStatus>,
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexTargetActivation {
    Disabled,
    AwaitingCapture,
    Verified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CodexCaptureClientStatus {
    pub id: String,
    pub label: String,
    pub verified: bool,
    pub last_captured_at_us: Option<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct CodexTargetSnapshot {
    gate_enabled: bool,
    states: BTreeMap<String, bool>,
    commands: BTreeMap<String, CodexHookCommand>,
}

impl CodexTargetRegistry {
    pub fn legacy(lifecycle: Arc<CodexHookLifecycleSet>, command: String) -> Self {
        Self {
            targets: Arc::from([CodexTarget {
                id: "default".to_owned(),
                label: "Codex".to_owned(),
                environment: "local",
                codex_home: None,
                hook_path: None,
                physical_codex_home: PathBuf::new(),
                lifecycle,
                command: Some(Arc::new(CodexHookCommand::same(command))),
                enable_error: None,
                summary_runtimes: native_runtime("default", paths::codex_home())
                    .into_iter()
                    .collect(),
            }]),
        }
    }

    pub fn explicit(codex_home: PathBuf, command: String) -> Self {
        let display_home = codex_home.to_string_lossy().into_owned();
        let mut target = target_from_home(
            "explicit",
            "Codex",
            "local",
            codex_home.clone(),
            display_home,
            Ok(CodexHookCommand::same(command)),
        );
        target.summary_runtimes = native_runtime("explicit", codex_home).into_iter().collect();
        Self {
            targets: Arc::from([target]),
        }
    }

    pub fn detect(executable: &Path, data_dir: &Path) -> Self {
        Self {
            targets: Arc::from(detect_targets(executable, data_dir)),
        }
    }

    /// Resolves the runtime that owns a captured result. A missing target is
    /// supported for pre-target records and deterministically selects the first
    /// detected runtime; an unknown explicit target never crosses installations.
    pub fn summary_runtime(&self, capture_target: Option<&str>) -> Option<CodexRuntimeDescriptor> {
        let mut runtimes = self
            .targets
            .iter()
            .flat_map(|target| target.summary_runtimes.iter());
        match capture_target {
            Some(capture_target) => runtimes
                .find(|runtime| runtime.capture_target() == capture_target)
                .cloned(),
            None => runtimes.next().cloned(),
        }
    }

    pub fn statuses(&self) -> Vec<CodexTargetStatus> {
        self.statuses_with_observations(&[])
    }

    pub fn statuses_with_observations(
        &self,
        observations: &[CaptureClientObservation],
    ) -> Vec<CodexTargetStatus> {
        self.targets
            .iter()
            .map(|target| match target_is_installed(target) {
                Ok(installed) => target.status(
                    installed,
                    installed || target.command.is_some(),
                    target.enable_error.clone(),
                    observations,
                ),
                Err(error) => target.status(false, false, Some(error.to_string()), observations),
            })
            .collect()
    }

    pub fn is_any_enabled(&self) -> Result<bool, CodexTargetError> {
        Ok(self
            .read_states()?
            .into_values()
            .any(std::convert::identity))
    }

    pub(crate) fn snapshot(
        &self,
        gate: &CaptureGate,
    ) -> Result<CodexTargetSnapshot, CodexTargetError> {
        let states = self.read_states()?;
        Ok(CodexTargetSnapshot {
            gate_enabled: gate.is_enabled()?,
            commands: self.snapshot_commands(&states)?,
            states,
        })
    }

    pub fn apply_all(&self, gate: &CaptureGate, enabled: bool) -> Result<(), CodexTargetError> {
        if enabled && !self.targets.iter().any(|target| target.command.is_some()) {
            return Err(CodexTargetError::NoAvailableTargets);
        }
        let mut desired = self.read_states()?;
        for target in self.targets.iter() {
            if !enabled || target.command.is_some() {
                desired.insert(target.id.clone(), enabled);
            }
        }
        self.apply_states(gate, desired)
    }

    pub fn apply_target(
        &self,
        gate: &CaptureGate,
        id: &str,
        enabled: bool,
    ) -> Result<(), CodexTargetError> {
        let target = self
            .targets
            .iter()
            .find(|target| target.id == id)
            .ok_or_else(|| CodexTargetError::UnknownTarget(id.to_owned()))?;
        if enabled && target.command.is_none() {
            return Err(CodexTargetError::UnavailableTarget {
                target: id.to_owned(),
                detail: target
                    .enable_error
                    .clone()
                    .unwrap_or_else(|| "capture command is unavailable".to_owned()),
            });
        }
        let gate_enabled = gate.is_enabled()?;
        let desired_gate = enabled
            || self
                .targets
                .iter()
                .filter(|other| other.id != id)
                .any(|other| {
                    // A broken or temporarily offline installation must not cause a
                    // healthy target toggle to turn off the shared admission gate.
                    target_is_installed(other).unwrap_or(true)
                });
        gate.set_enabled(desired_gate)?;
        if let Err(source) = self.write_target(target, enabled) {
            return match gate.set_enabled(gate_enabled) {
                Ok(()) => Err(source),
                Err(rollback) => Err(CodexTargetError::Rollback {
                    source: Box::new(source),
                    rollback: Box::new(rollback.into()),
                }),
            };
        }
        Ok(())
    }

    /// Reconcile persisted global intent while preserving per-target selection.
    /// Enabled targets are always rewritten so command and timeout upgrades take
    /// effect; a disabled gate atomically removes every managed hook.
    pub fn reconcile(&self, gate: &CaptureGate) -> Result<(), CodexTargetError> {
        let gate_enabled = gate.is_enabled()?;
        let mut desired = self.read_states()?;
        if !gate_enabled {
            desired.values_mut().for_each(|enabled| *enabled = false);
            return self.write_states(&desired);
        }
        if !desired.values().copied().any(std::convert::identity) {
            let target = self
                .targets
                .iter()
                .find(|target| target.command.is_some())
                .ok_or(CodexTargetError::NoAvailableTargets)?;
            desired.insert(target.id.clone(), true);
        }
        self.write_states(&desired)
    }

    pub(crate) fn snapshot_target(
        &self,
        gate: &CaptureGate,
        id: &str,
    ) -> Result<CodexTargetSnapshot, CodexTargetError> {
        let target = self
            .targets
            .iter()
            .find(|target| target.id == id)
            .ok_or_else(|| CodexTargetError::UnknownTarget(id.to_owned()))?;
        let enabled = target_is_installed(target)?;
        Ok(CodexTargetSnapshot {
            gate_enabled: gate.is_enabled()?,
            commands: if enabled && target.command.is_none() {
                target
                    .lifecycle
                    .managed_command()
                    .map_err(|source| CodexTargetError::TargetLifecycle {
                        target: id.to_owned(),
                        source: Box::new(source),
                    })?
                    .map(|command| BTreeMap::from([(id.to_owned(), command)]))
                    .unwrap_or_default()
            } else {
                BTreeMap::new()
            },
            states: BTreeMap::from([(id.to_owned(), enabled)]),
        })
    }

    pub(crate) fn restore(
        &self,
        gate: &CaptureGate,
        snapshot: &CodexTargetSnapshot,
    ) -> Result<(), CodexTargetError> {
        gate.set_enabled(snapshot.gate_enabled)?;
        self.write_states_with_fallback(&snapshot.states, &snapshot.commands)
    }

    pub(crate) fn restore_target(
        &self,
        gate: &CaptureGate,
        id: &str,
        snapshot: &CodexTargetSnapshot,
    ) -> Result<(), CodexTargetError> {
        let target = self
            .targets
            .iter()
            .find(|target| target.id == id)
            .ok_or_else(|| CodexTargetError::UnknownTarget(id.to_owned()))?;
        let enabled = snapshot.states.get(id).copied().unwrap_or(false);
        gate.set_enabled(snapshot.gate_enabled)?;
        self.write_target_with_fallback(target, enabled, snapshot.commands.get(id))
    }

    fn apply_states(
        &self,
        gate: &CaptureGate,
        desired: BTreeMap<String, bool>,
    ) -> Result<(), CodexTargetError> {
        let snapshot = self.snapshot(gate)?;
        let desired_gate = desired.values().copied().any(std::convert::identity);
        gate.set_enabled(desired_gate)?;
        if let Err(source) = self.write_states(&desired) {
            return match self.restore(gate, &snapshot) {
                Ok(()) => Err(source),
                Err(rollback) => Err(CodexTargetError::Rollback {
                    source: Box::new(source),
                    rollback: Box::new(rollback),
                }),
            };
        }
        Ok(())
    }

    fn read_states(&self) -> Result<BTreeMap<String, bool>, CodexTargetError> {
        let mut states = BTreeMap::new();
        for target in self.targets.iter() {
            let enabled = target_is_installed(target)?;
            states.insert(target.id.clone(), enabled);
        }
        Ok(states)
    }

    fn snapshot_commands(
        &self,
        states: &BTreeMap<String, bool>,
    ) -> Result<BTreeMap<String, CodexHookCommand>, CodexTargetError> {
        let mut commands = BTreeMap::new();
        for target in self.targets.iter().filter(|target| {
            states.get(&target.id).copied().unwrap_or(false) && target.command.is_none()
        }) {
            if let Some(command) = target.lifecycle.managed_command().map_err(|source| {
                CodexTargetError::TargetLifecycle {
                    target: target.id.clone(),
                    source: Box::new(source),
                }
            })? {
                commands.insert(target.id.clone(), command);
            }
        }
        Ok(commands)
    }

    fn write_states(&self, states: &BTreeMap<String, bool>) -> Result<(), CodexTargetError> {
        self.write_states_with_fallback(states, &BTreeMap::new())
    }

    fn write_states_with_fallback(
        &self,
        states: &BTreeMap<String, bool>,
        fallback: &BTreeMap<String, CodexHookCommand>,
    ) -> Result<(), CodexTargetError> {
        if self
            .targets
            .iter()
            .all(|target| !target.physical_codex_home.as_os_str().is_empty())
        {
            let updates = self
                .targets
                .iter()
                .filter_map(|target| {
                    let enabled = states.get(&target.id).copied().unwrap_or(false);
                    let command = match (
                        enabled,
                        target
                            .command
                            .as_deref()
                            .or_else(|| fallback.get(&target.id)),
                    ) {
                        (true, Some(command)) => Some(command.clone()),
                        // Keep an already-installed hook intact when command
                        // construction is temporarily unavailable. Disable is
                        // still represented by an explicit None update.
                        (true, None) => return None,
                        (false, _) => None,
                    };
                    Some((target.physical_codex_home.clone(), command))
                })
                .collect::<Vec<_>>();
            return apply_codex_hook_updates(updates).map_err(|source| {
                CodexTargetError::TargetLifecycle {
                    target: "detected-installations".to_owned(),
                    source: Box::new(source),
                }
            });
        }

        // Compatibility path for callers that supply an opaque multi-home
        // lifecycle set through app_with_codex_lifecycle.
        for target in self.targets.iter() {
            let enabled = states.get(&target.id).copied().unwrap_or(false);
            let result = if enabled {
                let command = target
                    .command
                    .as_deref()
                    .or_else(|| fallback.get(&target.id))
                    .ok_or_else(|| CodexTargetError::UnavailableTarget {
                        target: target.id.clone(),
                        detail: target
                            .enable_error
                            .clone()
                            .unwrap_or_else(|| "capture command is unavailable".to_owned()),
                    })?;
                // Legacy callers cannot express distinct platform commands.
                target.lifecycle.enable(command.command())
            } else {
                target.lifecycle.disable()
            };
            result.map_err(|source| CodexTargetError::TargetLifecycle {
                target: target.id.clone(),
                source: Box::new(source),
            })?;
        }
        Ok(())
    }

    fn write_target(&self, target: &CodexTarget, enabled: bool) -> Result<(), CodexTargetError> {
        self.write_target_with_fallback(target, enabled, None)
    }

    fn write_target_with_fallback(
        &self,
        target: &CodexTarget,
        enabled: bool,
        fallback: Option<&CodexHookCommand>,
    ) -> Result<(), CodexTargetError> {
        let command = target.command.as_deref().or(fallback);
        if enabled && command.is_none() {
            return Err(CodexTargetError::UnavailableTarget {
                target: target.id.clone(),
                detail: target
                    .enable_error
                    .clone()
                    .unwrap_or_else(|| "capture command is unavailable".to_owned()),
            });
        }
        let result = if target.physical_codex_home.as_os_str().is_empty() {
            if enabled {
                target
                    .lifecycle
                    .enable(command.expect("validated command").command())
            } else {
                target.lifecycle.disable()
            }
        } else {
            let command = enabled.then(|| command.expect("validated command").clone());
            apply_codex_hook_updates([(target.physical_codex_home.clone(), command)])
        };
        result.map_err(|source| CodexTargetError::TargetLifecycle {
            target: target.id.clone(),
            source: Box::new(source),
        })
    }
}

impl CodexTarget {
    fn status(
        &self,
        enabled: bool,
        available: bool,
        detail: Option<String>,
        observations: &[CaptureClientObservation],
    ) -> CodexTargetStatus {
        let installed_at_us = self
            .physical_codex_home
            .join("hooks.json")
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_micros()).ok());
        let observations = observations
            .iter()
            .filter(|observation| observation.target_id == self.id)
            .filter(|observation| {
                installed_at_us.is_none_or(|installed| observation.last_captured_at_us >= installed)
            })
            .collect::<Vec<_>>();
        let clients = self
            .expected_clients()
            .into_iter()
            .map(|(id, label)| {
                let last_captured_at_us = observations
                    .iter()
                    .filter(|observation| observation.client == id)
                    .map(|observation| observation.last_captured_at_us)
                    .max();
                CodexCaptureClientStatus {
                    id: id.to_owned(),
                    label: label.to_owned(),
                    verified: last_captured_at_us.is_some(),
                    last_captured_at_us,
                }
            })
            .collect();
        let activation = if !enabled {
            CodexTargetActivation::Disabled
        } else if observations.is_empty() {
            CodexTargetActivation::AwaitingCapture
        } else {
            CodexTargetActivation::Verified
        };
        CodexTargetStatus {
            id: self.id.clone(),
            label: self.label.clone(),
            environment: self.environment.to_owned(),
            codex_home: self.codex_home.clone(),
            hook_path: self.hook_path.clone(),
            enabled,
            available,
            activation,
            clients,
            detail,
        }
    }

    fn expected_clients(&self) -> Vec<(&'static str, &'static str)> {
        match self.environment {
            "windows" => vec![("app", "Codex App"), ("cli", "Codex CLI")],
            "shared" => vec![
                ("app", "Codex App"),
                ("cli", "Codex CLI"),
                ("wsl_cli", "Codex CLI · WSL"),
            ],
            "wsl" => vec![("wsl_cli", "Codex CLI · WSL")],
            _ => vec![("cli", "Codex CLI")],
        }
    }
}

fn target_is_installed(target: &CodexTarget) -> Result<bool, CodexTargetError> {
    target
        .lifecycle
        .managed_command()
        .map(|command| command.is_some())
        .map_err(|source| CodexTargetError::TargetLifecycle {
            target: target.id.clone(),
            source: Box::new(source),
        })
}

fn target_from_home(
    id: impl Into<String>,
    label: impl Into<String>,
    environment: &'static str,
    codex_home: PathBuf,
    display_home: String,
    command: Result<CodexHookCommand, paths::HookCommandError>,
) -> CodexTarget {
    let physical_codex_home = normalize_home(codex_home.clone());
    let (command, configuration_error) = match command {
        Ok(command) => (Some(Arc::new(command)), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let separator = if display_home.contains('\\') {
        "\\"
    } else {
        "/"
    };
    CodexTarget {
        id: id.into(),
        label: label.into(),
        environment,
        hook_path: Some(format!(
            "{}{}hooks.json",
            display_home.trim_end_matches(['/', '\\']),
            separator
        )),
        codex_home: Some(display_home),
        physical_codex_home,
        lifecycle: Arc::new(CodexHookLifecycleSet::from_codex_homes([codex_home])),
        command,
        enable_error: configuration_error,
        summary_runtimes: Vec::new(),
    }
}

fn native_runtime(
    capture_target: impl Into<String>,
    codex_home: PathBuf,
) -> Option<CodexRuntimeDescriptor> {
    find_native_codex_binary().map(|executable| CodexRuntimeDescriptor::Native {
        capture_target: capture_target.into(),
        executable,
        codex_home,
    })
}

#[cfg(windows)]
fn find_native_codex_binary() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(explicit) = std::env::var_os("AKRA_CODEX_EXECUTABLE") {
        let explicit = PathBuf::from(explicit);
        return codex_binary_is_usable(&explicit)
            .then(|| explicit.canonicalize().unwrap_or(explicit));
    }

    // Codex Desktop keeps its launchable CLI outside the access-controlled
    // WindowsApps package. Prefer it over package aliases that pass metadata
    // checks but fail CreateProcess with AccessDenied.
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let bin_root = PathBuf::from(local_app_data)
            .join("OpenAI")
            .join("Codex")
            .join("bin");
        candidates.extend(codex_binaries_below(&bin_root));
    }

    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        candidates.push(
            directory
                .join("node_modules")
                .join("@openai")
                .join("codex")
                .join("node_modules")
                .join("@openai")
                .join("codex-win32-x64")
                .join("vendor")
                .join("x86_64-pc-windows-msvc")
                .join("bin")
                .join("codex.exe"),
        );
        candidates.push(directory.join("codex.exe"));
    }

    let mut seen = std::collections::BTreeSet::new();
    candidates.into_iter().find_map(|candidate| {
        let identity = candidate
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        if !seen.insert(identity) || !codex_binary_is_usable(&candidate) {
            return None;
        }
        Some(candidate.canonicalize().unwrap_or(candidate))
    })
}

#[cfg(windows)]
fn codex_binaries_below(root: &Path) -> Vec<PathBuf> {
    let mut candidates = std::fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .flat_map(|entry| {
            let path = entry.path();
            [path.join("codex.exe"), path.join("bin").join("codex.exe")]
        })
        .collect::<Vec<_>>();
    candidates.push(root.join("codex.exe"));
    candidates.sort_by(|left, right| {
        let modified = |path: &PathBuf| path.metadata().and_then(|meta| meta.modified()).ok();
        modified(right).cmp(&modified(left))
    });
    candidates
}

#[cfg(windows)]
fn codex_binary_is_usable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    output_bounded(Command::new(path).arg("--version"), Duration::from_secs(2))
        .is_some_and(|output| output.status.success())
}

#[cfg(not(windows))]
fn find_native_codex_binary() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("AKRA_CODEX_EXECUTABLE") {
        let explicit = PathBuf::from(explicit);
        return explicit
            .is_file()
            .then(|| explicit.canonicalize().unwrap_or(explicit));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("codex"))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.canonicalize().unwrap_or(candidate))
}

#[cfg(windows)]
fn detect_targets(executable: &Path, data_dir: &Path) -> Vec<CodexTarget> {
    let mut targets = Vec::new();
    let default_home = paths::user_home().join(".codex");
    let native_executable = find_native_codex_binary();
    let default_display_home = default_home.to_string_lossy().into_owned();
    let mut default_target = target_from_home(
        "windows-native",
        "Codex App + CLI",
        "windows",
        default_home.clone(),
        default_display_home.clone(),
        paths::hook_command_for_target_and_home(
            executable,
            data_dir,
            "windows-native",
            &default_display_home,
        )
        .map(CodexHookCommand::same),
    );
    default_target.summary_runtimes = native_executable
        .as_ref()
        .map(|executable| CodexRuntimeDescriptor::Native {
            capture_target: "windows-native".to_owned(),
            executable: executable.clone(),
            codex_home: default_home.clone(),
        })
        .into_iter()
        .collect();
    push_detected_target(&mut targets, default_target);

    let configured_home = paths::codex_home();
    if home_identity(&configured_home) != home_identity(&default_home) {
        let configured_display_home = configured_home.to_string_lossy().into_owned();
        let mut configured_target = target_from_home(
            "windows-custom",
            "Codex custom home",
            "windows",
            configured_home.clone(),
            configured_display_home.clone(),
            paths::hook_command_for_target_and_home(
                executable,
                data_dir,
                "windows-custom",
                &configured_display_home,
            )
            .map(CodexHookCommand::same),
        );
        configured_target.summary_runtimes = native_executable
            .as_ref()
            .map(|executable| CodexRuntimeDescriptor::Native {
                capture_target: "windows-custom".to_owned(),
                executable: executable.clone(),
                codex_home: configured_home.clone(),
            })
            .into_iter()
            .collect();
        push_detected_target(&mut targets, configured_target);
    }

    // Explicit --home isolation is handled by CodexTargetRegistry::explicit;
    // ambient Windows CODEX_HOME does not hide independent WSL installations.
    let wsl_probes = if std::env::var_os("AKRA_HOOKERS_SKIP_WSL").is_some() {
        Vec::new()
    } else {
        detect_wsl()
    };
    for probe in wsl_probes {
        let Some(codex_home) = wsl_home_to_windows(&probe.distro, &probe.codex_home) else {
            continue;
        };
        let has_state =
            codex_home.join("config.toml").is_file() || codex_home.join("hooks.json").is_file();
        if probe.codex_binary.is_empty() && !has_state {
            continue;
        }
        let target_id = format!("wsl:{}", probe.distro);
        let mut target = target_from_home(
            target_id.clone(),
            format!("Codex · {}", probe.distro),
            "wsl",
            codex_home,
            probe.codex_home.clone(),
            paths::wsl_hook_command_for_target_and_home(
                executable,
                data_dir,
                &probe.distro,
                &target_id,
                &probe.codex_home,
            )
            .map(CodexHookCommand::posix),
        );
        if probe.codex_binary.starts_with('/') && probe.codex_home.starts_with('/') {
            target.summary_runtimes.push(CodexRuntimeDescriptor::Wsl {
                capture_target: target_id,
                distro: probe.distro,
                executable: probe.codex_binary,
                codex_home: probe.codex_home,
            });
        }
        merge_or_push_wsl_target(&mut targets, target, executable, data_dir);
    }
    targets
}

#[cfg(not(windows))]
fn detect_targets(executable: &Path, data_dir: &Path) -> Vec<CodexTarget> {
    let codex_home = paths::codex_home();
    let display_home = codex_home.to_string_lossy().into_owned();
    let mut target = target_from_home(
        "local",
        "Codex CLI",
        "posix",
        codex_home.clone(),
        display_home.clone(),
        paths::hook_command_for_target_and_home(executable, data_dir, "local", &display_home)
            .map(CodexHookCommand::same),
    );
    target.summary_runtimes = native_runtime("local", codex_home).into_iter().collect();
    vec![target]
}

#[cfg(windows)]
fn push_detected_target(targets: &mut Vec<CodexTarget>, target: CodexTarget) {
    if !targets.iter().any(|existing| {
        home_identity(&existing.physical_codex_home) == home_identity(&target.physical_codex_home)
    }) {
        targets.push(target);
    }
}

#[cfg(windows)]
fn merge_or_push_wsl_target(
    targets: &mut Vec<CodexTarget>,
    target: CodexTarget,
    executable: &Path,
    data_dir: &Path,
) {
    let wsl_codex_home = target.codex_home.clone().unwrap_or_default();
    let wsl_runtimes = target.summary_runtimes.clone();
    let Some(existing) = targets.iter_mut().find(|existing| {
        home_identity(&existing.physical_codex_home) == home_identity(&target.physical_codex_home)
    }) else {
        push_detected_target(targets, target);
        return;
    };
    existing.environment = "shared";
    existing.label = if existing.id == "windows-native" {
        "Codex App + CLI + WSL".to_owned()
    } else {
        "Codex shared home".to_owned()
    };
    let windows_codex_home = existing.codex_home.clone().unwrap_or_default();
    match paths::shared_wsl_hook_command_for_target_and_home(
        executable,
        data_dir,
        &existing.id,
        &wsl_codex_home,
    )
    .and_then(|command| {
        paths::hook_command_for_target_and_home(
            executable,
            data_dir,
            &existing.id,
            &windows_codex_home,
        )
        .map(|command_windows| CodexHookCommand::with_windows(command, command_windows))
    }) {
        Ok(command) => {
            existing.command = Some(Arc::new(command));
            existing.enable_error = None;
        }
        Err(error) => {
            existing.command = None;
            existing.enable_error = Some(error.to_string());
        }
    }
    existing.summary_runtimes.extend(wsl_runtimes);
}

#[cfg(windows)]
fn home_identity(path: &Path) -> String {
    normalize_home(path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn normalize_home(home: PathBuf) -> PathBuf {
    let absolute = if home.is_absolute() {
        home
    } else {
        std::env::current_dir()
            .map(|current| current.join(&home))
            .unwrap_or(home)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized.canonicalize().unwrap_or_else(|_| {
        normalized
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| normalized.file_name().map(|name| parent.join(name)))
            .unwrap_or(normalized)
    })
}

#[cfg(windows)]
#[derive(Debug)]
struct WslProbe {
    distro: String,
    codex_home: String,
    codex_binary: String,
}

#[cfg(windows)]
fn detect_wsl() -> Vec<WslProbe> {
    let Some(output) = output_bounded(
        Command::new("wsl.exe").args(["--list", "--quiet"]),
        Duration::from_secs(2),
    ) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    decode_windows_output(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|distro| !distro.is_empty() && !distro.starts_with("docker-desktop"))
        .filter_map(probe_wsl_distro)
        .collect()
}

#[cfg(windows)]
fn probe_wsl_distro(distro: &str) -> Option<WslProbe> {
    if distro.chars().any(|character| {
        !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-')
    }) {
        return None;
    }
    let script = concat!(
        "printf 'HOME=%s\\n' \"$HOME\"; ",
        "printf 'CODEX_HOME=%s\\n' \"${CODEX_HOME:-}\"; ",
        "printf 'CODEX_BIN=%s\\n' \"$(command -v codex 2>/dev/null || true)\""
    );
    let output = output_bounded(
        Command::new("wsl.exe").args(["-d", distro, "--", "sh", "-lc", script]),
        Duration::from_secs(4),
    )?;
    if !output.status.success() {
        return None;
    }
    let output_text = String::from_utf8_lossy(&output.stdout);
    let values = output_text
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<BTreeMap<_, _>>();
    let linux_home = values.get("HOME")?.trim();
    let configured = values.get("CODEX_HOME").copied().unwrap_or_default().trim();
    let codex_home = if configured.is_empty() {
        format!("{}/.codex", linux_home.trim_end_matches('/'))
    } else {
        configured.to_owned()
    };
    Some(WslProbe {
        distro: distro.to_owned(),
        codex_home,
        codex_binary: values
            .get("CODEX_BIN")
            .copied()
            .unwrap_or_default()
            .trim()
            .to_owned(),
    })
}

#[cfg(windows)]
fn output_bounded(command: &mut Command, timeout: Duration) -> Option<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = Vec::new();
    child.stdout.take()?.read_to_end(&mut stdout).ok()?;
    Some(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

#[cfg(windows)]
fn wsl_home_to_windows(distro: &str, path: &str) -> Option<PathBuf> {
    paths::wsl_cwd_to_windows(distro, path).ok()
}

#[cfg(windows)]
fn decode_windows_output(bytes: &[u8]) -> String {
    let looks_utf16 = bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).any(|byte| *byte == 0);
    if looks_utf16 {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units).replace('\0', "")
    } else {
        String::from_utf8_lossy(bytes).replace('\0', "")
    }
}

#[derive(Debug, Error)]
pub enum CodexTargetError {
    #[error(transparent)]
    Gate(#[from] CaptureGateError),
    #[error("no available Codex installations were detected")]
    NoAvailableTargets,
    #[error("unknown Codex capture target: {0}")]
    UnknownTarget(String),
    #[error("Codex capture target {target} is unavailable: {detail}")]
    UnavailableTarget { target: String, detail: String },
    #[error("Codex capture target {target} hook update failed: {source}")]
    TargetLifecycle {
        target: String,
        #[source]
        source: Box<CodexLifecycleError>,
    },
    #[error("{source}; additionally failed to restore Codex target state: {rollback}")]
    Rollback {
        source: Box<CodexTargetError>,
        rollback: Box<CodexTargetError>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn shared_installation_reports_app_and_cli_capture_evidence_independently() {
        let home = TempDir::new().expect("Codex home");
        let target = target_from_home(
            "windows-native",
            "Codex App + CLI",
            "windows",
            home.path().to_path_buf(),
            home.path().to_string_lossy().into_owned(),
            Ok(CodexHookCommand::same("capture command")),
        );
        target
            .lifecycle
            .enable("capture command")
            .expect("enable hook");
        let registry = CodexTargetRegistry {
            targets: Arc::from([target]),
        };
        let captured_at_us = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_micros(),
        )
        .expect("timestamp");
        let statuses = registry.statuses_with_observations(&[CaptureClientObservation {
            target_id: "windows-native".to_owned(),
            client: "cli".to_owned(),
            last_captured_at_us: captured_at_us,
        }]);

        assert_eq!(statuses[0].activation, CodexTargetActivation::Verified);
        assert_eq!(statuses[0].clients.len(), 2);
        assert_eq!(statuses[0].clients[0].id, "app");
        assert!(!statuses[0].clients[0].verified);
        assert_eq!(statuses[0].clients[1].id, "cli");
        assert!(statuses[0].clients[1].verified);
    }

    #[test]
    fn summary_runtime_lookup_preserves_target_binary_and_home() {
        let home = TempDir::new().expect("Codex home");
        let mut target = target_from_home(
            "windows-custom",
            "Custom Codex",
            "test",
            home.path().to_path_buf(),
            home.path().to_string_lossy().into_owned(),
            Ok(CodexHookCommand::same("capture command")),
        );
        target
            .summary_runtimes
            .push(CodexRuntimeDescriptor::Native {
                capture_target: "windows-custom".to_owned(),
                executable: PathBuf::from(r"C:\exact\codex.exe"),
                codex_home: PathBuf::from(r"D:\exact\.codex"),
            });
        target.summary_runtimes.push(CodexRuntimeDescriptor::Wsl {
            capture_target: "wsl:Ubuntu".to_owned(),
            distro: "Ubuntu".to_owned(),
            executable: "/home/alex/.local/bin/codex".to_owned(),
            codex_home: "/home/alex/.codex-custom".to_owned(),
        });
        let registry = CodexTargetRegistry {
            targets: Arc::from([target]),
        };

        assert_eq!(
            registry.summary_runtime(Some("wsl:Ubuntu")),
            Some(CodexRuntimeDescriptor::Wsl {
                capture_target: "wsl:Ubuntu".to_owned(),
                distro: "Ubuntu".to_owned(),
                executable: "/home/alex/.local/bin/codex".to_owned(),
                codex_home: "/home/alex/.codex-custom".to_owned(),
            })
        );
        assert!(registry.summary_runtime(Some("wsl:Debian")).is_none());
    }

    #[test]
    fn individual_targets_keep_the_global_gate_enabled_until_the_last_hook_is_off() {
        let homes = TempDir::new().expect("Codex homes");
        let state = TempDir::new().expect("capture state");
        let first_home = homes.path().join("first");
        let second_home = homes.path().join("second");
        let registry = CodexTargetRegistry {
            targets: Arc::from([
                target_from_home(
                    "first",
                    "First Codex",
                    "test",
                    first_home.clone(),
                    first_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("first capture command")),
                ),
                target_from_home(
                    "second",
                    "Second Codex",
                    "test",
                    second_home.clone(),
                    second_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("second capture command")),
                ),
            ]),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(false).expect("initial gate");

        registry
            .apply_target(&gate, "first", true)
            .expect("enable first");
        assert!(gate.is_enabled().expect("gate on"));
        assert!(registry.statuses()[0].enabled);
        assert!(!registry.statuses()[1].enabled);

        registry
            .apply_target(&gate, "second", true)
            .expect("enable second");
        registry
            .apply_target(&gate, "first", false)
            .expect("disable first");
        assert!(gate.is_enabled().expect("second keeps gate on"));
        assert!(!registry.statuses()[0].enabled);
        assert!(registry.statuses()[1].enabled);

        registry
            .apply_target(&gate, "second", false)
            .expect("disable second");
        assert!(!gate.is_enabled().expect("last target turns gate off"));
    }

    #[test]
    fn toggling_one_target_preserves_other_manifest_bytes() {
        let homes = TempDir::new().expect("Codex homes");
        let state = TempDir::new().expect("capture state");
        let first_home = homes.path().join("first");
        let second_home = homes.path().join("second");
        std::fs::create_dir_all(&first_home).expect("first home");
        let legacy_manifest = br#"{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{
        "type": "command",
        "command": "legacy capture command",
        "akraHookersManaged": true,
        "timeout": 1
      }]
    }]
  }
}
"#;
        std::fs::write(first_home.join("hooks.json"), legacy_manifest).expect("legacy first hook");
        let registry = CodexTargetRegistry {
            targets: Arc::from([
                target_from_home(
                    "first",
                    "First Codex",
                    "test",
                    first_home.clone(),
                    first_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("new first command")),
                ),
                target_from_home(
                    "second",
                    "Second Codex",
                    "test",
                    second_home.clone(),
                    second_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("second capture command")),
                ),
            ]),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(true).expect("initial gate");

        registry
            .apply_target(&gate, "second", true)
            .expect("enable second only");

        assert_eq!(
            std::fs::read(first_home.join("hooks.json")).expect("unchanged first hook"),
            legacy_manifest
        );
        assert!(registry.statuses()[1].enabled);
    }

    #[test]
    fn reconcile_with_disabled_gate_removes_every_installed_hook() {
        let homes = TempDir::new().expect("Codex homes");
        let state = TempDir::new().expect("capture state");
        let first_home = homes.path().join("first");
        let second_home = homes.path().join("second");
        let registry = CodexTargetRegistry {
            targets: Arc::from([
                target_from_home(
                    "first",
                    "First Codex",
                    "test",
                    first_home.clone(),
                    first_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("first current command")),
                ),
                target_from_home(
                    "second",
                    "Second Codex",
                    "test",
                    second_home.clone(),
                    second_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("second current command")),
                ),
            ]),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(true).expect("gate on");
        registry.apply_all(&gate, true).expect("install hooks");
        gate.set_enabled(false).expect("persist disabled intent");

        registry.reconcile(&gate).expect("reconcile disabled gate");

        assert!(registry.statuses().iter().all(|target| !target.enabled));
        assert!(!gate.is_enabled().expect("gate remains off"));
    }

    #[test]
    fn reconcile_refreshes_selected_hooks_without_enabling_new_targets() {
        let homes = TempDir::new().expect("Codex homes");
        let state = TempDir::new().expect("capture state");
        let first_home = homes.path().join("first");
        let second_home = homes.path().join("second");
        std::fs::create_dir_all(&first_home).expect("first home");
        std::fs::write(
            first_home.join("hooks.json"),
            br#"{
  "hooks": { "UserPromptSubmit": [{ "hooks": [{
    "type": "command",
    "command": "stale command",
    "commandWindows": "stale command",
    "akraHookersManaged": true,
    "timeout": 1
  }] }] }
}"#,
        )
        .expect("stale hook");
        let registry = CodexTargetRegistry {
            targets: Arc::from([
                target_from_home(
                    "first",
                    "First Codex",
                    "test",
                    first_home.clone(),
                    first_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("first current command")),
                ),
                target_from_home(
                    "second",
                    "Second Codex",
                    "test",
                    second_home.clone(),
                    second_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("second current command")),
                ),
            ]),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(true).expect("gate on");

        registry.reconcile(&gate).expect("reconcile enabled gate");

        let hooks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(first_home.join("hooks.json")).expect("first manifest"),
        )
        .expect("valid manifest");
        let hook = &hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0];
        assert_eq!(hook["command"], "first current command");
        assert_eq!(hook["timeout"], 5);
        assert!(!second_home.join("hooks.json").exists());
        let statuses = registry.statuses();
        assert!(statuses[0].enabled);
        assert!(!statuses[1].enabled);
    }

    #[test]
    fn reconcile_preserves_the_selected_target_when_legacy_manifest_has_two_hooks() {
        let homes = TempDir::new().expect("Codex homes");
        let state = TempDir::new().expect("capture state");
        let first_home = homes.path().join("first");
        let selected_home = homes.path().join("selected");
        std::fs::create_dir_all(&selected_home).expect("selected home");
        std::fs::write(
            selected_home.join("hooks.json"),
            br#"{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{
      "type": "command",
      "command": "legacy selected command",
      "commandWindows": "legacy selected command",
      "akraHookersManaged": true,
      "timeout": 5
    }] }],
    "SubagentStart": [{ "hooks": [{
      "type": "command",
      "command": "legacy selected command",
      "commandWindows": "legacy selected command",
      "akraHookersManaged": true,
      "timeout": 5
    }] }]
  }
}"#,
        )
        .expect("legacy two-hook manifest");
        let registry = CodexTargetRegistry {
            targets: Arc::from([
                target_from_home(
                    "first",
                    "First Codex",
                    "test",
                    first_home.clone(),
                    first_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("first current command")),
                ),
                target_from_home(
                    "selected",
                    "Selected Codex",
                    "test",
                    selected_home.clone(),
                    selected_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("selected current command")),
                ),
            ]),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(true).expect("gate on");

        registry
            .reconcile(&gate)
            .expect("upgrade selected target in place");

        assert!(
            !first_home.join("hooks.json").exists(),
            "reconcile must not move selection to the first available target"
        );
        assert!(
            registry.targets[1]
                .lifecycle
                .is_enabled()
                .expect("selected target is now complete")
        );
        let manifest =
            std::fs::read_to_string(selected_home.join("hooks.json")).expect("selected manifest");
        assert!(manifest.contains("selected current command"));
        assert!(manifest.contains("Stop"));
        assert!(!manifest.contains("SubagentStart"));
    }

    #[test]
    fn command_generation_failure_does_not_prevent_disabling_an_existing_hook() {
        let home = TempDir::new().expect("Codex home");
        let state = TempDir::new().expect("capture state");
        let lifecycle = CodexHookLifecycleSet::from_codex_homes([home.path().to_path_buf()]);
        lifecycle
            .enable("old managed command")
            .expect("existing hook");
        let registry = CodexTargetRegistry {
            targets: Arc::from([target_from_home(
                "broken-command",
                "Codex",
                "test",
                home.path().to_path_buf(),
                home.path().to_string_lossy().into_owned(),
                Err(paths::HookCommandError::InvalidWslDistro(
                    "bad name".to_owned(),
                )),
            )]),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(true).expect("gate on");
        let snapshot = registry
            .snapshot_target(&gate, "broken-command")
            .expect("snapshot existing command");
        let before = &registry.statuses()[0];
        assert!(before.enabled);
        assert!(before.available, "enabled target remains removable");

        registry
            .apply_target(&gate, "broken-command", false)
            .expect("disable without generating a command");

        let after = &registry.statuses()[0];
        assert!(!after.enabled);
        assert!(!after.available);
        assert!(!gate.is_enabled().expect("last target disabled"));

        registry
            .restore_target(&gate, "broken-command", &snapshot)
            .expect("restore exact existing command");
        assert!(registry.statuses()[0].enabled);
        let manifest =
            std::fs::read_to_string(home.path().join("hooks.json")).expect("restored manifest");
        assert!(manifest.contains("old managed command"));
    }

    #[test]
    fn malformed_other_manifest_does_not_block_a_healthy_individual_target() {
        let homes = TempDir::new().expect("Codex homes");
        let state = TempDir::new().expect("capture state");
        let healthy_home = homes.path().join("healthy");
        let broken_home = homes.path().join("broken");
        std::fs::create_dir_all(&broken_home).expect("broken home");
        std::fs::write(broken_home.join("hooks.json"), b"{ malformed").expect("broken manifest");
        let registry = CodexTargetRegistry {
            targets: Arc::from([
                target_from_home(
                    "healthy",
                    "Healthy Codex",
                    "test",
                    healthy_home.clone(),
                    healthy_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("healthy command")),
                ),
                target_from_home(
                    "broken",
                    "Broken Codex",
                    "test",
                    broken_home.clone(),
                    broken_home.to_string_lossy().into_owned(),
                    Ok(CodexHookCommand::same("broken command")),
                ),
            ]),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(false).expect("gate off");

        registry
            .snapshot_target(&gate, "healthy")
            .expect("selected-only snapshot");
        registry
            .apply_target(&gate, "healthy", true)
            .expect("healthy enable");
        registry
            .apply_target(&gate, "healthy", false)
            .expect("healthy disable");

        assert!(
            gate.is_enabled().expect("conservative gate"),
            "an unreadable target keeps aggregate admission intent on"
        );
        assert!(!registry.statuses()[0].enabled);
    }

    #[cfg(windows)]
    #[test]
    fn decodes_the_utf16_output_emitted_by_wsl_list() {
        let bytes = "Ubuntu\r\ndocker-desktop\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_windows_output(&bytes),
            "Ubuntu\r\ndocker-desktop\r\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn maps_shared_and_native_wsl_codex_homes() {
        assert_eq!(
            wsl_home_to_windows("Ubuntu", "/mnt/c/Users/alex/.codex").expect("shared"),
            Path::new(r"C:\Users\alex\.codex")
        );
        assert_eq!(
            wsl_home_to_windows("Ubuntu", "/home/alex/.codex").expect("native"),
            Path::new(r"\\wsl.localhost\Ubuntu\home\alex\.codex")
        );
    }

    #[cfg(windows)]
    #[test]
    fn canonical_home_aliases_create_one_logical_target() {
        let directory = TempDir::new().expect("home");
        let home = directory.path().join(".codex");
        std::fs::create_dir_all(&home).expect("Codex home");
        let alias = home.join("missing").join("..");
        let mut targets = Vec::new();
        push_detected_target(
            &mut targets,
            target_from_home(
                "first",
                "First",
                "windows",
                home.clone(),
                home.to_string_lossy().into_owned(),
                Ok(CodexHookCommand::same("first command")),
            ),
        );
        push_detected_target(
            &mut targets,
            target_from_home(
                "alias",
                "Alias",
                "windows",
                alias.clone(),
                alias.to_string_lossy().into_owned(),
                Ok(CodexHookCommand::same("alias command")),
            ),
        );

        assert_eq!(targets.len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn shared_windows_and_wsl_home_becomes_one_cross_platform_target() {
        let directory = TempDir::new().expect("home");
        let state = TempDir::new().expect("state");
        let home = directory.path().join(".codex");
        let executable = Path::new(r"C:\tools & apps\akra-hookers.exe");
        let data_dir = Path::new(r"C:\state&data");
        let mut targets = vec![target_from_home(
            "windows-native",
            "Codex App + CLI",
            "windows",
            home.clone(),
            home.to_string_lossy().into_owned(),
            paths::hook_command(executable, data_dir).map(CodexHookCommand::same),
        )];
        let wsl_alias = home.join("missing").join("..");
        let wsl_target = target_from_home(
            "wsl:Ubuntu",
            "Codex · Ubuntu",
            "wsl",
            wsl_alias,
            "/mnt/c/shared/.codex".to_owned(),
            paths::wsl_hook_command(executable, data_dir, "Ubuntu").map(CodexHookCommand::posix),
        );

        merge_or_push_wsl_target(&mut targets, wsl_target, executable, data_dir);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].environment, "shared");
        let registry = CodexTargetRegistry {
            targets: Arc::from(targets),
        };
        let gate = CaptureGate::new(state.path());
        gate.set_enabled(false).expect("gate off");
        registry
            .apply_target(&gate, "windows-native", true)
            .expect("enable shared target");
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.join("hooks.json")).expect("shared manifest"),
        )
        .expect("valid manifest");
        let hook = &manifest["hooks"]["UserPromptSubmit"][0]["hooks"][0];
        assert!(
            hook["command"]
                .as_str()
                .expect("posix command")
                .contains("WSL_DISTRO_NAME")
        );
        assert!(
            hook["commandWindows"]
                .as_str()
                .expect("Windows command")
                .contains("powershell.exe")
        );
    }
}
