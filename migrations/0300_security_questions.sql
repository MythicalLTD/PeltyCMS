-- WHMCS security questions: staff-defined questions; each user may store one
-- hashed answer, which staff can check when verifying a caller by phone.
CREATE TABLE security_questions (
    id         UUID PRIMARY KEY,
    question   TEXT NOT NULL CHECK (length(question) BETWEEN 1 AND 200),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE user_security_answers (
    user_id     UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    question_id UUID NOT NULL REFERENCES security_questions(id) ON DELETE CASCADE,
    answer_hash TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
