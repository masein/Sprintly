//! Pure business logic, kept off the HTTP layer for testability.
//! Auth, permissions, vault crypto, estimation math, payroll math live here.

pub mod achievements;
pub mod integrations;
pub mod labels;
pub mod metrics;
pub mod notifications;
pub mod password;
pub mod payroll;
pub mod permissions;
pub mod projects;
pub mod sessions;
pub mod sprints;
pub mod tasks;
pub mod timesheets;
pub mod tokens;
pub mod vault;
pub mod webhooks;
