//! Custom middleware. Auth extractor + CSRF guard + client-IP resolver.
//! Redis-backed rate limiting lands when we have endpoints that actually
//! need throttling beyond what tower-http gives us for free.

pub mod auth;
pub mod client_ip;
pub mod csrf;

pub use auth::CurrentUser;
pub use client_ip::client_ip;
