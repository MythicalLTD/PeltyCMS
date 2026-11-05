//! Data feeds (WHMCS `feeds/productsinfo.php`, `domainprice.php`): tiny
//! public JavaScript snippets that print a product's name, description or
//! price on an external site via `<script src=...>`.
use crate::error::ApiResult;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use pelty_core::AppError;
use rust_decimal::Decimal;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/feeds/products/{slug}", get(product_feed))
        .route("/feeds/tlds/{tld}", get(tld_feed))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct FeedQuery {
    /// `name`, `description` or `price` (default `price`).
    pub field: Option<String>,
    pub currency: Option<String>,
    /// Billing cycle for `price` (default `monthly`).
    pub cycle: Option<String>,
}

/// `document.write("...")` with a JSON-escaped string that cannot close a
/// surrounding `<script>` tag.
pub fn js_write(text: &str) -> String {
    let lit = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
    let lit = lit.replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026").replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029");
    format!("document.write({lit});\n")
}

fn js(body: String) -> Response {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=300"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], body).into_response()
}

fn currency_of(q: &FeedQuery) -> ApiResult<String> {
    let c = q.currency.as_deref().unwrap_or("EUR").to_ascii_uppercase();
    if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_uppercase()) {
        return Err(AppError::Validation("error.invalid_currency").into());
    }
    Ok(c)
}

#[utoipa::path(get, path = "/api/v1/feeds/products/{slug}", params(FeedQuery), responses((status = 200, content_type = "application/javascript"), (status = 404), (status = 422)))]
pub async fn product_feed(State(s): State<AppState>, Path(slug): Path<String>, Query(q): Query<FeedQuery>) -> ApiResult<Response> {
    let row: Option<(uuid::Uuid, String, String)> = sqlx::query_as("SELECT id, name, description FROM products WHERE slug = $1 AND status = 'active'")
        .bind(&slug).fetch_optional(&s.pool).await.map_err(pelty_database::db_err)?;
    let Some((id, name, description)) = row else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    let text = match q.field.as_deref().unwrap_or("price") {
        "name" => name,
        "description" => description,
        "price" => {
            let cur = currency_of(&q)?;
            let cycle = q.cycle.as_deref().unwrap_or("monthly");
            let amount: Option<Decimal> = sqlx::query_scalar("SELECT amount FROM product_prices WHERE product_id = $1 AND currency = $2 AND cycle = $3")
                .bind(id).bind(&cur).bind(cycle).fetch_optional(&s.pool).await.map_err(pelty_database::db_err)?;
            let Some(a) = amount else { return Ok(StatusCode::NOT_FOUND.into_response()) };
            format!("{} {cur}", money(a))
        }
        _ => return Err(AppError::Validation("error.validation").into()),
    };
    Ok(js(js_write(&text)))
}

/// Registration price for one year of a TLD.
#[utoipa::path(get, path = "/api/v1/feeds/tlds/{tld}", params(FeedQuery), responses((status = 200, content_type = "application/javascript"), (status = 404), (status = 422)))]
pub async fn tld_feed(State(s): State<AppState>, Path(tld): Path<String>, Query(q): Query<FeedQuery>) -> ApiResult<Response> {
    let cur = currency_of(&q)?;
    let tld = tld.trim_start_matches('.').to_ascii_lowercase();
    let amount: Option<Decimal> = sqlx::query_scalar(TLD_SQL).bind(&tld).bind(&cur).fetch_optional(&s.pool).await.map_err(pelty_database::db_err)?;
    let Some(a) = amount else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    Ok(js(js_write(&format!("{} {cur}", money(a)))))
}

const TLD_SQL: &str = "SELECT register_price FROM tld_pricing WHERE tld = $1 AND currency = $2 AND active";

/// Two decimals at least, trailing zeros beyond that trimmed (exact Decimal).
pub fn money(a: Decimal) -> String {
    let n = a.normalize();
    if n.scale() < 2 { format!("{:.2}", n) } else { n.to_string() }
}

#[cfg(test)]
mod tests {
    use super::{js_write, money};
    #[test]
    fn escapes_and_formats() {
        assert_eq!(js_write("a</script><b>\"x\""), "document.write(\"a\\u003c/script\\u003e\\u003cb\\u003e\\\"x\\\"\");\n");
        assert_eq!(money("9.9".parse().unwrap()), "9.90");
        assert_eq!(money("10.0000".parse().unwrap()), "10.00");
        assert_eq!(money("1.2345".parse().unwrap()), "1.2345");
    }
}
