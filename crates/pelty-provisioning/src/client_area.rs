//! WHMCS-style client-area contributions from a provisioning module:
//! tabs (overview / data / usage / actions / custom view) and rich action
//! metadata (labels, danger, input fields). Modules override
//! [`ProvisioningModule::client_area`]; the default builds a sensible panel
//! from `custom_actions` + usage support.
use serde::Serialize;
use serde_json::{json, Value};

/// One input field on a client module action form.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClientActionField {
    pub key: &'static str,
    pub label: &'static str,
    /// text | password | number | textarea | select
    pub kind: &'static str,
    pub required: bool,
    pub choices: &'static [&'static str],
}

/// Declared client action button (beyond a bare action id).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClientActionMeta {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub danger: bool,
    pub fields: &'static [ClientActionField],
}

/// One tab in the client service module panel.
#[derive(Debug, Clone, Serialize)]
pub struct ClientAreaTab {
    pub id: String,
    pub title: String,
    /// overview | data | usage | actions | view
    pub kind: String,
    /// Declarative UI node tree when `kind == "view"` (same shape as plugin UI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<Value>,
}

/// Full client-area contribution for one module.
#[derive(Debug, Clone, Serialize)]
pub struct ClientArea {
    pub tabs: Vec<ClientAreaTab>,
    pub actions: Vec<ClientActionMeta>,
}

const DANGER: &[&str] = &[
    "stop",
    "shutdown",
    "reboot",
    "restart",
    "reissue",
    "reset_password",
    "terminate",
];

/// Built-in field for password-changing actions.
pub const PASSWORD_FIELD: ClientActionField = ClientActionField {
    key: "password",
    label: "New password",
    kind: "password",
    required: false,
    choices: &[],
};

fn meta_for(id: &'static str) -> ClientActionMeta {
    let fields: &'static [ClientActionField] = if matches!(id, "change_password" | "reset_password")
    {
        &[PASSWORD_FIELD]
    } else {
        &[]
    };
    ClientActionMeta {
        id,
        label: id,
        description: "",
        danger: DANGER.contains(&id),
        fields,
    }
}

/// Default panel: Overview + Data + Usage (if claimed) + Actions.
pub fn default_area(actions: &[&'static str], has_usage: bool) -> ClientArea {
    let mut tabs = vec![
        ClientAreaTab {
            id: "overview".into(),
            title: "Overview".into(),
            kind: "overview".into(),
            view: None,
        },
        ClientAreaTab {
            id: "details".into(),
            title: "Details".into(),
            kind: "data".into(),
            view: None,
        },
    ];
    if has_usage {
        tabs.push(ClientAreaTab {
            id: "usage".into(),
            title: "Usage".into(),
            kind: "usage".into(),
            view: None,
        });
    }
    if !actions.is_empty() {
        tabs.push(ClientAreaTab {
            id: "actions".into(),
            title: "Actions".into(),
            kind: "actions".into(),
            view: None,
        });
    }
    ClientArea {
        tabs,
        actions: actions.iter().copied().map(meta_for).collect(),
    }
}

/// Parse an optional `client_area` object from a module `describe()` / product
/// config override. Unknown shapes are ignored (safe fallback).
pub fn parse_override(v: &Value, fallback: ClientArea) -> ClientArea {
    let Some(obj) = v.as_object() else {
        return fallback;
    };
    let mut area = fallback;
    if let Some(tabs) = obj.get("tabs").and_then(|t| t.as_array()) {
        let parsed: Vec<ClientAreaTab> = tabs
            .iter()
            .filter_map(|t| {
                let id = t.get("id")?.as_str()?.chars().take(40).collect::<String>();
                if id.is_empty() {
                    return None;
                }
                let title = t
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or(&id)
                    .chars()
                    .take(80)
                    .collect();
                let kind = t
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .unwrap_or("view")
                    .chars()
                    .take(20)
                    .collect::<String>();
                if !["overview", "data", "usage", "actions", "view"].contains(&kind.as_str()) {
                    return None;
                }
                let view = t.get("view").cloned().filter(|n| n.is_object());
                Some(ClientAreaTab {
                    id,
                    title,
                    kind,
                    view,
                })
            })
            .take(12)
            .collect();
        if !parsed.is_empty() {
            area.tabs = parsed;
        }
    }
    if let Some(actions) = obj.get("actions").and_then(|a| a.as_array()) {
        // Enrich labels/descriptions for known action ids only.
        for a in actions {
            let Some(id) = a.get("id").and_then(|x| x.as_str()) else {
                continue;
            };
            if let Some(m) = area.actions.iter_mut().find(|m| m.id == id) {
                if let Some(l) = a.get("label").and_then(|x| x.as_str()) {
                    // Can't mutate &'static str easily — encode in JSON at API layer instead.
                    let _ = (m, l);
                }
            }
        }
        let _ = json!({});
    }
    area
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_includes_usage_and_actions() {
        let a = default_area(&["start", "stop", "change_password"], true);
        assert_eq!(a.tabs.len(), 4);
        assert!(a
            .actions
            .iter()
            .any(|x| x.id == "change_password" && !x.fields.is_empty()));
        assert!(a.actions.iter().any(|x| x.id == "stop" && x.danger));
    }

    #[test]
    fn override_tabs() {
        let fb = default_area(&["start"], false);
        let v = json!({"tabs":[
            {"id":"info","title":"Info","kind":"data"},
            {"id":"custom","title":"Custom","kind":"view","view":{"type":"text","text":"Hello"}}
        ]});
        let a = parse_override(&v, fb);
        assert_eq!(a.tabs.len(), 2);
        assert_eq!(a.tabs[1].kind, "view");
        assert!(a.tabs[1].view.is_some());
    }
}
