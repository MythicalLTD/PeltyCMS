-- "My reviews", the customer delete cascade and the open-question throttle
-- filter by customer.
CREATE INDEX product_reviews_customer_idx ON product_reviews (customer_id);
CREATE INDEX product_questions_customer_idx ON product_questions (customer_id, product_id);
