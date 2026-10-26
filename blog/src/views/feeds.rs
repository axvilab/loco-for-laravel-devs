//! XML views. Loco's view engine only loads `assets/views/**/*.html`, so the
//! XML templates are named `*.xml.html` (they're autoescaped as HTML, which
//! is valid escaping for XML text too). `format::render().view(..)` always
//! answers `text/html`, so we render to a string and set the type ourselves.
use axum::http::header;
use loco_rs::prelude::*;
use serde::Serialize;

use crate::models::{posts, tags};

#[derive(Debug, Serialize)]
pub struct FeedItem {
    pub title: String,
    pub url: String,
    pub pub_date: String,
    pub description_html: String,
}

#[derive(Debug, Serialize)]
pub struct SitemapUrl {
    pub loc: String,
    pub lastmod: String,
}

/// Wrap an XML body with its content type.
///
/// # Errors
/// Response building errors.
pub fn xml(content_type: &'static str, body: String) -> Result<Response> {
    format::render()
        .etag(&crate::views::page_cache::etag_for(&body))?
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=300")
        .response()
        .body(axum::body::Body::from(body))
        .map_err(|e| Error::Message(e.to_string()))
}

/// # Errors
/// Template rendering errors.
pub fn rss(v: &impl ViewRenderer, base_url: &str, posts: &[posts::Model]) -> Result<String> {
    let items: Vec<FeedItem> = posts
        .iter()
        .map(|p| FeedItem {
            title: p.title.clone(),
            url: format!("{base_url}{}", crate::views::posts::post_url(&p.slug)),
            // RSS wants RFC 822/2822 dates
            pub_date: p.published_at.map(|d| d.to_rfc2822()).unwrap_or_default(),
            description_html: p.body_html(),
        })
        .collect();
    let last_build = posts
        .first()
        .and_then(|p| p.published_at)
        .map(|d| d.to_rfc2822())
        .unwrap_or_default();
    v.render(
        "feeds/rss.xml.html",
        data!({ "base_url": base_url, "items": items, "last_build": last_build }),
    )
}

/// # Errors
/// Template rendering errors.
pub fn sitemap(
    v: &impl ViewRenderer,
    base_url: &str,
    posts: &[posts::Model],
    tags: &[tags::Model],
) -> Result<String> {
    let newest = posts
        .first()
        .map(|p| p.updated_at.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let mut urls = vec![SitemapUrl {
        loc: format!("{base_url}/"),
        lastmod: newest.clone(),
    }];
    urls.extend(posts.iter().map(|p| SitemapUrl {
        loc: format!("{base_url}{}", crate::views::posts::post_url(&p.slug)),
        lastmod: p.updated_at.format("%Y-%m-%d").to_string(),
    }));
    urls.extend(tags.iter().map(|t| SitemapUrl {
        loc: format!("{base_url}/tags/{}", t.slug),
        lastmod: newest.clone(),
    }));
    v.render("feeds/sitemap.xml.html", data!({ "urls": urls }))
}
