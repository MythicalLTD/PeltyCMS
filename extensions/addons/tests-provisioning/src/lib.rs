//! Registry with every provisioning addon linked natively, the way the host
//! sees them once their `.wasm` packages are installed. Only used by tests.
use pelty_provisioning::Registry;
use std::sync::Arc;

pub fn builtins() -> Registry {
    let mut r = Registry::default();
    r.register(Arc::new(
        pelty_addon_provisioning_pterodactyl::pterodactyl::PTERODACTYL,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_pterodactyl::pterodactyl::PELICAN,
    ));
    r.register(Arc::new(pelty_addon_provisioning_webhook::webhook::Webhook));
    r.register(Arc::new(
        pelty_addon_provisioning_https_agent::agent::HttpsAgent,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_featherpanel::featherpanel::FeatherPanel,
    ));
    r.register(Arc::new(pelty_addon_provisioning_proxmox::proxmox::Proxmox));
    r.register(Arc::new(pelty_addon_provisioning_cpanel::cpanel::Cpanel));
    r.register(Arc::new(pelty_addon_provisioning_plesk::plesk::Plesk));
    r.register(Arc::new(
        pelty_addon_provisioning_directadmin::directadmin::DirectAdmin,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_cyberpanel::cyberpanel::CyberPanel,
    ));
    r.register(Arc::new(pelty_addon_provisioning_hestia::hestia::Hestia));
    r.register(Arc::new(
        pelty_addon_provisioning_ispconfig::ispconfig::IspConfig,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_hetzner_cloud::hetzner::HetznerCloud,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_digitalocean::digitalocean::DigitalOcean,
    ));
    r.register(Arc::new(pelty_addon_provisioning_vultr::vultr::Vultr));
    r.register(Arc::new(pelty_addon_provisioning_linode::linode::Linode));
    r.register(Arc::new(
        pelty_addon_provisioning_virtualizor::virtualizor::Virtualizor,
    ));
    r.register(Arc::new(pelty_addon_provisioning_solusvm::solusvm::SolusVm));
    r.register(Arc::new(pelty_addon_provisioning_solusio::solusio::SolusIo));
    r.register(Arc::new(pelty_addon_provisioning_amp::amp::Amp));
    r.register(Arc::new(
        pelty_addon_provisioning_docker::docker::DockerHost,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_openstack::openstack::OpenStack,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_ssl_reseller::ssl::SslReseller,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_vendor::vendor::VendorReseller,
    ));
    r.register(Arc::new(
        pelty_addon_provisioning_licensing::licensing::Licensing,
    ));
    r.register(Arc::new(pelty_addon_provisioning_convoy::convoy::Convoy));
    r.register(Arc::new(
        pelty_addon_provisioning_virtfusion::virtfusion::VirtFusion,
    ));
    r.register(Arc::new(pelty_addon_provisioning_enhance::enhance::Enhance));
    r
}
