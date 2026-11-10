//! Payout providers for creator payouts. `manual` marks the payout as
//! processing for staff to pay outside PeltyCMS. Stripe Connect and PayPal
//! Payouts are interface stubs: they report "not configured" until a real
//! integration implements [`PayoutProvider::send`].
use pelty_database::marketplace::{Creator, Payout};
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a provider did with a payout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// Money moved; reference from the provider.
    Paid(String),
    /// Accepted, settles later (or staff pays by hand).
    Processing(String),
}

pub trait PayoutProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Whether credentials exist for this provider.
    fn configured(&self) -> bool;
    /// Sends the payout. `Err` is an error code (`error.*`).
    fn send<'a>(
        &'a self,
        creator: &'a Creator,
        payout: &'a Payout,
    ) -> BoxFuture<'a, Result<Sent, &'static str>>;
}

pub struct Manual;
impl PayoutProvider for Manual {
    fn id(&self) -> &'static str {
        "manual"
    }
    fn configured(&self) -> bool {
        true
    }
    fn send<'a>(
        &'a self,
        _c: &'a Creator,
        _p: &'a Payout,
    ) -> BoxFuture<'a, Result<Sent, &'static str>> {
        Box::pin(async { Ok(Sent::Processing(String::new())) })
    }
}

/// Stripe Connect transfer (stub). Needs a platform key and the creator's
/// connected account id (`payout_details.stripe_account`).
pub struct StripeConnect {
    pub key_present: bool,
}
impl PayoutProvider for StripeConnect {
    fn id(&self) -> &'static str {
        "stripe_connect"
    }
    fn configured(&self) -> bool {
        self.key_present
    }
    fn send<'a>(
        &'a self,
        c: &'a Creator,
        _p: &'a Payout,
    ) -> BoxFuture<'a, Result<Sent, &'static str>> {
        Box::pin(async move {
            if !self.key_present
                || c.payout_details
                    .get("stripe_account")
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty)
            {
                return Err("error.payout_provider_unconfigured");
            }
            // Integration point: POST /v1/transfers {amount, currency, destination}.
            Err("error.payout_provider_stub")
        })
    }
}

/// PayPal Payouts (stub). Needs API credentials and `payout_details.paypal_email`.
pub struct PayPal {
    pub key_present: bool,
}
impl PayoutProvider for PayPal {
    fn id(&self) -> &'static str {
        "paypal"
    }
    fn configured(&self) -> bool {
        self.key_present
    }
    fn send<'a>(
        &'a self,
        c: &'a Creator,
        _p: &'a Payout,
    ) -> BoxFuture<'a, Result<Sent, &'static str>> {
        Box::pin(async move {
            if !self.key_present
                || c.payout_details
                    .get("paypal_email")
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty)
            {
                return Err("error.payout_provider_unconfigured");
            }
            // Integration point: POST /v1/payments/payouts (sender_batch_header + items).
            Err("error.payout_provider_stub")
        })
    }
}

/// Provider for a method; credentials are read from the environment.
pub fn provider(method: &str) -> Option<Box<dyn PayoutProvider>> {
    let env = |k: &str| std::env::var(k).is_ok_and(|v| !v.trim().is_empty());
    match method {
        "manual" => Some(Box::new(Manual)),
        "stripe_connect" => Some(Box::new(StripeConnect {
            // Stripe Connect secret key for creator payouts. The provider is a
            // stub in this release: with it set, payouts still answer `payout_provider_stub`.
            key_present: env("PELTY_STRIPE_CONNECT_KEY"),
        })),
        "paypal" => Some(Box::new(PayPal {
            // PayPal Payouts client id for creator payouts. The provider is a
            // stub in this release: with it set, payouts still answer `payout_provider_stub`.
            key_present: env("PELTY_PAYPAL_PAYOUTS_CLIENT_ID"),
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_exist_for_every_method() {
        for m in pelty_database::marketplace::PAYOUT_METHODS {
            assert_eq!(provider(m).unwrap().id(), *m);
        }
        assert!(provider("bitcoin").is_none());
        assert!(provider("manual").unwrap().configured());
    }
}
