//! Daily exchange-rate update: fetch the configured feed through the
//! swappable HTTP transport, apply rates (+ markup) and optionally re-sync
//! prices. Admin routes live in `routes::currencies`.
use crate::AppState;
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, currencies, exchange_rates as er, jobs};
use pelty_payments::HttpRequestSpec;
use serde_json::{json, Value};
use time::OffsetDateTime;

pub const JOB_KIND: &str = "currency.update_rates";

/// Fetch + apply now. Errors are recorded in the settings row.
pub async fn update_now(s: &AppState, actor: Option<uuid::Uuid>) -> AppResult<Value> {
    let cfg = er::settings(&s.pool).await?;
    let res = fetch_and_apply(s, &cfg).await;
    er::record_run(
        &s.pool,
        res.as_ref().err().map(|e| e.to_string()).as_deref(),
    )
    .await?;
    let applied = res?;
    let mut synced = 0;
    if cfg.update_prices && !applied.updated.is_empty() {
        synced = currencies::sync_prices(&s.pool, false).await?;
    }
    audit::record(
        &s.pool,
        actor,
        "currency.rates_updated",
        None,
        None,
        json!({ "updated": applied.updated, "missing": applied.missing, "prices": synced }),
    )
    .await?;
    Ok(json!({ "updated": applied.updated, "missing": applied.missing, "prices_synced": synced }))
}

async fn fetch_and_apply(s: &AppState, cfg: &er::RateSettings) -> AppResult<er::Applied> {
    let t = s.http();
    let (status, body) = t
        .send(HttpRequestSpec {
            method: "GET",
            url: cfg.url.clone(),
            headers: vec![("accept".into(), "application/xml, application/json".into())],
            body: String::new(),
        })
        .await
        .map_err(|_| AppError::Conflict("error.rate_feed_unavailable"))?;
    if !(200..300).contains(&status) {
        return Err(AppError::Conflict("error.rate_feed_unavailable"));
    }
    let feed = if cfg.source == "ecb" {
        er::parse_ecb(&body)?
    } else {
        er::parse_json(&body)?
    };
    er::apply(&s.pool, &feed, cfg.markup_percent, &cfg.source).await
}

/// Enqueue today's update when the feed is enabled (deduplicated per day).
pub async fn schedule(s: &AppState) -> AppResult<Option<i64>> {
    if !er::settings(&s.pool).await?.enabled {
        return Ok(None);
    }
    let key = format!("{JOB_KIND}:{}", OffsetDateTime::now_utc().date());
    jobs::enqueue(&s.pool, JOB_KIND, json!({}), None, Some(&key), 3).await
}
