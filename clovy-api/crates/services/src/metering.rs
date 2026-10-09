use crate::pricing::PricingTable;
use clovy_domain::{ActionSlug, ProviderCredentials, UpstreamProvider, UserId};

pub(crate) fn uses_user_key_for_model(
    pricing: &PricingTable,
    model_id: &str,
    provider_credentials: &ProviderCredentials,
) -> bool {
    pricing
        .upstream_provider(model_id)
        .is_some_and(|provider| provider_credentials.has_user_key(provider))
}

pub(crate) fn log_skipped_user_key(
    action: ActionSlug,
    user_id: &UserId,
    model_id: &str,
    provider: UpstreamProvider,
) {
    tracing::info!(
        user_id = %user_id.0,
        action = action.as_str(),
        model = model_id,
        provider = provider.as_str(),
        "skipped Clovy credit metering for user-provided API key"
    );
}

pub(crate) fn log_skipped_user_key_for_model(
    pricing: &PricingTable,
    action: ActionSlug,
    user_id: &UserId,
    model_id: &str,
) {
    let provider_name = pricing
        .upstream_provider(model_id)
        .map_or("unknown", UpstreamProvider::as_str);
    tracing::info!(
        user_id = %user_id.0,
        action = action.as_str(),
        model = model_id,
        provider = provider_name,
        "skipped Clovy credit metering for user-provided API key"
    );
}
