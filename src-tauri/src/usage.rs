//! Weekly usage fetch via Grok CLI chat proxy billing endpoint.

use crate::auth::{AccountTokens, TOKEN_HEADER};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub const BILLING_BASE: &str = "https://cli-chat-proxy.grok.com/v1";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Error)]
pub enum UsageError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("usage fetch failed: {0}")]
    Message(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageData {
    pub percentage: f64,
    pub breakdown: HashMap<String, f64>,
    pub reset_at: Option<DateTime<Utc>>,
    pub period_start: Option<DateTime<Utc>>,
    pub period_type: Option<String>,
    pub extra_credits_cents: Option<i64>,
    pub on_demand_used_cents: Option<i64>,
    pub on_demand_cap_cents: Option<i64>,
    pub subscription_tier: Option<String>,
    pub is_unified: Option<bool>,
    pub fetched_at: DateTime<Utc>,
    pub account_id: String,
    pub account_email: Option<String>,
    pub account_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cent {
    #[serde(default)]
    val: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsagePeriod {
    #[serde(rename = "type", default)]
    period_type: Option<String>,
    start: Option<String>,
    end: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProductUsage {
    product: Option<String>,
    usage_percent: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingConfig {
    credit_usage_percent: Option<f64>,
    current_period: Option<UsagePeriod>,
    monthly_limit: Option<Cent>,
    used: Option<Cent>,
    on_demand_cap: Option<Cent>,
    on_demand_used: Option<Cent>,
    prepaid_balance: Option<Cent>,
    is_unified_billing_user: Option<bool>,
    billing_period_start: Option<String>,
    billing_period_end: Option<String>,
    #[serde(default)]
    product_usage: Vec<ProductUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingResponse {
    config: Option<BillingConfig>,
    #[serde(default)]
    subscription_tier: Option<String>,
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn friendly_product(name: &str) -> String {
    match name {
        "GrokBuild" | "PRODUCT_GROK_BUILD" => "Build".into(),
        "GrokChat" | "PRODUCT_GROK_CHAT" => "Chat".into(),
        "GrokImagine" | "PRODUCT_GROK_IMAGINE" => "Imagine".into(),
        "GrokVoice" | "PRODUCT_GROK_VOICE" => "Voice".into(),
        "GrokApi" | "PRODUCT_GROK_API" => "API".into(),
        other => other.trim_start_matches("PRODUCT_GROK_").to_string(),
    }
}

/// Parse a billing API JSON body into `UsageData` (pure; used by fetch + tests).
fn parse_billing_response(
    billing: BillingResponse,
    account: &AccountTokens,
) -> Result<UsageData, UsageError> {
    let config = billing
        .config
        .ok_or_else(|| UsageError::Message("Empty billing config".into()))?;

    // Prefer credit_usage_percent; fall back to used/monthly_limit.
    let percentage = if let Some(p) = config.credit_usage_percent {
        p
    } else {
        match (config.used.as_ref(), config.monthly_limit.as_ref()) {
            (Some(u), Some(lim)) if lim.val > 0 => (u.val as f64 / lim.val as f64) * 100.0,
            _ => 0.0,
        }
    };

    let mut breakdown = HashMap::new();
    for p in &config.product_usage {
        if let (Some(name), Some(pct)) = (&p.product, p.usage_percent) {
            breakdown.insert(friendly_product(name), pct);
        }
    }

    let (period_start, reset_at, period_type) = if let Some(period) = &config.current_period {
        (
            period.start.as_deref().and_then(parse_ts),
            period.end.as_deref().and_then(parse_ts),
            period.period_type.clone(),
        )
    } else {
        (
            config.billing_period_start.as_deref().and_then(parse_ts),
            config.billing_period_end.as_deref().and_then(parse_ts),
            None,
        )
    };

    Ok(UsageData {
        percentage,
        breakdown,
        reset_at,
        period_start,
        period_type,
        extra_credits_cents: config.prepaid_balance.map(|c| c.val),
        on_demand_used_cents: config.on_demand_used.map(|c| c.val),
        on_demand_cap_cents: config.on_demand_cap.map(|c| c.val),
        subscription_tier: billing.subscription_tier,
        is_unified: config.is_unified_billing_user,
        fetched_at: Utc::now(),
        account_id: account.user_id.clone(),
        account_email: account.email.clone(),
        account_name: account.display_name.clone(),
    })
}

/// Parse raw JSON text from the billing endpoint.
pub fn parse_billing_json(json: &str, account: &AccountTokens) -> Result<UsageData, UsageError> {
    let billing: BillingResponse = serde_json::from_str(json)
        .map_err(|e| UsageError::Message(format!("Failed to parse billing data: {e}")))?;
    parse_billing_response(billing, account)
}

/// Fetch weekly usage for an authenticated account.
pub async fn fetch_usage(account: &AccountTokens) -> Result<UsageData, UsageError> {
    let url = format!("{BILLING_BASE}/billing?format=credits");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", account.access_token))
        .header("X-XAI-Token-Auth", TOKEN_HEADER)
        .header("x-userid", &account.user_id)
        .header("x-grok-client-version", CLIENT_VERSION)
        .header("x-grok-client-mode", "desktop")
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(UsageError::Message(format!(
            "Billing service error: {detail}"
        )));
    }

    let body = resp.text().await?;
    parse_billing_json(&body, account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{XAI_CLIENT_ID, XAI_ISSUER};

    fn sample_account() -> AccountTokens {
        AccountTokens {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: None,
            user_id: "user-1".into(),
            email: Some("a@example.com".into()),
            display_name: Some("A".into()),
            client_id: XAI_CLIENT_ID.into(),
            issuer: XAI_ISSUER.into(),
            source: "test".into(),
        }
    }

    #[test]
    fn parses_credits_config_shape() {
        let json = r#"{
            "config": {
                "creditUsagePercent": 95.0,
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "start": "2026-07-20T04:25:30.945578+00:00",
                    "end": "2026-07-27T04:25:30.945578+00:00"
                },
                "onDemandCap": {"val": 0},
                "onDemandUsed": {"val": 0},
                "productUsage": [
                    {"product": "GrokBuild", "usagePercent": 91.0},
                    {"product": "GrokChat", "usagePercent": 4.0}
                ],
                "isUnifiedBillingUser": true,
                "prepaidBalance": {"val": 0}
            }
        }"#;
        let usage = parse_billing_json(json, &sample_account()).expect("parse");
        assert_eq!(usage.percentage, 95.0);
        assert_eq!(usage.breakdown.get("Build"), Some(&91.0));
        assert_eq!(usage.breakdown.get("Chat"), Some(&4.0));
        assert_eq!(usage.is_unified, Some(true));
        assert!(usage.reset_at.is_some());
        assert_eq!(usage.account_id, "user-1");
    }

    #[test]
    fn falls_back_to_used_over_limit() {
        let json = r#"{
            "config": {
                "monthlyLimit": {"val": 2000},
                "used": {"val": 500},
                "billingPeriodEnd": "2026-08-01T00:00:00Z"
            }
        }"#;
        let usage = parse_billing_json(json, &sample_account()).expect("parse");
        assert!((usage.percentage - 25.0).abs() < 0.001);
        assert!(usage.reset_at.is_some());
    }

    #[test]
    fn empty_config_errors() {
        let err = parse_billing_json(r#"{"config":null}"#, &sample_account()).unwrap_err();
        assert!(err.to_string().contains("Empty"));
    }
}
