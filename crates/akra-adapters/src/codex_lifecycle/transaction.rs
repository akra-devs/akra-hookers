use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use fs2::FileExt;
use tempfile::NamedTempFile;

use super::{
    CodexHookLifecycleSet, CodexLifecycleError, CodexMatcherGroup, ManifestSnapshot,
    read_manifest_bytes, remove_akra_hooks,
};

pub(super) fn enable(
    lifecycles: &CodexHookLifecycleSet,
    command: &str,
) -> Result<(), CodexLifecycleError> {
    enable_with(lifecycles, command, write_manifest_atomic)
}

pub(super) fn disable(lifecycles: &CodexHookLifecycleSet) -> Result<(), CodexLifecycleError> {
    disable_with(lifecycles, write_manifest_atomic)
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

fn enable_with(
    lifecycles: &CodexHookLifecycleSet,
    command: &str,
    write: impl FnMut(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), CodexLifecycleError> {
    let _locks = lock_manifests(lifecycles)?;
    let changes = lifecycles
        .lifecycles
        .iter()
        .map(|lifecycle| {
            lifecycle
                .manifest_path
                .parent()
                .ok_or(CodexLifecycleError::MissingManifestParent)?;
            let ManifestSnapshot {
                original,
                mut hooks,
            } = lifecycle.read_snapshot()?;
            remove_akra_hooks(&mut hooks);
            hooks
                .hooks
                .user_prompt_submit
                .push(CodexMatcherGroup::akra_hook(command));
            let intended = serde_json::to_vec_pretty(&hooks)?;
            Ok(PreparedManifest {
                path: lifecycle.manifest_path.clone(),
                original,
                intended: Some(intended),
            })
        })
        .collect::<Result<Vec<_>, CodexLifecycleError>>()?;

    apply_changes(&changes, write)
}

fn disable_with(
    lifecycles: &CodexHookLifecycleSet,
    write: impl FnMut(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), CodexLifecycleError> {
    let _locks = lock_manifests(lifecycles)?;
    let changes = lifecycles
        .lifecycles
        .iter()
        .map(|lifecycle| {
            lifecycle
                .manifest_path
                .parent()
                .ok_or(CodexLifecycleError::MissingManifestParent)?;
            let ManifestSnapshot {
                original,
                mut hooks,
            } = lifecycle.read_snapshot()?;
            let intended = if original.is_some() {
                remove_akra_hooks(&mut hooks);
                Some(serde_json::to_vec_pretty(&hooks)?)
            } else {
                None
            };
            Ok(PreparedManifest {
                path: lifecycle.manifest_path.clone(),
                original,
                intended,
            })
        })
        .collect::<Result<Vec<_>, CodexLifecycleError>>()?;

    apply_changes(&changes, write)
}

struct PreparedManifest {
    path: PathBuf,
    original: Option<Vec<u8>>,
    intended: Option<Vec<u8>>,
}

fn apply_changes(
    changes: &[PreparedManifest],
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
        match read_manifest_bytes(&change.path) {
            Ok(current) if current == change.original => {}
            Ok(_) => {
                return rollback_after_error(
                    changes,
                    &attempted,
                    CodexLifecycleError::ConcurrentManifestChange(change.path.clone()),
                );
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
    changes: &[PreparedManifest],
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

fn restore(change: &PreparedManifest) -> Result<(), CodexLifecycleError> {
    if read_manifest_bytes(&change.path)? != change.intended {
        return Err(CodexLifecycleError::ConcurrentManifestChange(
            change.path.clone(),
        ));
    }
    match &change.original {
        Some(original) => write_manifest_atomic(&change.path, original)?,
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

pub(super) fn write_manifest_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
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
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
    {
        return Ok(());
    }
    result
}

fn lock_manifests(lifecycles: &CodexHookLifecycleSet) -> Result<Vec<File>, CodexLifecycleError> {
    let mut paths = lifecycles
        .lifecycles
        .iter()
        .map(|lifecycle| lifecycle.manifest_path.clone())
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().map(|path| lock_manifest(&path)).collect()
}

fn lock_manifest(manifest: &Path) -> Result<File, CodexLifecycleError> {
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
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&lock_path)?;
    file.lock_exclusive()?;
    Ok(file)
}

fn lock_path(manifest: &Path) -> PathBuf {
    manifest.with_extension("json.akra.lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
}
