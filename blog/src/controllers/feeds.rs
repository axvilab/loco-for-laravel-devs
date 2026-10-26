//! Machine-readable endpoints: RSS, sitemap, robots.txt.
use axum::http::header;
use loco_rs::prelude::*;

use crate::{
    data::settings::Settings,
    models::{posts, tags},
    views,
};

/// `GET /feed.xml` — RSS 2.0, newest 20 posts.
#[debug_handler]
pub async fn rss(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let settings = Settings::from_context(&ctx)?;
    let posts = posts::Model::latest_published(&ctx.db, 20).await?;
    views::feeds::rss(&v, &settings.base_url, &posts)
}

/// `GET /sitemap.xml`
#[debug_handler]
pub async fn sitemap(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let settings = Settings::from_context(&ctx)?;
    let posts = posts::Model::latest_published(&ctx.db, 50_000).await?;
    let tags = tags::Model::with_published_posts(&ctx.db).await?;
    views::feeds::sitemap(&v, &settings.base_url, &posts, &tags)
}

/// `GET /robots.txt` — points crawlers at the sitemap, keeps them out of /admin.
#[debug_handler]
pub async fn robots(State(ctx): State<AppContext>) -> Result<Response> {
    let base = Settings::from_context(&ctx)?.base_url;
    format::render()
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .text(&format!(
            "User-agent: *\nDisallow: /admin\nDisallow: /login\n\nSitemap: {base}/sitemap.xml\n"
        ))
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/feed.xml", get(rss))
        .add("/sitemap.xml", get(sitemap))
        .add("/robots.txt", get(robots))
}
