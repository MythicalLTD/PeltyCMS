//! Generic webhook module: each action calls a configurable HTTP endpoint
//! with a templated body. Server config `actions` holds the defaults; a
//! product's `actions` overrides them per action. Every request carries
//! `X-Pelty-Signature: sha256=<hmac(api_key, body)>` and, when the API key
//! is set, `Authorization: Bearer <api_key>`.
//!
//! ```json
//! {"create": {"method": "POST", "path": "/services", "body": "{\"email\":\"{{email}}\"}", "remote_id_pointer": "/id"},
//!  "suspend": {"method": "POST", "path": "/services/{{remote_id}}/suspend"}}
//! ```
use pelty_provisioning::http::call_raw;
use pelty_provisioning::http::Method;
use pelty_provisioning::template::{render, sign, vars, Escape};
use pelty_provisioning::{
    BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError, ProvisionResult, ProvisioningModule,
    RemoteAccount, ServerConn, ServiceCtx,
};
use serde_json::{json, Value};

#[derive(Default)]
pub struct Webhook;

fn json_field(v: &Value, key: &str) -> ProvisionResult<Value> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(json!({})),
        Some(Value::String(s)) => serde_json::from_str(s)
            .map_err(|_| ProvisionError::Config(format!("{key} must be a JSON object"))),
        Some(o @ Value::Object(_)) => Ok(o.clone()),
        _ => Err(ProvisionError::Config(format!(
            "{key} must be a JSON object"
        ))),
    }
}

/// The endpoint for `action`: product override, else server default.
fn endpoint(action: &str, server: &ServerConn, svc: &ServiceCtx) -> ProvisionResult<Value> {
    let p = json_field(&svc.product_config, "actions")?;
    let s = json_field(&server.config, "actions")?;
    p.get(action)
        .or_else(|| s.get(action))
        .filter(|v| v.is_object())
        .cloned()
        .ok_or_else(|| ProvisionError::Config(format!("webhook action {action} is not configured")))
}

async fn fire(action: &str, server: &ServerConn, svc: &ServiceCtx) -> ProvisionResult<Value> {
    let ep = endpoint(action, server, svc)?;
    let v = vars(action, svc);
    let method = ep
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("POST")
        .to_ascii_uppercase();
    let method = Method::from_bytes(method.as_bytes())
        .map_err(|_| ProvisionError::Config("invalid webhook method".into()))?;
    let path = render(
        ep.get("path").and_then(Value::as_str).unwrap_or(""),
        &v,
        Escape::Url,
    );
    if !path.is_empty() && !path.starts_with('/') {
        return Err(ProvisionError::Config(
            "webhook path must start with /".into(),
        ));
    }
    let body = match ep.get("body").and_then(Value::as_str) {
        Some(t) => render(t, &v, Escape::Json),
        None => json!({ "action": action, "service_id": svc.service_id, "remote_id": svc.remote_id, "email": svc.customer_email }).to_string(),
    };
    let sig = format!("sha256={}", sign(&server.api_key, body.as_bytes()));
    let auth = format!("Bearer {}", server.api_key);
    let mut headers = vec![
        ("accept", "application/json"),
        ("x-pelty-action", action),
        ("x-pelty-signature", sig.as_str()),
    ];
    if !server.api_key.is_empty() {
        headers.push(("authorization", auth.as_str()));
    }
    let send_body = if method == Method::GET {
        None
    } else {
        Some(("application/json", body))
    };
    call_raw(server, method, &path, &headers, send_body)
        .await
        .map(|r| json!({ "endpoint": ep, "response": r }))
}

impl ProvisioningModule for Webhook {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            actions: vec![],
            id: "webhook",
            name: "Webhook",
            server_fields: vec![FieldSpec {
                key: "actions",
                label: "provisioning.field.webhook_actions",
                kind: "json",
                required: true,
                default: None,
            }],
            product_fields: vec![FieldSpec {
                key: "actions",
                label: "provisioning.field.webhook_actions",
                kind: "json",
                required: false,
                default: None,
            }],
        }
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let s = json_field(&server.config, "actions")?;
            let path = s
                .pointer("/test/path")
                .and_then(Value::as_str)
                .unwrap_or("/");
            let sig = format!("sha256={}", sign(&server.api_key, b""));
            call_raw(
                server,
                Method::GET,
                path,
                &[
                    ("accept", "application/json"),
                    ("x-pelty-action", "test"),
                    ("x-pelty-signature", &sig),
                ],
                None,
            )
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
            let r = fire("create", server, svc).await?;
            let ptr = r
                .pointer("/endpoint/remote_id_pointer")
                .and_then(Value::as_str)
                .unwrap_or("/id");
            let remote = match r["response"].pointer(ptr) {
                Some(Value::String(s)) if !s.is_empty() => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => svc.service_id.clone(),
            };
            Ok(Created {
                remote_id: remote,
                module_data: json!({}),
            })
        })
    }

    fn suspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { fire("suspend", server, svc).await.map(|_| ()) })
    }

    fn unsuspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { fire("unsuspend", server, svc).await.map(|_| ()) })
    }

    fn terminate<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            match fire("terminate", server, svc).await {
                Err(ProvisionError::Rejected { status: 404, .. }) => Ok(()),
                r => r.map(|_| ()),
            }
        })
    }

    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { fire("change_package", server, svc).await.map(|_| ()) })
    }

    /// Optional `usage` action: the JSON response is the usage object
    /// (overage metrics read numbers from it by dotted path).
    fn usage<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<Option<Value>>> {
        Box::pin(async move {
            if endpoint("usage", server, svc).is_err() {
                return Ok(None);
            }
            Ok(Some(fire("usage", server, svc).await?["response"].clone()))
        })
    }

    /// Optional `list_accounts` action: the response (or the array at
    /// `accounts_pointer`, default `/accounts`) lists
    /// `{remote_id|id, username, domain, email, package, status}` objects.
    fn list_accounts<'a>(
        &'a self,
        server: &'a ServerConn,
    ) -> BoxFuture<'a, ProvisionResult<Option<Vec<RemoteAccount>>>> {
        Box::pin(async move {
            let svc = ServiceCtx::default();
            let Ok(ep) = endpoint("list_accounts", server, &svc) else {
                return Ok(None);
            };
            let r = fire("list_accounts", server, &svc).await?;
            let ptr = ep
                .get("accounts_pointer")
                .and_then(Value::as_str)
                .unwrap_or("/accounts");
            let list = r["response"]
                .pointer(ptr)
                .or(Some(&r["response"]))
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ProvisionError::Transient("list_accounts: response is not a list".into())
                })?;
            let s = |v: &Value, k: &str| match v.get(k) {
                Some(Value::String(x)) => x.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            };
            Ok(Some(
                list.iter()
                    .map(|a| RemoteAccount {
                        remote_id: Some(s(a, "remote_id"))
                            .filter(|x| !x.is_empty())
                            .unwrap_or_else(|| s(a, "id")),
                        username: s(a, "username"),
                        domain: s(a, "domain"),
                        email: s(a, "email"),
                        package: s(a, "package"),
                        status: s(a, "status"),
                    })
                    .filter(|a| !a.remote_id.is_empty())
                    .collect(),
            ))
        })
    }
}
