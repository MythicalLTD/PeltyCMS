//! Service lifecycle state machine. Mirrored by the `services_lifecycle_guard`
//! trigger so illegal transitions fail even if app code is bypassed.
pub const STATUSES: &[&str] = &[
    "pending",
    "active",
    "suspended",
    "terminated",
    "cancelled",
    "fraud",
];

/// Allowed `(from, to)` transitions.
pub const TRANSITIONS: &[(&str, &str)] = &[
    ("pending", "active"),
    ("pending", "cancelled"),
    ("pending", "fraud"),
    ("active", "suspended"),
    ("active", "terminated"),
    ("active", "cancelled"),
    ("suspended", "active"),
    ("suspended", "terminated"),
    ("suspended", "cancelled"),
];

pub fn can_transition(from: &str, to: &str) -> bool {
    TRANSITIONS.contains(&(from, to))
}

/// Admin actions → target status.
pub fn action_target(action: &str) -> Option<&'static str> {
    Some(match action {
        "activate" | "unsuspend" => "active",
        "suspend" => "suspended",
        "terminate" => "terminated",
        "cancel" => "cancelled",
        "fraud" => "fraud",
        _ => return None,
    })
}

pub fn is_terminal(s: &str) -> bool {
    matches!(s, "terminated" | "cancelled" | "fraud")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions() {
        assert!(can_transition("pending", "active"));
        assert!(can_transition("suspended", "active"));
        assert!(!can_transition("terminated", "active"));
        assert!(!can_transition("pending", "suspended"));
        for s in STATUSES {
            if is_terminal(s) {
                assert!(
                    TRANSITIONS.iter().all(|(f, _)| f != s),
                    "{s} must be terminal"
                );
            }
        }
        assert_eq!(action_target("unsuspend"), Some("active"));
        assert_eq!(action_target("rm"), None);
    }
}
