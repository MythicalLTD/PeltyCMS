//! SSH-less command module: a small agent on the target host exposes HTTPS
//! endpoints and runs the configured command for each action. Pelty never
//! opens a shell; it sends a signed JSON request and the agent decides how
//! to execute it.
//!
//! Request: `POST {base_url}/v1/run` with
//! `{"action","service_id","remote_id","command","request_id"}`.
//! Headers: `X-Pelty-Timestamp: <unix secs>` and
//! `X-Pelty-Signature: sha256=<hmac(api_key, "<timestamp>.<body>")>`;
//! the agent must reject stale timestamps and bad signatures.
//! Response: `{"ok": bool, "output": "...", "remote_id": "..."}`.
use pelty_provisioning::http::call_raw;
use pelty_provisioning::http::Method;
use pelty_provisioning::template::{render, sign, vars, Escape};
use pelty_provisioning::{
    BoxFuture, Created, FieldSpec, ModuleInfo, ProvisionError, ProvisionResult, ProvisioningModule,
    ServerConn, ServiceCtx,
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct HttpsAgent;

const FIELDS: &[(&str, &str, bool)] = &[
    ("create_command", "provisioning.field.create_command", true),
    (
        "suspend_command",
        "provisioning.field.suspend_command",
        false,
    ),
    (
        "unsuspend_command",
        "provisioning.field.unsuspend_command",
        false,
    ),
    (
        "terminate_command",
        "provisioning.field.terminate_command",
        false,
    ),
    (
        "change_package_command",
        "provisioning.field.change_package_command",
        false,
    ),
];

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn headers(secret: &str, body: &str) -> (String, String) {
    let ts = now().to_string();
    let sig = format!("sha256={}", sign(secret, format!("{ts}.{body}").as_bytes()));
    (ts, sig)
}

async fn run_cmd(action: &str, server: &ServerConn, svc: &ServiceCtx) -> ProvisionResult<Value> {
    let key = format!("{action}_command");
    let tpl = svc
        .product_config
        .get(&key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ProvisionError::Config(format!("missing {key}")))?;
    let command = render(tpl, &vars(action, svc), Escape::Plain);
    let body = json!({
        "action": action,
        "service_id": svc.service_id,
        "remote_id": svc.remote_id,
        "command": command,
        "request_id": format!("{}:{action}", svc.service_id),
    })
    .to_string();
    let (ts, sig) = headers(&server.api_key, &body);
    let r = call_raw(
        server,
        Method::POST,
        "/v1/run",
        &[
            ("accept", "application/json"),
            ("x-pelty-timestamp", &ts),
            ("x-pelty-signature", &sig),
        ],
        Some(("application/json", body)),
    )
    .await?;
    if r.get("ok").and_then(Value::as_bool) != Some(true) {
        let msg = r
            .get("output")
            .and_then(Value::as_str)
            .unwrap_or("agent reported failure")
            .chars()
            .take(300)
            .collect();
        return Err(ProvisionError::Rejected {
            status: 422,
            message: msg,
        });
    }
    Ok(r)
}

impl ProvisioningModule for HttpsAgent {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            actions: vec![],
            id: "https_agent",
            name: "HTTPS agent",
            server_fields: vec![],
            product_fields: FIELDS
                .iter()
                .map(|(k, l, r)| FieldSpec {
                    key: k,
                    label: l,
                    kind: "text",
                    required: *r,
                    default: None,
                })
                .collect(),
        }
    }

    fn test_connection<'a>(&'a self, server: &'a ServerConn) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move {
            let (ts, sig) = headers(&server.api_key, "");
            call_raw(
                server,
                Method::GET,
                "/v1/health",
                &[
                    ("accept", "application/json"),
                    ("x-pelty-timestamp", &ts),
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
            let r = run_cmd("create", server, svc).await?;
            let remote = r
                .get("remote_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| svc.service_id.clone());
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
        Box::pin(async move { run_cmd("suspend", server, svc).await.map(|_| ()) })
    }

    fn unsuspend<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { run_cmd("unsuspend", server, svc).await.map(|_| ()) })
    }

    fn terminate<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { run_cmd("terminate", server, svc).await.map(|_| ()) })
    }

    fn change_package<'a>(
        &'a self,
        server: &'a ServerConn,
        svc: &'a ServiceCtx,
    ) -> BoxFuture<'a, ProvisionResult<()>> {
        Box::pin(async move { run_cmd("change_package", server, svc).await.map(|_| ()) })
    }
}
