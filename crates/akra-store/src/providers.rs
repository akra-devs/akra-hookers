use crate::{ActivityStore, ProviderIntegration, StoreError};

impl ActivityStore {
    pub async fn set_provider_enabled(
        &self,
        provider: &str,
        enabled: bool,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO provider_integrations (provider, enabled, installation_state)
             VALUES (?, ?, 'configured')
             ON CONFLICT(provider) DO UPDATE SET enabled = excluded.enabled,
               updated_at = CURRENT_TIMESTAMP",
        )
        .bind(provider)
        .bind(enabled)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn provider_enabled(&self, provider: &str) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT enabled FROM provider_integrations WHERE provider = ?",
        )
        .bind(provider)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(1)
            != 0)
    }

    pub async fn provider(&self, provider: &str) -> Result<ProviderIntegration, StoreError> {
        let enabled = self.provider_enabled(provider).await?;
        Ok(ProviderIntegration {
            provider: provider.to_owned(),
            enabled,
        })
    }
}
