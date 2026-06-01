//! Rate-limit primitive tests. These exercise the real Redis-backed bucket
//! (`middleware::rate_limit::hit_conn`) against the `REDIS_URL` Redis — the
//! same one CI provides as a service.

use sprintly_api::{middleware::rate_limit, AppError};
use uuid::Uuid;

async fn conn() -> redis::aio::MultiplexedConnection {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379/0".into());
    redis::Client::open(url)
        .expect("redis url")
        .get_multiplexed_async_connection()
        .await
        .expect("redis connection — is REDIS_URL reachable?")
}

#[tokio::test]
async fn allows_up_to_limit_then_denies() {
    let mut c = conn().await;
    let key = format!("test:rl:{}", Uuid::now_v7());

    // First `limit` hits are allowed.
    for i in 0..3 {
        rate_limit::hit_conn(&mut c, &key, 3, 60)
            .await
            .unwrap_or_else(|_| panic!("hit {i} should be within limit"));
    }

    // The next one trips the limiter with a positive Retry-After.
    match rate_limit::hit_conn(&mut c, &key, 3, 60).await {
        Err(AppError::RateLimited { retry_after }) => assert!(retry_after >= 1),
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn keys_are_independent() {
    let mut c = conn().await;
    let a = format!("test:rl:{}", Uuid::now_v7());
    let b = format!("test:rl:{}", Uuid::now_v7());

    // Exhaust bucket A.
    rate_limit::hit_conn(&mut c, &a, 1, 60).await.unwrap();
    assert!(rate_limit::hit_conn(&mut c, &a, 1, 60).await.is_err());

    // Bucket B is untouched.
    rate_limit::hit_conn(&mut c, &b, 1, 60)
        .await
        .expect("a fresh key must not be limited by another key");
}
