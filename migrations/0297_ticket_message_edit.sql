-- Staff can edit or delete ticket replies (WHMCS UpdateTicketReply / DeleteTicketReply).
ALTER TABLE ticket_messages ADD COLUMN edited_at TIMESTAMPTZ;
