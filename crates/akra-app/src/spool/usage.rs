use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{MAX_DIRECTORY_SCAN_ENTRIES, MAX_PENDING_BYTES, MAX_PENDING_ITEMS, Spool, SpoolError};

const USAGE_SCHEMA_VERSION: u8 = 1;
const USAGE_SLOT_NAMES: [&str; 2] = [".usage-0.json", ".usage-1.json"];
const MAX_USAGE_STATE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UsageState {
    schema_version: u8,
    generation: u64,
    items: u64,
    bytes: u64,
    operation: Option<UsageOperation>,
    checksum: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum UsageOperation {
    Enqueue {
        pending_name: String,
        temporary_name: String,
        bytes: u64,
    },
    Remove {
        entries: Vec<UsageEntry>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UsageEntry {
    pending_name: String,
    bytes: u64,
}

impl UsageState {
    fn rebuilt(items: u64, bytes: u64) -> Self {
        Self {
            schema_version: USAGE_SCHEMA_VERSION,
            generation: 0,
            items,
            bytes,
            operation: None,
            checksum: String::new(),
        }
    }

    fn has_valid_checksum(&self) -> bool {
        self.schema_version == USAGE_SCHEMA_VERSION
            && !self.checksum.is_empty()
            && self.checksum == self.calculated_checksum()
    }

    fn seal(&mut self) {
        self.checksum = self.calculated_checksum();
    }

    fn calculated_checksum(&self) -> String {
        let mut unsigned = self.clone();
        unsigned.checksum.clear();
        let encoded = serde_json::to_vec(&unsigned).expect("usage state serialization");
        hex::encode(Sha256::digest(encoded))
    }
}

impl Spool {
    pub(super) fn reserve_enqueue_locked(
        &self,
        pending: &Path,
        temporary: &Path,
        incoming_bytes: u64,
    ) -> Result<(), SpoolError> {
        let pending_name = self.local_name(pending, ".pending")?;
        let temporary_name = self.local_name(temporary, ".tmp")?;
        let mut state = self.load_usage_locked()?;
        let items = usize::try_from(state.items).unwrap_or(usize::MAX);
        if items >= MAX_PENDING_ITEMS
            || state.bytes.saturating_add(incoming_bytes) > MAX_PENDING_BYTES
        {
            return Err(SpoolError::QueueFull {
                items,
                bytes: state.bytes,
            });
        }
        state.items = state
            .items
            .checked_add(1)
            .ok_or(SpoolError::QueueStateOverflow)?;
        state.bytes = state
            .bytes
            .checked_add(incoming_bytes)
            .ok_or(SpoolError::QueueStateOverflow)?;
        state.operation = Some(UsageOperation::Enqueue {
            pending_name,
            temporary_name,
            bytes: incoming_bytes,
        });
        self.persist_usage_locked(state)
    }

    pub(super) fn remove_paths_locked(&self, paths: &[PathBuf]) -> Result<usize, SpoolError> {
        let mut state = self.load_usage_locked()?;
        let mut names = HashSet::new();
        let mut entries = Vec::new();
        for path in paths {
            let pending_name = self.local_name(path, ".pending")?;
            if !names.insert(pending_name.clone()) {
                continue;
            }
            let metadata = match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_file() => metadata,
                Ok(_) => return Err(SpoolError::NonRegular),
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            entries.push(UsageEntry {
                pending_name,
                bytes: metadata.len(),
            });
        }
        if entries.is_empty() {
            return Ok(0);
        }
        state.operation = Some(UsageOperation::Remove {
            entries: entries.clone(),
        });
        self.persist_usage_locked(state)?;

        let mut removed = 0;
        let mut first_error = None;
        for entry in &entries {
            match fs::remove_file(self.directory.join(&entry.pending_name)) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == ErrorKind::NotFound => removed += 1,
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        let sync_error = super::sync_directory(&self.directory).err();
        if let Some(error) = first_error.or(sync_error) {
            return Err(error.into());
        }
        Ok(removed)
    }

    fn load_usage_locked(&self) -> Result<UsageState, SpoolError> {
        let mut states = Vec::new();
        for slot in USAGE_SLOT_NAMES {
            if let Some(state) = self.read_usage_slot(&self.directory.join(slot))? {
                states.push(state);
            }
        }
        let mut state = match states.into_iter().max_by_key(|state| state.generation) {
            Some(state) => state,
            None => self.rebuild_usage_locked()?,
        };
        self.reconcile_usage_operation(&mut state)?;
        Ok(state)
    }

    fn read_usage_slot(&self, path: &Path) -> Result<Option<UsageState>, SpoolError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_USAGE_STATE_BYTES {
            return Ok(None);
        }
        let mut options = OpenOptions::new();
        options.read(true);
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
        let mut encoded = Vec::new();
        options
            .open(path)?
            .take(MAX_USAGE_STATE_BYTES + 1)
            .read_to_end(&mut encoded)?;
        if encoded.len() as u64 > MAX_USAGE_STATE_BYTES {
            return Ok(None);
        }
        let Ok(state) = serde_json::from_slice::<UsageState>(&encoded) else {
            return Ok(None);
        };
        Ok(state.has_valid_checksum().then_some(state))
    }

    fn rebuild_usage_locked(&self) -> Result<UsageState, SpoolError> {
        let mut items = 0_u64;
        let mut bytes = 0_u64;
        for (index, entry) in fs::read_dir(&self.directory)?.enumerate() {
            if index >= MAX_DIRECTORY_SCAN_ENTRIES {
                return Err(SpoolError::QueueInspectionLimit { entries: index + 1 });
            }
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|extension| extension != "pending")
            {
                continue;
            }
            items = items.saturating_add(1);
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
        Ok(UsageState::rebuilt(items, bytes))
    }

    fn reconcile_usage_operation(&self, state: &mut UsageState) -> Result<(), SpoolError> {
        let Some(operation) = state.operation.take() else {
            return Ok(());
        };
        match operation {
            UsageOperation::Enqueue {
                pending_name,
                temporary_name,
                bytes,
            } => {
                validate_local_name(&pending_name, ".pending")?;
                validate_local_name(&temporary_name, ".tmp")?;
                let pending = self.directory.join(&pending_name);
                if !pending
                    .symlink_metadata()
                    .is_ok_and(|metadata| metadata.file_type().is_file())
                {
                    state.items = state.items.saturating_sub(1);
                    state.bytes = state.bytes.saturating_sub(bytes);
                    let temporary = self.directory.join(temporary_name);
                    match fs::remove_file(temporary) {
                        Ok(()) => {}
                        Err(error) if error.kind() == ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            UsageOperation::Remove { entries } => {
                for entry in entries {
                    validate_local_name(&entry.pending_name, ".pending")?;
                    if !self.directory.join(&entry.pending_name).exists() {
                        state.items = state.items.saturating_sub(1);
                        state.bytes = state.bytes.saturating_sub(entry.bytes);
                    }
                }
            }
        }
        Ok(())
    }

    fn persist_usage_locked(&self, mut state: UsageState) -> Result<(), SpoolError> {
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(SpoolError::QueueStateOverflow)?;
        state.seal();
        let encoded = serde_json::to_vec(&state)?;
        if encoded.len() as u64 > MAX_USAGE_STATE_BYTES {
            return Err(SpoolError::QueueStateOverflow);
        }
        let slot = self
            .directory
            .join(USAGE_SLOT_NAMES[(state.generation as usize) % USAGE_SLOT_NAMES.len()]);
        let temporary = self
            .directory
            .join(format!(".usage-{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        match fs::remove_file(&slot) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::rename(&temporary, slot)?;
        super::sync_directory(&self.directory)?;
        Ok(())
    }

    fn local_name(&self, path: &Path, suffix: &str) -> Result<String, SpoolError> {
        if path.parent() != Some(self.directory.as_path()) {
            return Err(SpoolError::InvalidQueueState);
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(SpoolError::InvalidQueueState)?
            .to_owned();
        validate_local_name(&name, suffix)?;
        Ok(name)
    }
}

fn validate_local_name(name: &str, suffix: &str) -> Result<(), SpoolError> {
    if name.len() > 160
        || !name.ends_with(suffix)
        || name.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | '_')
        })
    {
        return Err(SpoolError::InvalidQueueState);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn abandoned_enqueue_reservation_is_reconciled_without_a_directory_scan() {
        let directory = TempDir::new().expect("spool directory");
        let spool = Spool::open(directory.path()).expect("spool");
        let pending = directory.path().join("abandoned.pending");
        let temporary = directory.path().join("abandoned.tmp");
        {
            let _admission = spool.lock_admission().expect("admission lock");
            spool
                .reserve_enqueue_locked(&pending, &temporary, MAX_PENDING_BYTES)
                .expect("reservation");
            fs::write(&temporary, b"partial payload").expect("temporary payload");
        }

        spool
            .enqueue(b"replacement")
            .expect("abandoned reservation is rolled back");

        assert!(!temporary.exists());
        assert_eq!(spool.pending().expect("pending items").len(), 1);
    }

    #[test]
    fn completed_enqueue_reservation_remains_part_of_capacity() {
        let directory = TempDir::new().expect("spool directory");
        let spool = Spool::open(directory.path()).expect("spool");
        let pending = directory.path().join("completed.pending");
        let temporary = directory.path().join("completed.tmp");
        {
            let _admission = spool.lock_admission().expect("admission lock");
            spool
                .reserve_enqueue_locked(&pending, &temporary, MAX_PENDING_BYTES)
                .expect("reservation");
            fs::write(&pending, b"durable payload").expect("pending payload");
        }

        assert!(matches!(
            spool.enqueue(b"overflow"),
            Err(SpoolError::QueueFull { .. })
        ));
    }
}
