//! Admin operations repositories: audit log, dashboard, reports, exports/imports, mass actions, jobs and housekeeping.
//!
//! Re-exported at the crate root. Covered by the admin/reports/jobs integration tests.

pub mod admin_ops;
pub mod admin_tools;
pub mod audit;
pub mod csv_import;
pub mod dashboard;
pub mod exports;
pub mod housekeeping;
pub mod job_stats;
pub mod jobs;
pub mod mass;
pub mod report_library;
pub mod reports;
