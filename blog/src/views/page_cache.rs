//! Rendered-page cache for the public pages (Rails "fragment caching",
//! Laravel `Cache::remember('page:...', ...)` around a view render).
//!
//! Invalidation is by **generation**: every key embeds the current value of
//! `pages:generation`, and any write to posts bumps it — all old entries
//! become unreachable at once (and expire by TTL). No key bookkeeping.
//!
//! Limits, honestly:
//! * `ctx.cache` is `InMem` here, i.e. **per process**. A bump in the web
//!   process is instant; a change made by another process (the worker, the
//!   scheduler, a task) is only picked up when the TTL runs out. With more
//!   than one process, use `kind: Redis` (feature `cache_redis`).
//! * A cache failure never fails the page: we log and render uncached.
use std::{future::Future, time::Duration};

use loco_rs::{cache::Cache, prelude::*};

/// Upper bound for staleness when another process changed something.
pub const TTL: Duration = Duration::from_secs(60);
const GENERATION_KEY: &str = "pages:generation";

async fn generation(cache: &Cache) -> u64 {
    match cache.get::<u64>(GENERATION_KEY).await {
        Ok(Some(generation)) => generation,
        Ok(None) => 0,
        Err(err) => {
            tracing::warn!(error = %err, "page cache: cannot read generation");
            0
        }
    }
}

/// Drop every cached page (call after any post/tag write).
pub async fn invalidate(cache: &Cache) {
    let next = generation(cache).await.wrapping_add(1);
    if let Err(err) = cache.insert(GENERATION_KEY, &next).await {
        tracing::warn!(error = %err, "page cache: cannot bump generation");
    }
}

/// A strong `ETag` for a body. `DefaultHasher::new()` uses fixed keys, so
/// the value is stable across requests and processes of the same build.
#[must_use]
pub fn etag_for(body: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}

/// HTML response with an `ETag`. Loco's `etag` middleware (on by default)
/// then answers `If-None-Match` with `304 Not Modified` — but it never
/// *computes* an ETag itself; the handler has to set one.
///
/// # Errors
/// Response building errors.
pub fn respond_html(html: &str) -> Result<Response> {
    format::render()
        .etag(&etag_for(html))?
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        .html(html)
}

/// Return the cached HTML for `key`, or run `render`, store and return it.
///
/// # Errors
/// Only errors from `render` itself.
pub async fn html<F>(cache: &Cache, key: &str, render: F) -> Result<String>
where
    F: Future<Output = Result<String>> + Send,
{
    let full_key = format!("pages:{}:{key}", generation(cache).await);
    match cache.get::<String>(&full_key).await {
        Ok(Some(hit)) => return Ok(hit),
        Ok(None) => {}
        Err(err) => tracing::warn!(error = %err, key = full_key, "page cache: read failed"),
    }
    let rendered = render.await?;
    if let Err(err) = cache.insert_with_expiry(&full_key, &rendered, TTL).await {
        tracing::warn!(error = %err, key = full_key, "page cache: write failed");
    }
    Ok(rendered)
}
