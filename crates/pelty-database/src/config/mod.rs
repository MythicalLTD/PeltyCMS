//! Settings repositories: key/value settings, typed general settings, automation, branding, site content, translations and the setup wizard.
//!
//! Re-exported at the crate root. pelty-docs reads general_settings.rs and setup_wizard.rs from here.

pub mod settings;
pub mod general_settings;
pub mod automation_settings;
pub mod branding;
pub mod site;
pub mod content;
pub mod translations;
pub mod languages;
pub mod setup_wizard;
pub mod license;
