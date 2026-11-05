//! Business logic for register/login/logout. HTTP-agnostic.
use crate::{password, token};
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, sessions, users, Pool};
use serde_json::json;
use time::Duration;
use uuid::Uuid;

pub struct RegisterInput<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub display_name: &'a str,
}
pub struct LoginInput<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub ip: Option<&'a str>,
    pub user_agent: Option<&'a str>,
}

pub struct IssuedSession {
    pub token: String,
    pub csrf_token: String,
    pub session: sessions::Session,
    pub user: users::User,
}

#[derive(Clone)]
pub struct AuthService {
    pub pool: Pool,
    pub idle: Duration,
    pub absolute: Duration,
}

pub fn normalize_email(e: &str) -> AppResult<String> {
    let e = e.trim().to_lowercase();
    let ok = e.len() <= 254
        && e.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
        })
        && !e.chars().any(char::is_whitespace);
    if ok {
        Ok(e)
    } else {
        Err(AppError::Validation("error.invalid_email"))
    }
}

impl AuthService {
    pub async fn register(
        &self,
        i: RegisterInput<'_>,
        role: &str,
        ip: Option<&str>,
    ) -> AppResult<users::User> {
        let email = normalize_email(i.email)?;
        crate::policy::load(&self.pool)
            .await?
            .check_password(i.password, &email)?;
        let name = i.display_name.trim();
        if name.chars().count() > 100 {
            return Err(AppError::Validation("error.name_too_long"));
        }
        let hash = password::hash(i.password)?;
        let user = users::create(&self.pool, &email, &hash, name, role)
            .await
            .map_err(|e| match e {
                AppError::Conflict(_) => AppError::Conflict("error.email_taken"),
                e => e,
            })?;
        audit::record(
            &self.pool,
            Some(user.id),
            "auth.register",
            Some(&user.id.to_string()),
            ip,
            json!({ "role": role }),
        )
        .await?;
        Ok(user)
    }

    pub async fn login(&self, i: LoginInput<'_>) -> AppResult<IssuedSession> {
        let user = self.check_credentials(&i).await?;
        self.issue_session(user, i.ip, i.user_agent).await
    }

    /// Password step only; failures are audited and timing-equalised.
    pub async fn check_credentials(&self, i: &LoginInput<'_>) -> AppResult<users::User> {
        let email = i.email.trim().to_lowercase();
        let user = users::find_by_email(&self.pool, &email).await?;
        let user = match user {
            // Locked accounts answer exactly like a wrong password (no
            // enumeration) and don't extend the lock.
            Some(u) if users::is_locked(&self.pool, u.id).await? => {
                password::dummy_verify(i.password);
                audit::record(
                    &self.pool,
                    Some(u.id),
                    "auth.login_locked",
                    None,
                    i.ip,
                    json!({}),
                )
                .await?;
                return Err(AppError::InvalidCredentials);
            }
            Some(u) if password::verify(i.password, &u.password_hash) && u.is_active => {
                users::login_succeeded(&self.pool, u.id).await?;
                // Imported (WHMCS) hash verified: store a native Argon2id hash.
                if password::needs_rehash(&u.password_hash) {
                    users::set_password(&self.pool, u.id, &password::hash(i.password)?).await?;
                }
                u
            }
            Some(u) => {
                let (failures, locked) = users::login_failed(&self.pool, u.id).await?;
                audit::record(
                    &self.pool,
                    Some(u.id),
                    "auth.login_failed",
                    None,
                    i.ip,
                    json!({ "failures": failures, "user_agent": i.user_agent.map(|u| u.chars().take(255).collect::<String>()) }),
                )
                .await?;
                if let Some(minutes) = locked {
                    audit::record(
                        &self.pool,
                        Some(u.id),
                        "auth.account_locked",
                        None,
                        i.ip,
                        json!({ "failures": failures, "minutes": minutes }),
                    )
                    .await?;
                    if failures == users::LOCK_AFTER {
                        self.notify_admins_locked(&u.email, i.ip, failures, minutes)
                            .await;
                    }
                }
                return Err(AppError::InvalidCredentials);
            }
            None => {
                password::dummy_verify(i.password);
                audit::record(
                    &self.pool,
                    None,
                    "auth.login_failed",
                    None,
                    i.ip,
                    json!({ "unknown_user": true }),
                )
                .await?;
                return Err(AppError::InvalidCredentials);
            }
        };
        Ok(user)
    }

    /// Queue an `admin_account_locked` email to every admin (first lock of a
    /// streak only). Best effort: failures are logged, never block login flow.
    async fn notify_admins_locked(
        &self,
        email: &str,
        ip: Option<&str>,
        failures: i32,
        minutes: i32,
    ) {
        let admins = match users::admin_emails(&self.pool).await {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(error = %e, "admin lookup for lockout notice failed");
                return;
            }
        };
        for to in admins {
            let payload = json!({ "template": "admin_account_locked", "locale": "en", "to": to,
                "vars": { "email": email, "ip": ip.unwrap_or("unknown"), "attempts": failures.to_string(), "minutes": minutes.to_string() } });
            if let Err(e) =
                pelty_database::jobs::enqueue(&self.pool, "email.send", payload, None, None, 6)
                    .await
            {
                tracing::error!(error = %e, "could not queue lockout notice");
            }
        }
    }

    /// Create a fresh session (rotation; no fixation) after all factors passed.
    pub async fn issue_session(
        &self,
        user: users::User,
        ip: Option<&str>,
        user_agent: Option<&str>,
    ) -> AppResult<IssuedSession> {
        let i = LoginInput {
            email: "",
            password: "",
            ip,
            user_agent,
        };
        let pol = crate::policy::load(&self.pool).await?;
        let idle = pol
            .session_idle_minutes
            .map(time::Duration::minutes)
            .unwrap_or(self.idle);
        let absolute = pol
            .session_absolute_hours
            .map(time::Duration::hours)
            .unwrap_or(self.absolute);
        // Always a fresh token on login (session rotation; no fixation).
        let tok = token::generate();
        let csrf = token::generate();
        let hash = token::hash(&tok);
        let session = sessions::create(
            &self.pool,
            sessions::NewSession {
                user_id: user.id,
                token_hash: &hash,
                csrf_token: &csrf,
                ip: i.ip,
                user_agent: i.user_agent,
                idle,
                absolute,
            },
        )
        .await?;
        audit::record(
            &self.pool,
            Some(user.id),
            "auth.login",
            Some(&session.id.to_string()),
            i.ip,
            json!({ "user_agent": i.user_agent.map(|u| u.chars().take(255).collect::<String>()) }),
        )
        .await?;
        Ok(IssuedSession {
            token: tok,
            csrf_token: csrf,
            session,
            user,
        })
    }

    pub async fn authenticate(&self, tok: &str) -> AppResult<Option<sessions::Session>> {
        let idle = crate::policy::load(&self.pool)
            .await?
            .session_idle_minutes
            .map(time::Duration::minutes)
            .unwrap_or(self.idle);
        sessions::touch(&self.pool, &token::hash(tok), idle).await
    }

    pub async fn revoke(
        &self,
        user: Uuid,
        session: Uuid,
        ip: Option<&str>,
        action: &str,
    ) -> AppResult<bool> {
        let ok = sessions::revoke(&self.pool, user, session).await?;
        if ok {
            audit::record(
                &self.pool,
                Some(user),
                action,
                Some(&session.to_string()),
                ip,
                json!({}),
            )
            .await?;
        }
        Ok(ok)
    }
}
