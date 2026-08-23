use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::UNIX_EPOCH,
};

use fs2::FileExt;

use super::{
    MAX_DIRECTORY_SCAN_ENTRIES, MAX_PENDING_ITEM_BYTES, MAX_PENDING_ITEMS, RECOVERY_BATCH_SIZE,
    Spool, SpoolError, SpoolItem,
};

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
    pub fn expire_result_items_if_due(
        &self,
        now_us: i64,
        retention_us: i64,
    ) -> Result<usize, SpoolError> {
        loop {
            let scheduled_at_us = self.next_result_sweep_at_us.load(Ordering::Acquire);
            if now_us < scheduled_at_us {
                return Ok(0);
            }
            let next_at_us = now_us.saturating_add(super::RESULT_RETENTION_SWEEP_INTERVAL_US);
            if self
                .next_result_sweep_at_us
                .compare_exchange(
                    scheduled_at_us,
                    next_at_us,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return match self.expire_result_items(now_us, retention_us) {
                    Ok(expired) => Ok(expired),
                    Err(error) => {
                        self.next_result_sweep_at_us.store(0, Ordering::Release);
                        Err(error)
                    }
                };
            }
        }
    }

    pub fn expire_result_items(&self, now_us: i64, retention_us: i64) -> Result<usize, SpoolError> {
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
        let removed = self.remove_paths_locked(&expired)?;
        let mut deferred = self
            .deferred
            .lock()
            .map_err(|_| SpoolError::StatePoisoned)?;
        for path in &expired {
            deferred.remove(path);
        }
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
        self.acknowledge_batch(vec![item])?;
        Ok(())
    }

    pub(crate) fn acknowledge_batch(&self, items: Vec<SpoolItem>) -> Result<usize, SpoolError> {
        if items.is_empty() {
            return Ok(0);
        }
        let _admission = self.lock_admission()?;
        let paths = items.into_iter().map(|item| item.path).collect::<Vec<_>>();
        let removed = self.remove_paths_locked(&paths)?;
        let mut deferred = self
            .deferred
            .lock()
            .map_err(|_| SpoolError::StatePoisoned)?;
        for path in paths {
            deferred.remove(&path);
        }
        Ok(removed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn result_retention_full_sweep_runs_at_most_once_per_interval() {
        let directory = TempDir::new().expect("spool directory");
        let spool = Spool::open(directory.path()).expect("spool");
        spool
            .enqueue_marked(b"invalid result payload", true)
            .expect("first result");
        let now_us = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_micros(),
        )
        .expect("timestamp");

        assert_eq!(
            spool
                .expire_result_items_if_due(now_us, 0)
                .expect("initial sweep"),
            1
        );
        spool
            .enqueue_marked(b"another invalid result payload", true)
            .expect("second result");
        assert_eq!(
            spool
                .expire_result_items_if_due(
                    now_us + super::super::RESULT_RETENTION_SWEEP_INTERVAL_US - 1,
                    0,
                )
                .expect("gated sweep"),
            0
        );
        assert_eq!(spool.pending().expect("pending").len(), 1);
        assert_eq!(
            spool
                .expire_result_items_if_due(
                    now_us + super::super::RESULT_RETENTION_SWEEP_INTERVAL_US,
                    0,
                )
                .expect("due sweep"),
            1
        );
        assert!(spool.pending().expect("pending").is_empty());
    }

    #[test]
    fn batch_acknowledgement_removes_multiple_items_together() {
        let directory = TempDir::new().expect("spool directory");
        let spool = Spool::open(directory.path()).expect("spool");
        spool.enqueue(b"first").expect("first payload");
        spool.enqueue(b"second").expect("second payload");

        let items = spool.pending().expect("pending items");
        assert_eq!(spool.acknowledge_batch(items).expect("batch removal"), 2);
        assert!(spool.pending().expect("pending after removal").is_empty());
    }
}
