//! Glue between payments / the scheduler and plan changes & cancellations.
use crate::AppState;
use time::OffsetDateTime;
use uuid::Uuid;

/// Apply upgrades paid by this invoice. Failures are logged and never block
/// the payment; the daily sweep re-checks paid upgrades as a fallback.
pub async fn after_payment(s: &AppState, invoice_id: Uuid) {
    crate::overdue::after_payment(s, invoice_id).await;
    match pelty_database::service_changes::after_payment(&s.pool, invoice_id).await {
        Ok(ids) => {
            for id in ids {
                tracing::info!(service_id = %id, %invoice_id, "plan upgrade applied");
                crate::provision::on_plan_change(s, id).await;
            }
        }
        Err(e) => tracing::error!(error = %e, %invoice_id, "applying paid plan change failed"),
    }
}

/// Daily work: scheduled downgrades and effective end-of-period cancellations.
pub async fn sweep(s: &AppState) -> pelty_core::AppResult<()> {
    let today = OffsetDateTime::now_utc().date();
    let (changed, cancel) = pelty_database::service_changes::sweep(&s.pool, today).await?;
    for id in changed {
        tracing::info!(service_id = %id, "scheduled downgrade applied");
        crate::provision::on_plan_change(s, id).await;
    }
    for id in cancel {
        if let Err(crate::error::ApiError(e)) = crate::routes::service_changes::cancel_service(s, id, None).await {
            tracing::error!(error = %e, service_id = %id, "scheduled cancellation failed");
        }
    }
    Ok(())
}
