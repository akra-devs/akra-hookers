use crate::{ActivityStore, StoreError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureClientObservation {
    pub target_id: String,
    pub client: String,
    pub last_captured_at_us: i64,
}

impl ActivityStore {
    pub async fn capture_client_observations(
        &self,
    ) -> Result<Vec<CaptureClientObservation>, StoreError> {
        sqlx::query_as::<_, (String, String, i64)>(
            "SELECT capture_target, capture_client, MAX(captured_at_us)
             FROM activity_events
             WHERE capture_target IS NOT NULL
               AND capture_client IS NOT NULL
               AND captured_at_us IS NOT NULL
               AND deleted_at_us IS NULL
             GROUP BY capture_target, capture_client
             ORDER BY capture_target, capture_client",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(
                    |(target_id, client, last_captured_at_us)| CaptureClientObservation {
                        target_id,
                        client,
                        last_captured_at_us,
                    },
                )
                .collect()
        })
        .map_err(StoreError::from)
    }
}
