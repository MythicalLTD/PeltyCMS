#[test]
fn registry_has_builtins() {
    let r = pelty_addon_tests_provisioning::builtins();
    let ids: Vec<_> = r.list().into_iter().map(|m| m.id).collect();
    for id in [
        "featherpanel",
        "https_agent",
        "pelican",
        "pterodactyl",
        "webhook",
        "proxmox",
        "cpanel",
        "plesk",
        "directadmin",
        "licensing",
        "convoy",
        "virtfusion",
        "enhance",
        "vendor_reseller",
        "solusio",
    ] {
        assert!(ids.contains(&id), "{id}");
    }
    assert_eq!(ids.len(), 28);
}
