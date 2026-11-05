//! WHMCS / Blesta / Paymenter -> PeltyCMS importer.
//!
//! * `blesta`, `paymenter`, JSON-export readers converted into the WHMCS
//!   model, then written by the same `import::run_source`.
//!
//! * `sqldump`, reads a WHMCS SQL export (no MySQL needed) into the same model.
//! * `whmcs_crypt`, WHMCS' cc_encryption_hash cipher and legacy password hashes.
//! * `whmcs`, plain row structs mirroring the WHMCS tables we read, plus an
//!   optional MySQL loader (cargo feature `mysql`).
//! * `map`, pure, unit-tested mapping rules (statuses, cycles, money, slugs).
//! * `import`, writes into PeltyCMS inside ONE transaction, idempotently via
//!   the `import_map` table. `dry_run` executes everything and rolls back, so
//!   the report is exactly what a real run would do.
pub mod blesta;
pub mod foreign;
pub mod import;
mod import_more;
pub mod map;
pub mod paymenter;
pub mod sqldump;
pub mod whmcs_crypt;
pub mod whmcs;

pub use import::{run, run_source, run_with, Report, Sealer};
pub use whmcs::WhmcsData;
