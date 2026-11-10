-- Live creator payouts: Stripe Connect (Express) onboarding and transfers,
-- PayPal Payouts batches, payout scheduling/approval, transfer reversals on
-- refunds, provider webhook dedupe, and multi-image resource galleries.
ALTER TABLE creators
    ADD COLUMN stripe_account_id    TEXT NOT NULL DEFAULT '' CHECK (length(stripe_account_id) <= 64),
    ADD COLUMN stripe_status        TEXT NOT NULL DEFAULT 'none'
        CHECK (stripe_status IN ('none', 'onboarding', 'restricted', 'enabled', 'disconnected')),
    ADD COLUMN stripe_status_detail TEXT NOT NULL DEFAULT '' CHECK (length(stripe_status_detail) <= 500),
    ADD COLUMN stripe_updated_at    TIMESTAMPTZ,
    ADD COLUMN paypal_email         TEXT NOT NULL DEFAULT '' CHECK (length(paypal_email) <= 254),
    -- Set by staff: the creator cannot change the payout method.
    ADD COLUMN payout_method_locked BOOLEAN NOT NULL DEFAULT false;
CREATE UNIQUE INDEX creators_stripe_account ON creators (stripe_account_id) WHERE stripe_account_id <> '';

UPDATE creators SET paypal_email = payout_details->>'paypal_email'
 WHERE payout_details ? 'paypal_email' AND length(payout_details->>'paypal_email') <= 254;

ALTER TABLE creator_ledger DROP CONSTRAINT creator_ledger_kind_check;
ALTER TABLE creator_ledger ADD CONSTRAINT creator_ledger_kind_check
    CHECK (kind IN ('sale', 'refund', 'payout', 'payout_reversal', 'adjustment', 'transfer_reversal'));

ALTER TABLE creator_payouts
    -- PayPal: `pending:<sender_batch_id>` until PayPal answers, then its payout_batch_id.
    ADD COLUMN batch_id TEXT NOT NULL DEFAULT '' CHECK (length(batch_id) <= 120),
    -- Stripe: amount pulled back from the transfer (refunds after payout).
    ADD COLUMN reversed NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (reversed >= 0),
    -- Created by the payout schedule rather than the creator.
    ADD COLUMN auto     BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX creator_payouts_open ON creator_payouts (method, status) WHERE status IN ('requested', 'processing');

-- Provider webhook events already applied (dedupe).
CREATE TABLE creator_payout_events (
    provider    TEXT NOT NULL,
    event_id    TEXT NOT NULL CHECK (length(event_id) <= 200),
    kind        TEXT NOT NULL DEFAULT '' CHECK (length(kind) <= 100),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, event_id)
);

-- Stripe transfer reversals booked to the ledger (dedupe by reversal id).
CREATE TABLE creator_transfer_reversals (
    reversal_id TEXT PRIMARY KEY CHECK (length(reversal_id) <= 100),
    payout_id   UUID NOT NULL REFERENCES creator_payouts(id) ON DELETE CASCADE,
    amount      NUMERIC(19,4) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Extra screenshots for a product (marketplace resource gallery).
CREATE TABLE product_gallery (
    id         UUID PRIMARY KEY,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    url        TEXT NOT NULL CHECK (length(url) BETWEEN 1 AND 500),
    caption    TEXT NOT NULL DEFAULT '' CHECK (length(caption) <= 200),
    sort_order INT NOT NULL DEFAULT 0
);
CREATE INDEX product_gallery_product ON product_gallery (product_id, sort_order);
