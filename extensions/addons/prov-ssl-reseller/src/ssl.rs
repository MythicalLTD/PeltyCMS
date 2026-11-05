//! SSL certificate ordering through a reseller REST API (Sectigo/GoGetSSL
//! style): create = place an order (CSR + DCV method), suspend/unsuspend are
//! no-ops (certificates cannot be paused), terminate = cancel/revoke.
//! Custom buttons: status, resend validation email, download, reissue.
//! Contract (`/api/v1`): `POST /orders`, `GET /orders/{id}`,
//! `POST /orders/{id}/{cancel|resend|reissue}`, `GET /orders/{id}/certificate`.
//! A direct RFC 8555 ACME client is not included.
use pelty_provisioning::http::call;
use pelty_provisioning::http::Method;
use pelty_provisioning::util::domain;
use pelty_provisioning::{
    cfg_str, cfg_u64_or, fld, remote_id, BoxFuture, Created, ModuleInfo, ProvisionError,
    ProvisionResult, ProvisioningModule, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

const J: &str = "application/json";

#[derive(Default)]
pub struct SslReseller;

fn oid(svc: &ServiceCtx) -> ProvisionResult<&str> {
    let id = remote_id(svc)?;
    if !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(id)
    } else {
        Err(ProvisionError::Config("invalid order id".into()))
    }
}

impl ProvisioningModule for SslReseller {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "ssl_reseller",
            name: "SSL certificates (reseller API)",
            server_fields: vec![],
            product_fields: vec![
                fld!("ssl_product", "text", true),
                fld!("validity_months", "number", false, 12),
                fld!("dcv_method", "text", false, "email"),
                fld!("san_count", "number", false, 0),
            ],
            actions: vec![],
        }
    }
    fn checkout_fields(&self) -> Vec<pelty_provisioning::CheckoutField> {
        vec![
            pelty_provisioning::CheckoutField {
                key: "common_name",
                name: "Domain (common name)",
                description: "The domain the certificate is issued for, e.g. example.com",
                kind: "text",
                choices: &[],
                pattern: r"^(\*\.)?([a-zA-Z0-9-]{1,63}\.)+[a-zA-Z]{2,63}$",
                required: true,
            },
            pelty_provisioning::CheckoutField {
                key: "csr",
                name: "CSR (optional)",
                description:
                    "Paste a PEM certificate signing request, or leave empty to generate one later.",
                kind: "textarea",
                choices: &[],
                pattern: "",
                required: false,
            },
        ]
    }
    fn custom_actions(&self) -> &'static [&'static str] {
        &["status", "resend_validation", "download", "reissue"]
    }
    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            call(server, Method::GET, "/api/v1/account", J, None)
                .await
                .map(|_| ())
        })
    }
    fn create<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Created>> {
        Box::pin(async move {
            if let Some(id) = &svc.remote_id {
                return Ok(Created {
                    remote_id: id.clone(),
                    module_data: svc.module_data.clone(),
                });
            }
            let c = &svc.product_config;
            let d = domain(svc)?;
            let csr = svc
                .custom_fields
                .get("csr")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !csr.is_empty() && !csr.contains("BEGIN CERTIFICATE REQUEST") {
                return Err(ProvisionError::Config(
                    "custom field csr is not a PEM certificate request".into(),
                ));
            }
            let approver = svc
                .custom_fields
                .get("approver_email")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(format!("admin@{d}"));
            let body = json!({
                "product": cfg_str(c, "ssl_product")?, "period": cfg_u64_or(c, "validity_months", 12)?, "domain": d, "csr": csr,
                "dcv_method": c.get("dcv_method").and_then(Value::as_str).unwrap_or("email"), "approver_email": approver,
                "san_count": cfg_u64_or(c, "san_count", 0)?, "contact_email": svc.customer_email, "external_id": format!("pelty-{}", svc.service_id),
            });
            let r = call(server, Method::POST, "/api/v1/orders", J, Some(&body)).await?;
            let id = r
                .get("id")
                .map(|v| v.to_string().trim_matches('"').to_string())
                .filter(|s| !s.is_empty() && s != "null")
                .ok_or_else(|| ProvisionError::Transient("reseller returned no order id".into()))?;
            Ok(Created {
                remote_id: id,
                module_data: json!({ "domain": d, "status": r.get("status"), "dcv": r.get("dcv") }),
            })
        })
    }
    fn suspend<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn unsuspend<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn terminate<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            pelty_provisioning::util::gone_ok(
                call(
                    server,
                    Method::POST,
                    &format!("/api/v1/orders/{}/cancel", oid(svc)?),
                    J,
                    None,
                )
                .await,
            )
        })
    }
    fn change_package<'a>(
        &'a self,
        _s: &'a ServerConn,
        _svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async {
            Err(ProvisionError::Config(
                "certificates cannot change product; order a new one".into(),
            ))
        })
    }
    fn custom<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
        action: &'a str,
        params: &'a Value,
    ) -> BoxFuture<'a, ProvisionResult<Value>> {
        Box::pin(async move {
            let id = oid(svc)?;
            match action {
                "status" => {
                    let r = call(
                        server,
                        Method::GET,
                        &format!("/api/v1/orders/{id}"),
                        J,
                        None,
                    )
                    .await?;
                    Ok(
                        json!({ "status": r.get("status"), "valid_until": r.get("valid_until"), "dcv": r.get("dcv"), "module_data_patch": { "status": r.get("status") } }),
                    )
                }
                "resend_validation" => call(
                    server,
                    Method::POST,
                    &format!("/api/v1/orders/{id}/resend"),
                    J,
                    None,
                )
                .await
                .map(|_| json!({ "ok": true })),
                "download" => {
                    let r = call(
                        server,
                        Method::GET,
                        &format!("/api/v1/orders/{id}/certificate"),
                        J,
                        None,
                    )
                    .await?;
                    Ok(
                        json!({ "certificate": r.get("certificate"), "ca_bundle": r.get("ca_bundle") }),
                    )
                }
                "reissue" => {
                    let csr = params
                        .get("csr")
                        .and_then(Value::as_str)
                        .filter(|c| c.contains("BEGIN CERTIFICATE REQUEST"))
                        .ok_or_else(|| ProvisionError::Config("a PEM CSR is required".into()))?;
                    call(
                        server,
                        Method::POST,
                        &format!("/api/v1/orders/{id}/reissue"),
                        J,
                        Some(&json!({ "csr": csr })),
                    )
                    .await
                    .map(|_| json!({ "ok": true }))
                }
                _ => Err(ProvisionError::Config(format!("unknown action {action}"))),
            }
        })
    }
}
