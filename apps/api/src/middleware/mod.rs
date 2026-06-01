//! Custom middleware. Auth extractor + CSRF guard + client-IP resolver +
//! Redis-backed rate limiting (auth endpoints, vault reveal).

pub mod auth;
pub mod client_ip;
pub mod csrf;
pub mod rate_limit;

pub use auth::CurrentUser;
pub use client_ip::client_ip;
