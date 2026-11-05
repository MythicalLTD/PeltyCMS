//! JSON wire format between the host and WASM addons of kind `provisioning`
//! and `registrar` (`pelty:addon@0.2`). Shared by the guest SDK and the host
//! adapters so both sides agree byte for byte.
use crate::registrar::{Contact, RegistrarError, RegistrarModule, RemoteDomain};
use crate::*;
use serde_json::{json, Value};

pub type Fut<'a> = BoxFuture<'a, Result<Value, String>>;
/// Drives a module future to completion (guest: synchronous host I/O).
pub type BlockOn<'b> = &'b dyn Fn(Fut<'_>) -> Result<Value, String>;

// ----- errors -----

pub fn perr_to(e: &ProvisionError) -> String {
    match e {
        ProvisionError::Config(m) => format!("config:{m}"),
        ProvisionError::Rejected { status, message } => format!("rejected:{status}:{message}"),
        ProvisionError::Transient(m) => format!("transient:{m}"),
    }
}

pub fn perr_from(s: &str) -> ProvisionError {
    match s.split_once(':') {
        Some(("config", m)) => ProvisionError::Config(m.into()),
        Some(("rejected", rest)) => {
            let (st, msg) = rest.split_once(':').unwrap_or(("0", rest));
            ProvisionError::Rejected {
                status: st.parse().unwrap_or(0),
                message: msg.into(),
            }
        }
        Some(("transient", m)) => ProvisionError::Transient(m.into()),
        _ => ProvisionError::Transient(s.into()),
    }
}

pub fn rerr_to(e: &RegistrarError) -> String {
    match e {
        RegistrarError::Unavailable => "unavailable:".into(),
        RegistrarError::BadAuthCode => "bad_auth_code:".into(),
        RegistrarError::UnknownDomain => "unknown_domain:".into(),
        RegistrarError::Unsupported => "unsupported:".into(),
        RegistrarError::Remote(m) => format!("remote:{m}"),
    }
}

pub fn rerr_from(s: &str) -> RegistrarError {
    match s.split_once(':') {
        Some(("unavailable", _)) => RegistrarError::Unavailable,
        Some(("bad_auth_code", _)) => RegistrarError::BadAuthCode,
        Some(("unknown_domain", _)) => RegistrarError::UnknownDomain,
        Some(("unsupported", _)) => RegistrarError::Unsupported,
        Some(("remote", m)) => RegistrarError::Remote(m.into()),
        _ => RegistrarError::Remote(s.into()),
    }
}

// ----- provisioning -----

pub fn server_to(s: &ServerConn) -> Value {
    json!({ "base_url": s.base_url, "config": s.config })
}

pub fn svc_to(s: &ServiceCtx) -> Value {
    json!({
        "service_id": s.service_id, "customer_email": s.customer_email,
        "customer_first_name": s.customer_first_name, "customer_last_name": s.customer_last_name,
        "product_config": s.product_config, "custom_fields": s.custom_fields,
        "remote_id": s.remote_id, "module_data": s.module_data,
    })
}

pub fn svc_from(v: &Value) -> ServiceCtx {
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    ServiceCtx {
        service_id: s("service_id"),
        customer_email: s("customer_email"),
        customer_first_name: s("customer_first_name"),
        customer_last_name: s("customer_last_name"),
        product_config: v["product_config"].clone(),
        custom_fields: v["custom_fields"].clone(),
        remote_id: v["remote_id"].as_str().map(str::to_string),
        module_data: v["module_data"].clone(),
    }
}

/// `describe()` for a provisioning module.
pub fn describe(m: &dyn ProvisioningModule) -> String {
    let mut info = m.info();
    info.actions = m.custom_actions().to_vec();
    json!({ "info": info, "checkout_fields": m.checkout_fields() }).to_string()
}

/// Guest-side dispatcher for provisioning ops. The API key comes from the
/// host secret `api_key` (never part of the JSON input).
pub fn call(
    m: &dyn ProvisioningModule,
    api_key: Option<String>,
    block_on: BlockOn<'_>,
    op: &str,
    input: &str,
) -> Result<String, String> {
    let input: Value = serde_json::from_str(input).map_err(|e| format!("config:bad input {e}"))?;
    let server = ServerConn {
        base_url: input["server"]["base_url"]
            .as_str()
            .unwrap_or_default()
            .into(),
        api_key: api_key.unwrap_or_default(),
        config: input["server"]["config"].clone(),
    };
    let svc = svc_from(&input["svc"]);
    let (server, svc) = (&server, &svc);
    let e = |x: ProvisionError| perr_to(&x);
    let out = match op {
        "test_connection" => block_on(Box::pin(async move {
            m.test_connection(server)
                .await
                .map(|_| Value::Null)
                .map_err(e)
        }))?,
        "create" => block_on(Box::pin(async move {
            m.create(server, svc).await.map(|c| json!(c)).map_err(e)
        }))?,
        "suspend" => block_on(Box::pin(async move {
            m.suspend(server, svc).await.map(|_| Value::Null).map_err(e)
        }))?,
        "unsuspend" => block_on(Box::pin(async move {
            m.unsuspend(server, svc)
                .await
                .map(|_| Value::Null)
                .map_err(e)
        }))?,
        "terminate" => block_on(Box::pin(async move {
            m.terminate(server, svc)
                .await
                .map(|_| Value::Null)
                .map_err(e)
        }))?,
        "change_package" => block_on(Box::pin(async move {
            m.change_package(server, svc)
                .await
                .map(|_| Value::Null)
                .map_err(e)
        }))?,
        "sso_url" => {
            let token = input["token"].as_str().unwrap_or_default().to_string();
            block_on(Box::pin(async move {
                m.sso_url(server, svc, &token)
                    .await
                    .map(|u| json!(u))
                    .map_err(e)
            }))?
        }
        "custom" => {
            let action = input["action"].as_str().unwrap_or_default().to_string();
            let params = input["params"].clone();
            block_on(Box::pin(async move {
                m.custom(server, svc, &action, &params).await.map_err(e)
            }))?
        }
        "usage" => block_on(Box::pin(async move {
            m.usage(server, svc).await.map(|u| json!(u)).map_err(e)
        }))?,
        "list_accounts" => block_on(Box::pin(async move {
            m.list_accounts(server).await.map(|u| json!(u)).map_err(e)
        }))?,
        "catalog" => block_on(Box::pin(async move {
            m.catalog(server).await.map(|u| json!(u)).map_err(e)
        }))?,
        other => return Err(format!("config:unknown op {other}")),
    };
    Ok(out.to_string())
}

// ----- registrar -----

pub fn remote_to(d: &RemoteDomain) -> Value {
    json!({ "expires_on": d.expires_on, "status": d.status, "nameservers": d.nameservers })
}

pub fn remote_from(v: &Value) -> RemoteDomain {
    RemoteDomain {
        expires_on: v["expires_on"].as_str().unwrap_or_default().into(),
        status: v["status"].as_str().unwrap_or_default().into(),
        nameservers: v["nameservers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// `describe()` for a registrar: id, name and admin settings schema.
pub fn describe_registrar(m: &dyn RegistrarModule, settings: Value) -> String {
    json!({ "id": m.id(), "name": m.name(), "settings": settings }).to_string()
}

/// Guest-side dispatcher for registrar ops (`settings` configure the module).
pub fn call_registrar(
    base: &dyn RegistrarModule,
    block_on: BlockOn<'_>,
    op: &str,
    input: &str,
) -> Result<String, String> {
    let input: Value = serde_json::from_str(input).map_err(|e| format!("remote:bad input {e}"))?;
    let configured = base.configure(&input["settings"]);
    let m: &dyn RegistrarModule = configured.as_deref().unwrap_or(base);
    let s = |k: &str| input[k].as_str().unwrap_or_default().to_string();
    let (domain, expires) = (s("domain"), s("expires_on"));
    let years = input["years"].as_u64().unwrap_or(1) as u32;
    let ns: Vec<String> = input["nameservers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let contact: Contact = serde_json::from_value(input["registrant"].clone()).unwrap_or_default();
    let fields = input.get("fields").cloned().unwrap_or(Value::Null);
    let (d, ex, ns, c, f) = (&domain, &expires, &ns, &contact, &fields);
    let e = |x: RegistrarError| rerr_to(&x);
    let out = match op {
        "available" => block_on(Box::pin(async move {
            m.available(d).await.map(|b| json!(b)).map_err(e)
        }))?,
        "register" => block_on(Box::pin(async move {
            m.register_with_fields(d, years, ex, c, ns, f)
                .await
                .map(|_| Value::Null)
                .map_err(e)
        }))?,
        "renew" => block_on(Box::pin(async move {
            m.renew(d, years, ex).await.map(|_| Value::Null).map_err(e)
        }))?,
        "transfer" => {
            let code = s("auth_code");
            block_on(Box::pin(async move {
                m.transfer_with_fields(d, &code, ex, f)
                    .await
                    .map(|_| Value::Null)
                    .map_err(e)
            }))?
        }
        "set_nameservers" => block_on(Box::pin(async move {
            m.set_nameservers(d, ns)
                .await
                .map(|_| Value::Null)
                .map_err(e)
        }))?,
        "set_contact" => block_on(Box::pin(async move {
            m.set_contact(d, c).await.map(|_| Value::Null).map_err(e)
        }))?,
        "info" => block_on(Box::pin(async move {
            m.info(d).await.map(|r| remote_to(&r)).map_err(e)
        }))?,
        "epp_code" => block_on(Box::pin(async move {
            m.epp_code(d).await.map(|c| json!(c)).map_err(e)
        }))?,
        "tld_prices" => block_on(Box::pin(async move {
            m.tld_prices().await.map(|c| json!(c)).map_err(e)
        }))?,
        "premium_price" => block_on(Box::pin(async move {
            m.premium_price(d).await.map(|c| json!(c)).map_err(e)
        }))?,
        "set_lock" => {
            let locked = input["locked"].as_bool().unwrap_or(true);
            block_on(Box::pin(async move {
                m.set_lock(d, locked).await.map(|_| Value::Null).map_err(e)
            }))?
        }
        "set_id_protection" => {
            let enabled = input["enabled"].as_bool().unwrap_or(true);
            block_on(Box::pin(async move {
                m.set_id_protection(d, enabled)
                    .await
                    .map(|_| Value::Null)
                    .map_err(e)
            }))?
        }
        "replace_dns" => {
            let records: Vec<crate::registrar::DnsZoneRecord> =
                serde_json::from_value(input["records"].clone()).unwrap_or_default();
            block_on(Box::pin(async move {
                m.replace_dns(d, &records)
                    .await
                    .map(|_| Value::Null)
                    .map_err(e)
            }))?
        }
        "get_dns" => block_on(Box::pin(async move {
            m.get_dns(d).await.map(|r| json!(r)).map_err(e)
        }))?,
        "set_dnssec" => {
            let enabled = input["enabled"].as_bool().unwrap_or(true);
            block_on(Box::pin(async move {
                m.set_dnssec(d, enabled)
                    .await
                    .map(|_| Value::Null)
                    .map_err(e)
            }))?
        }
        other => return Err(format!("remote:unknown op {other}")),
    };
    Ok(out.to_string())
}
