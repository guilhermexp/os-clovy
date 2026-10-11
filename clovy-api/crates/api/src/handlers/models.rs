use crate::{envelope::ApiResponse, error::ApiError, state::ApiState};
use axum::{
    Json,
    extract::{Query, State},
};
use clovy_config::ModelPriceConfig;
use clovy_domain::ModelKind;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct ModelsQuery {
    #[serde(rename = "type")]
    model_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub model_type: String,
    pub description: Option<String>,
    pub privacy: Option<String>,
    pub pricing: Option<serde_json::Value>,
    pub context_tokens: Option<i64>,
    pub traits: Vec<String>,
    pub capabilities: Vec<String>,
    pub price_unit: String,
    pub price_description: String,
    pub credits_per_million_seconds: Option<u64>,
    pub input_credits_per_million_tokens: Option<u64>,
    pub output_credits_per_million_tokens: Option<u64>,
    pub service_available: bool,
}

pub(crate) async fn list_models(
    State(state): State<ApiState>,
    Query(query): Query<ModelsQuery>,
) -> Result<Json<ApiResponse<Vec<ModelDto>>>, ApiError> {
    // Media has flat/quote pricing, separate from the token/audio table.
    // Explicit new filters leave unfiltered and text/asr clients unchanged.
    if let Some(kind @ ("image" | "video")) = query.model_type.as_deref() {
        let available = state.service_key_configured(clovy_domain::UpstreamProvider::Fal);
        let models = if kind == "image" {
            state
                .image()
                .priced_models()
                .map(|(id, credits)| {
                    media_dto(
                        id,
                        kind,
                        available,
                        format!("{} per image", format_credits_as_usd(credits)),
                    )
                })
                .collect()
        } else {
            state
                .video()
                .priced_models()
                .map(|(id, markup_millis)| {
                    media_dto(
                        id,
                        kind,
                        available,
                        format!("{}x provider quote", f64::from(markup_millis) / 1000.0),
                    )
                })
                .collect()
        };
        return Ok(Json(ApiResponse::ok(models)));
    }
    let kind = query
        .model_type
        .as_deref()
        .map(parse_model_kind)
        .transpose()?;
    let models = state
        .pricing()
        .priced_models(kind)
        .into_iter()
        .map(|(id, model)| to_dto(&id, &model, &state))
        .collect();
    Ok(Json(ApiResponse::ok(models)))
}

fn media_dto(id: &str, kind: &str, available: bool, price: String) -> ModelDto {
    ModelDto {
        provider: "fal".to_string(),
        id: id.to_string(),
        name: id.to_string(),
        model_type: kind.to_string(),
        description: None,
        privacy: None,
        pricing: None,
        context_tokens: None,
        traits: Vec::new(),
        capabilities: Vec::new(),
        price_unit: kind.to_string(),
        price_description: price,
        credits_per_million_seconds: None,
        input_credits_per_million_tokens: None,
        output_credits_per_million_tokens: None,
        service_available: available,
    }
}

fn parse_model_kind(value: &str) -> Result<ModelKind, ApiError> {
    match value {
        "asr" => Ok(ModelKind::Asr),
        "text" => Ok(ModelKind::Text),
        _ => Err(ApiError::unprocessable("model_type_invalid")),
    }
}

fn to_dto(id: &str, model: &ModelPriceConfig, state: &ApiState) -> ModelDto {
    let upstream_provider = match model.provider {
        clovy_config::ModelProvider::Groq => clovy_domain::UpstreamProvider::Groq,
        clovy_config::ModelProvider::Openrouter => clovy_domain::UpstreamProvider::Openrouter,
    };
    let service_available = state.service_key_configured(upstream_provider);
    ModelDto {
        provider: model.provider.as_str().to_string(),
        id: id.to_string(),
        name: model.display_name.clone(),
        model_type: model.model_type.as_str().to_string(),
        description: model.description.clone(),
        privacy: model.privacy.clone(),
        pricing: model.pricing.clone(),
        context_tokens: model.context_tokens,
        traits: model.traits.clone(),
        capabilities: model.capabilities.clone(),
        price_unit: model.unit.as_str().to_string(),
        price_description: price_description(model),
        credits_per_million_seconds: model.credits_per_million_seconds,
        input_credits_per_million_tokens: model.input_credits_per_million_tokens,
        output_credits_per_million_tokens: model.output_credits_per_million_tokens,
        service_available,
    }
}

fn price_description(model: &ModelPriceConfig) -> String {
    // `pricing` may contain raw upstream metadata. The user-facing description
    // must come from Clovy's configured credit price.
    match model.unit {
        clovy_config::PriceUnit::Seconds => format!(
            "{} per second audio",
            model.credits_per_million_seconds.map_or_else(
                || "$0.00".to_string(),
                |credits| format_credits_as_usd_per_unit(credits, 1_000_000)
            )
        ),
        clovy_config::PriceUnit::Tokens => format!(
            "{} input / {} output per 1M tokens",
            format_credits_as_usd(model.input_credits_per_million_tokens.unwrap_or_default()),
            format_credits_as_usd(model.output_credits_per_million_tokens.unwrap_or_default())
        ),
    }
}

fn format_credits_as_usd(credits: u64) -> String {
    let cents = (u128::from(credits) + 5) / 10;
    format!("${}.{:02}", cents / 100, cents % 100)
}

fn format_credits_as_usd_per_unit(credits: u64, units: u64) -> String {
    if units == 0 {
        return "$0.00".to_string();
    }
    let micro_usd = (u128::from(credits) * 1_000 + (u128::from(units) / 2)) / u128::from(units);
    if micro_usd >= 1_000_000 {
        let cents = (micro_usd + 5_000) / 10_000;
        format!("${}.{:02}", cents / 100, cents % 100)
    } else {
        let decimals = format!("{micro_usd:06}");
        format!("$0.{}", decimals.trim_end_matches('0'))
    }
}

#[cfg(test)]
mod tests {
    use super::price_description;
    use clovy_config::{ModelPriceConfig, ModelProvider, ModelType, PriceUnit};
    use pretty_assertions::assert_eq;

    #[test]
    fn price_description_uses_clovy_credit_price_over_provider_display() {
        let model = ModelPriceConfig {
            unit: PriceUnit::Seconds,
            // gpt-4o-mini-transcribe's packaged price; also pins the display
            // string default_pricing() carries for the same model.
            credits_per_million_seconds: Some(50_000),
            input_credits_per_million_tokens: None,
            output_credits_per_million_tokens: None,
            provider: ModelProvider::Groq,
            model_type: ModelType::Asr,
            display_name: "ASR".to_string(),
            description: None,
            privacy: None,
            pricing: Some(serde_json::json!({ "display": "$0.001/sec audio" })),
            context_tokens: None,
            traits: Vec::new(),
            capabilities: Vec::new(),
        };

        assert_eq!(price_description(&model), "$0.00005 per second audio");
    }
}
