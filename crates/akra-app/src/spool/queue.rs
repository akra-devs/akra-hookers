use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::UNIX_EPOCH,
};

use fs2::FileExt;

use super::{
    MAX_PENDING_BYTES, MAX_PENDING_ITEM_BYTES, MAX_PENDING_ITEMS, RECOVERY_BATCH_SIZE, Spool,
    SpoolError, SpoolItem,
};

const MAX_DIRECTORY_SCAN_ENTRIES: usize = MAX_PENDING_ITEMS * 4;

impl Spool {
    pub(super) fn lock_admission(&self) -> Result<File, SpoolError> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join(".admission.lock"))?;
        lock.lock_exclusive()?;
        Ok(lock)
    }

    pub(super) fn ensure_capacity(&self, incoming_bytes: u64) -> Result<(), SpoolError> {
        let mut entries = 0;
        let mut items = 0;
        let mut bytes = 0_u64;

        for entry in fs::read_dir(&self.directory)? {
            entries += 1;
            if entries > MAX_DIRECTORY_SCAN_ENTRIES {
                return Err(SpoolError::QueueInspectionLimit { entries });
            }
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !is_pending(&path) {
                continue;
            }
            items += 1;
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
            if items >= MAX_PENDING_ITEMS
                || bytes.saturating_add(incoming_bytes) > MAX_PENDING_BYTES
            {
                return Err(SpoolError::QueueFull { items, bytes });
            }
        }
        Ok(())
    }

    pub fn drain(&self) -> Result<Vec<Vec<u8>>, SpoolError> {
        self.recovery_candidates()?
            .iter()
            .map(|item| self.read(item))
            .collect()
    }

    pub fn pending(&self) -> Result<Vec<SpoolItem>, SpoolError> {
        Ok(self
            .pending_paths(|_| true)?
            .into_iter()
            .map(|path| SpoolItem { path })
            .collect())
    }

    pub(crate) fn recovery_candidates(&self) -> Result<Vec<SpoolItem>, SpoolError> {
        let deferred = self
            .deferred
            .lock()
            .map_err(|_| SpoolError::StatePoisoned)?;
        let paths = self.pending_paths(|path| !deferred.contains(path))?;
        drop(deferred);
        if paths.is_empty() {
            return Ok(Vec::new());
        }

        let batch_size = paths.len().min(RECOVERY_BATCH_SIZE);
        let start = self
            .recovery_offset
            .fetch_add(batch_size, Ordering::Relaxed)
            % paths.len();
        Ok((0..batch_size)
            .map(|offset| SpoolItem {
                path: paths[(start + offset) % paths.len()].clone(),
            })
            .collect())
    }

    /// Remove raw assistant-result payloads after their bounded retention window.
    /// New result items carry a filename marker so even malformed envelopes can
    /// be scrubbed using their filesystem timestamp on the next recovery pass.
    pub(crate) fn expire_result_items(
        &self,
        now_us: i64,
        retention_us: i64,
    ) -> Result<usize, SpoolError> {
        let cutoff_us = now_us.saturating_sub(retention_us);
        let _admission = self.lock_admission()?;
        let paths = self.pending_paths(|_| true)?;
        let mut expired = Vec::new();

        for path in paths {
            let marked_result = is_result_pending(&path);
            let decoded = self
                .read(&SpoolItem { path: path.clone() })
                .ok()
                .and_then(|payload| super::CaptureEnvelope::decode(&payload).ok());
            let decoded_result = decoded.as_ref().is_some_and(|envelope| {
                envelope
                    .payload
                    .get("hook_event_name")
                    .and_then(serde_json::Value::as_str)
                    == Some("Stop")
            });
            if !marked_result && !decoded_result {
                continue;
            }
            let captured_at_us = decoded
                .as_ref()
                .filter(|_| decoded_result)
                .map(super::CaptureEnvelope::captured_at_us)
                .or_else(|| modified_at_us(&path));
            if captured_at_us.is_some_and(|captured| captured <= cutoff_us) {
                expired.push(path);
            }
        }

        if expired.is_empty() {
            return Ok(0);
        }
        let mut deferred = self
            .deferred
            .lock()
            .map_err(|_| SpoolError::StatePoisoned)?;
        let mut removed = 0;
        for path in expired {
            deferred.remove(&path);
            match fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        drop(deferred);
        super::sync_directory(&self.directory)?;
        Ok(removed)
    }

    fn pending_paths(
        &self,
        include: impl Fn(&PathBuf) -> bool,
    ) -> Result<Vec<PathBuf>, SpoolError> {
        let mut paths = Vec::new();
        for (index, entry) in fs::read_dir(&self.directory)?.enumerate() {
            if index >= MAX_DIRECTORY_SCAN_ENTRIES {
                return Err(SpoolError::QueueInspectionLimit { entries: index + 1 });
            }
            let path = entry?.path();
            if is_pending(&path) && include(&path) {
                paths.push(path);
                if paths.len() == MAX_PENDING_ITEMS {
                    break;
                }
            }
        }
        paths.sort();
        Ok(paths)
    }

    pub fn read(&self, item: &SpoolItem) -> Result<Vec<u8>, SpoolError> {
        let metadata = fs::symlink_metadata(&item.path)?;
        if !metadata.file_type().is_file() {
            return Err(SpoolError::NonRegular);
        }
        if metadata.len() > MAX_PENDING_ITEM_BYTES as u64 {
            return Err(SpoolError::Oversized(metadata.len()));
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

        let mut file = options.open(&item.path)?;
        if !file.metadata()?.file_type().is_file() {
            return Err(SpoolError::NonRegular);
        }
        let mut payload = Vec::new();
        Read::by_ref(&mut file)
            .take((MAX_PENDING_ITEM_BYTES + 1) as u64)
            .read_to_end(&mut payload)?;
        if payload.len() > MAX_PENDING_ITEM_BYTES {
            return Err(SpoolError::Oversized(payload.len() as u64));
        }
        Ok(payload)
    }

    pub(crate) fn defer(&self, item: &SpoolItem) -> Result<(), SpoolError> {
        self.deferred
            .lock()
            .map_err(|_| SpoolError::StatePoisoned)?
            .insert(item.path.clone());
        Ok(())
    }

    pub fn acknowledge(&self, item: SpoolItem) -> Result<(), SpoolError> {
        self.deferred
            .lock()
            .map_err(|_| SpoolError::StatePoisoned)?
            .remove(&item.path);
        fs::remove_file(item.path)?;
        super::sync_directory(&self.directory)?;
        Ok(())
    }
}

fn is_pending(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "pending")
}

fn is_result_pending(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".result.pending"))
}

fn modified_at_us(path: &Path) -> Option<i64> {
    let elapsed = fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?;
    i64::try_from(elapsed.as_micros()).ok()
}
