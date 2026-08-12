use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use fs2::FileExt;
use tempfile::NamedTempFile;

use super::{
    CodexHookLifecycleSet, CodexHookUpdate, CodexLifecycleError, ManifestSnapshot,
    append_akra_hooks, is_wsl_unc, read_config_bytes, read_manifest_bytes, remove_akra_hooks,
    trust,
};

pub(super) fn apply_updates(updates: &[CodexHookUpdate]) -> Result<(), CodexLifecycleError> {
    apply_updates_with(updates, write_codex_file_atomic)
}

fn apply_updates_with(
    updates: &[CodexHookUpdate],
    write: impl FnMut(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), CodexLifecycleError> {
    let _locks = lock_paths(
        updates
            .iter()
            .map(|update| update.lifecycle.manifest_path.clone()),
    )?;
    let mut changes = Vec::with_capacity(updates.len() * 2);
    for update in updates {
        update
            .lifecycle
            .manifest_path
            .parent()
            .ok_or(CodexLifecycleError::MissingManifestParent)?;
        let config_path = update.lifecycle.config_path()?;
        let ManifestSnapshot {
            original: manifest_original,
            mut hooks,
        } = update.lifecycle.read_snapshot()?;
        let removed = remove_akra_hooks(&mut hooks);
        let installed = update.command.as_ref().map(|command| {
            let locations = append_akra_hooks(&mut hooks, command);
            (locations, command)
        });
        let manifest_intended = if update.command.is_some() || manifest_original.is_some() {
            Some(serde_json::to_vec_pretty(&hooks)?)
        } else {
            None
        };
        let config_original = if installed.is_some()
            || !removed.managed_locations.is_empty()
            || !removed.retained_moves.is_empty()
        {
            read_config_bytes(&config_path)?
        } else {
            None
        };
        let config_intended = trust::prepare_config(
            &config_path,
            &update.lifecycle.manifest_path,
            config_original.as_deref(),
            &removed.managed_locations,
            &removed.retained_moves,
            installed
                .as_ref()
                .map(|(locations, command)| (locations.as_slice(), *command)),
        )?;

        // Write trust first so a newly discovered hook never appears without its
        // matching state during a normal successful installation.
        changes.push(PreparedFile {
            kind: PreparedFileKind::Config,
            path: config_path,
            original: config_original,
            intended: config_intended,
        });
        changes.push(PreparedFile {
            kind: PreparedFileKind::Manifest,
            path: update.lifecycle.manifest_path.clone(),
            original: manifest_original,
            intended: manifest_intended,
        });
    }
    apply_changes(&changes, write)
}

pub(super) fn enable(
    lifecycles: &CodexHookLifecycleSet,
    command: &str,
) -> Result<(), CodexLifecycleError> {
    enable_with(lifecycles, command, write_codex_file_atomic)
}

pub(super) fn disable(lifecycles: &CodexHookLifecycleSet) -> Result<(), CodexLifecycleError> {
    disable_with(lifecycles, write_codex_file_atomic)
}

pub(super) fn is_enabled(lifecycles: &CodexHookLifecycleSet) -> Result<bool, CodexLifecycleError> {
    let _locks = lock_manifests(lifecycles)?;
    let mut items = lifecycles.lifecycles.iter();
    let Some(first) = items.next() else {
        return Ok(false);
    };
    if !first.is_enabled()? {
        return Ok(false);
    }
    for lifecycle in items {
        if !lifecycle.is_enabled()? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn managed_command(
    lifecycles: &CodexHookLifecycleSet,
) -> Result<Option<super::CodexHookCommand>, CodexLifecycleError> {
    let _locks = lock_manifests(lifecycles)?;
    for lifecycle in &lifecycles.lifecycles {
        if let Some(command) = lifecycle.managed_command()? {
            return Ok(Some(command));
        }
    }
    Ok(None)
}

fn enable_with(
    lifecycles: &CodexHookLifecycleSet,
    command: &str,
    write: impl FnMut(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), CodexLifecycleError> {
    let command = super::CodexHookCommand::same(command);
    let updates = lifecycles
        .lifecycles
        .iter()
        .cloned()
        .map(|lifecycle| CodexHookUpdate {
            lifecycle,
            command: Some(command.clone()),
        })
        .collect::<Vec<_>>();
    apply_updates_with(&updates, write)
}

fn disable_with(
    lifecycles: &CodexHookLifecycleSet,
    write: impl FnMut(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), CodexLifecycleError> {
    let updates = lifecycles
        .lifecycles
        .iter()
        .cloned()
        .map(|lifecycle| CodexHookUpdate {
            lifecycle,
            command: None,
        })
        .collect::<Vec<_>>();
    apply_updates_with(&updates, write)
}

#[derive(Clone, Copy)]
enum PreparedFileKind {
    Manifest,
    Config,
}

struct PreparedFile {
    kind: PreparedFileKind,
    path: PathBuf,
    original: Option<Vec<u8>>,
    intended: Option<Vec<u8>>,
}

fn apply_changes(
    changes: &[PreparedFile],
    mut write: impl FnMut(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), CodexLifecycleError> {
    let mut attempted = Vec::new();

    for (index, change) in changes.iter().enumerate() {
        if change.original.as_deref() == change.intended.as_deref() {
            continue;
        }
        let Some(intended) = &change.intended else {
            continue;
        };
        let parent = change
            .path
            .parent()
            .ok_or(CodexLifecycleError::MissingManifestParent)?;
        if let Err(error) = fs::create_dir_all(parent) {
            return rollback_after_error(changes, &attempted, error.into());
        }
        match read_prepared_file(change) {
            Ok(current) if current == change.original => {}
            Ok(_) => {
                return rollback_after_error(changes, &attempted, concurrent_change_error(change));
            }
            Err(error) => return rollback_after_error(changes, &attempted, error),
        }

        attempted.push(index);
        if let Err(error) = write(&change.path, intended) {
            return rollback_after_error(changes, &attempted, error.into());
        }
    }

    Ok(())
}

fn rollback_after_error(
    changes: &[PreparedFile],
    attempted: &[usize],
    source: CodexLifecycleError,
) -> Result<(), CodexLifecycleError> {
    let mut rollback_error = None;

    for index in attempted.iter().rev() {
        let change = &changes[*index];
        let result = restore(change);
        if rollback_error.is_none() {
            rollback_error = result.err();
        }
    }

    match rollback_error {
        Some(rollback) => Err(CodexLifecycleError::Rollback {
            source: Box::new(source),
            rollback: Box::new(rollback),
        }),
        None => Err(source),
    }
}

fn restore(change: &PreparedFile) -> Result<(), CodexLifecycleError> {
    if read_prepared_file(change)? != change.intended {
        return Err(concurrent_change_error(change));
    }
    match &change.original {
        Some(original) => write_codex_file_atomic(&change.path, original)?,
        None => {
            match fs::remove_file(&change.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            sync_parent(&change.path)?;
        }
    }
    Ok(())
}

fn read_prepared_file(change: &PreparedFile) -> Result<Option<Vec<u8>>, CodexLifecycleError> {
    match change.kind {
        PreparedFileKind::Manifest => read_manifest_bytes(&change.path),
        PreparedFileKind::Config => read_config_bytes(&change.path),
    }
}

fn concurrent_change_error(change: &PreparedFile) -> CodexLifecycleError {
    match change.kind {
        PreparedFileKind::Manifest => {
            CodexLifecycleError::ConcurrentManifestChange(change.path.clone())
        }
        PreparedFileKind::Config => {
            CodexLifecycleError::ConcurrentConfigChange(change.path.clone())
        }
    }
}

pub(super) fn write_codex_file_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Codex manifest has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent(path)
}

fn sync_parent(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Codex manifest has no parent"))?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    let result = options
        .open(parent)
        .and_then(|directory| directory.sync_all());
    #[cfg(windows)]
    if let Err(error) = &result
        && (error.kind() == std::io::ErrorKind::PermissionDenied
            || (is_wsl_unc(path) && error.raw_os_error() == Some(1)))
    {
        return Ok(());
    }
    result
}

fn lock_manifests(
    lifecycles: &CodexHookLifecycleSet,
) -> Result<Vec<ManifestLock>, CodexLifecycleError> {
    lock_paths(
        lifecycles
            .lifecycles
            .iter()
            .map(|lifecycle| lifecycle.manifest_path.clone()),
    )
}

fn lock_paths(
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<Vec<ManifestLock>, CodexLifecycleError> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().map(|path| lock_manifest(&path)).collect()
}

fn lock_manifest(manifest: &Path) -> Result<ManifestLock, CodexLifecycleError> {
    // WSL's Plan 9 UNC bridge does not implement Windows byte-range locks. A
    // kernel named mutex provides process-wide serialization without relying on
    // the bridge and is released automatically if its owner exits or crashes.
    #[cfg(windows)]
    if is_wsl_unc(manifest) {
        return lock_wsl_manifest(manifest);
    }
    let parent = manifest
        .parent()
        .ok_or(CodexLifecycleError::MissingManifestParent)?;
    fs::create_dir_all(parent)?;
    let lock_path = lock_path(manifest);
    if let Ok(metadata) = fs::symlink_metadata(&lock_path)
        && !metadata.file_type().is_file()
    {
        return Err(CodexLifecycleError::UnsafeManifest(lock_path));
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        if !is_wsl_unc(manifest) {
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&lock_path)?;
    file.lock_exclusive()?;
    Ok(ManifestLock::File { _guard: file })
}

enum ManifestLock {
    File {
        _guard: File,
    },
    #[cfg(windows)]
    WslNamed {
        _guard: named_lock::NamedLockGuard,
    },
}

#[cfg(windows)]
fn lock_wsl_manifest(manifest: &Path) -> Result<ManifestLock, CodexLifecycleError> {
    use std::{thread, time::Duration, time::Instant};

    const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
    const RETRY_INTERVAL: Duration = Duration::from_millis(25);

    let lock = named_lock::NamedLock::create(&wsl_lock_name(manifest))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match lock.try_lock() {
            Ok(guard) => return Ok(ManifestLock::WslNamed { _guard: guard }),
            Err(named_lock::Error::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(RETRY_INTERVAL);
            }
            Err(named_lock::Error::WouldBlock) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "timed out waiting for the WSL Codex hook manifest lock: {}",
                        manifest.display()
                    ),
                )
                .into());
            }
            Err(error) => return Err(std::io::Error::other(error.to_string()).into()),
        }
    }
}

#[cfg(windows)]
fn wsl_lock_name(manifest: &Path) -> String {
    // Use a stable digest rather than DefaultHasher so different Akra binary
    // versions agree on the mutex name while an upgrade is in progress. A hash
    // collision can only over-serialize unrelated manifests; it cannot allow
    // concurrent writes to the same manifest.
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let path = canonical_wsl_lock_path(manifest);
    let mut hash = FNV_OFFSET;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("akra-hookers-codex-wsl-{hash:016x}-{}", path.len())
}

#[cfg(windows)]
fn canonical_wsl_lock_path(manifest: &Path) -> String {
    let path = manifest.to_string_lossy().replace('/', "\\").to_lowercase();
    if let Some(rest) = path.strip_prefix(r"\\?\unc\wsl.localhost\") {
        format!(r"wsl.localhost\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\wsl.localhost\") {
        format!(r"wsl.localhost\{rest}")
    } else {
        path
    }
}

fn lock_path(manifest: &Path) -> PathBuf {
    manifest.with_extension("json.akra.lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[cfg(windows)]
    use std::{
        process::Command,
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    const EMPTY_MANIFEST: &[u8] = b"{ \"description\": \"exact bytes\", \"hooks\": {} }\n";
    const MANAGED_MANIFEST: &[u8] = br#"{
  "hooks": { "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "akra-hookers capture" }] }] }
}
"#;

    #[test]
    fn failed_enable_write_restores_exact_bytes_and_missing_state() {
        let homes = TempDir::new().expect("homes");
        let paths = ["first", "second", "third"].map(|name| homes.path().join(name));
        fs::create_dir_all(&paths[0]).expect("first home");
        fs::create_dir_all(&paths[2]).expect("third home");
        fs::write(paths[0].join("hooks.json"), EMPTY_MANIFEST).expect("first manifest");
        fs::write(paths[2].join("hooks.json"), EMPTY_MANIFEST).expect("third manifest");
        let set = CodexHookLifecycleSet::from_codex_homes(paths.clone());
        let mut writes = 0;

        let error = enable_with(&set, "akra-hookers capture", |path, contents| {
            writes += 1;
            if writes == 3 {
                fs::write(path, contents)?;
                return Err(std::io::Error::other("injected enable failure"));
            }
            fs::write(path, contents)
        })
        .expect_err("third write fails");

        assert!(error.to_string().contains("injected enable failure"));
        assert_eq!(
            fs::read(paths[0].join("hooks.json")).expect("first"),
            EMPTY_MANIFEST
        );
        assert!(!paths[1].join("hooks.json").exists());
        assert_eq!(
            fs::read(paths[2].join("hooks.json")).expect("third"),
            EMPTY_MANIFEST
        );
        for path in paths {
            assert!(
                !path.join("config.toml").exists(),
                "failed enable must remove newly created trust config"
            );
        }
    }

    #[test]
    fn failed_manifest_write_restores_exact_config_bytes() {
        let home = TempDir::new().expect("home");
        let manifest = home.path().join("hooks.json");
        let config = home.path().join("config.toml");
        let original_config = b"# exact config bytes\nmodel = \"gpt-test\"\n";
        fs::write(&manifest, EMPTY_MANIFEST).expect("manifest");
        fs::write(&config, original_config).expect("config");
        let set = CodexHookLifecycleSet::from_codex_homes([home.path().to_path_buf()]);

        let error = enable_with(&set, "akra-hookers capture", |path, contents| {
            fs::write(path, contents)?;
            if path.file_name().is_some_and(|name| name == "hooks.json") {
                Err(std::io::Error::other("injected manifest failure"))
            } else {
                Ok(())
            }
        })
        .expect_err("manifest write fails after config write");

        assert!(error.to_string().contains("injected manifest failure"));
        assert_eq!(fs::read(&config).expect("restored config"), original_config);
        assert_eq!(
            fs::read(&manifest).expect("restored manifest"),
            EMPTY_MANIFEST
        );
    }

    #[test]
    fn failed_disable_write_restores_every_manifest() {
        let homes = TempDir::new().expect("homes");
        let paths = ["first", "second", "third"].map(|name| homes.path().join(name));
        for path in &paths {
            fs::create_dir_all(path).expect("Codex home");
            fs::write(path.join("hooks.json"), MANAGED_MANIFEST).expect("manifest");
        }
        let set = CodexHookLifecycleSet::from_codex_homes(paths.clone());
        let mut writes = 0;

        let error = disable_with(&set, |path, contents| {
            writes += 1;
            if writes == 2 {
                fs::write(path, contents)?;
                return Err(std::io::Error::other("injected disable failure"));
            }
            fs::write(path, contents)
        })
        .expect_err("second write fails");

        assert!(error.to_string().contains("injected disable failure"));
        for path in paths {
            assert_eq!(
                fs::read(path.join("hooks.json")).expect("restored manifest"),
                MANAGED_MANIFEST
            );
        }
    }

    #[test]
    fn concurrent_later_change_rolls_back_earlier_manifest_without_overwrite() {
        let homes = TempDir::new().expect("homes");
        let paths = ["first", "second"].map(|name| homes.path().join(name));
        for path in &paths {
            fs::create_dir_all(path).expect("Codex home");
            fs::write(path.join("hooks.json"), EMPTY_MANIFEST).expect("manifest");
        }
        let second_manifest = paths[1].join("hooks.json");
        let external = b"{ \"description\": \"external writer\", \"hooks\": {} }\n";
        let set = CodexHookLifecycleSet::from_codex_homes(paths.clone());
        let mut writes = 0;

        let error = enable_with(&set, "akra-hookers capture", |path, contents| {
            writes += 1;
            fs::write(path, contents)?;
            if writes == 1 {
                fs::write(&second_manifest, external)?;
            }
            Ok(())
        })
        .expect_err("concurrent later change must abort");

        assert!(matches!(
            error,
            CodexLifecycleError::ConcurrentManifestChange(_)
        ));
        assert_eq!(
            fs::read(paths[0].join("hooks.json")).expect("rolled back first"),
            EMPTY_MANIFEST
        );
        assert_eq!(
            fs::read(second_manifest).expect("preserved external change"),
            external
        );
    }

    #[cfg(windows)]
    #[test]
    fn wsl_named_mutex_serializes_across_processes() {
        const MODE_ENV: &str = "AKRA_WSL_LOCK_SERIALIZE_MODE";
        const MANIFEST_ENV: &str = "AKRA_WSL_LOCK_SERIALIZE_MANIFEST";
        const READY_ENV: &str = "AKRA_WSL_LOCK_SERIALIZE_READY";

        if std::env::var_os(MODE_ENV).is_some() {
            let manifest = PathBuf::from(std::env::var_os(MANIFEST_ENV).expect("manifest path"));
            let ready = PathBuf::from(std::env::var_os(READY_ENV).expect("ready path"));
            let _guard = lock_wsl_manifest(&manifest).expect("child lock");
            fs::write(ready, b"ready").expect("ready marker");
            thread::sleep(Duration::from_millis(600));
            return;
        }

        let coordination = TempDir::new().expect("coordination directory");
        let ready = coordination.path().join("ready");
        let manifest = unique_wsl_manifest("serialize");
        let mut child = spawn_lock_test(
            "codex::lifecycle::transaction::tests::wsl_named_mutex_serializes_across_processes",
            MODE_ENV,
            MANIFEST_ENV,
            READY_ENV,
            &manifest,
            &ready,
        );
        wait_for_marker(&ready);

        let started = Instant::now();
        let guard = lock_wsl_manifest(&manifest).expect("parent lock after child");
        let waited = started.elapsed();
        drop(guard);

        assert!(child.wait().expect("child status").success());
        assert!(
            waited >= Duration::from_millis(300),
            "parent acquired a process-owned WSL mutex too early after {waited:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn wsl_named_mutex_recovers_after_owner_exits_without_drop() {
        const MODE_ENV: &str = "AKRA_WSL_LOCK_RECOVERY_MODE";
        const MANIFEST_ENV: &str = "AKRA_WSL_LOCK_RECOVERY_MANIFEST";
        const READY_ENV: &str = "AKRA_WSL_LOCK_RECOVERY_READY";

        if std::env::var_os(MODE_ENV).is_some() {
            let manifest = PathBuf::from(std::env::var_os(MANIFEST_ENV).expect("manifest path"));
            let ready = PathBuf::from(std::env::var_os(READY_ENV).expect("ready path"));
            let _guard = lock_wsl_manifest(&manifest).expect("child lock");
            fs::write(ready, b"ready").expect("ready marker");
            std::process::exit(71);
        }

        let coordination = TempDir::new().expect("coordination directory");
        let ready = coordination.path().join("ready");
        let manifest = unique_wsl_manifest("recovery");
        let mut child = spawn_lock_test(
            "codex::lifecycle::transaction::tests::wsl_named_mutex_recovers_after_owner_exits_without_drop",
            MODE_ENV,
            MANIFEST_ENV,
            READY_ENV,
            &manifest,
            &ready,
        );
        wait_for_marker(&ready);
        let status = child.wait().expect("child status");
        assert_eq!(status.code(), Some(71));

        let started = Instant::now();
        let guard = lock_wsl_manifest(&manifest).expect("recover abandoned mutex");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "abandoned WSL mutex was not recovered promptly"
        );
        drop(guard);
    }

    #[cfg(windows)]
    #[test]
    fn wsl_named_mutex_canonicalizes_extended_unc_prefix() {
        let regular = Path::new(r"\\wsl.localhost\Ubuntu\home\akra\.codex\hooks.json");
        let extended = Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\akra\.codex\hooks.json");

        assert_eq!(wsl_lock_name(regular), wsl_lock_name(extended));
    }

    #[cfg(windows)]
    fn spawn_lock_test(
        test_name: &str,
        mode_env: &str,
        manifest_env: &str,
        ready_env: &str,
        manifest: &Path,
        ready: &Path,
    ) -> std::process::Child {
        Command::new(std::env::current_exe().expect("current test executable"))
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(mode_env, "child")
            .env(manifest_env, manifest)
            .env(ready_env, ready)
            .spawn()
            .expect("spawn child test")
    }

    #[cfg(windows)]
    fn unique_wsl_manifest(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        PathBuf::from(format!(
            r"\\wsl.localhost\Ubuntu\tmp\akra-lock-{label}-{}-{nonce}\.codex\hooks.json",
            std::process::id()
        ))
    }

    #[cfg(windows)]
    fn wait_for_marker(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "child did not create marker {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
