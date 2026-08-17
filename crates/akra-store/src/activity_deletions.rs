use crate::{ActivityStore, StoreError};

impl ActivityStore {
    /// Tombstones one active activity and removes its live canvas projection.
    /// The source row remains in place so dedupe and relational provenance do
    /// not become inconsistent after a user-initiated deletion.
    pub async fn delete_activity(&self, activity_id: i64) -> Result<(), StoreError> {
        self.soft_delete_activity(activity_id).await
    }
}
