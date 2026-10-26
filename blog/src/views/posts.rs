//! Public blog pages. Controllers hand models to these functions; these
//! functions turn them into template data. Templates never see an entity.
use axum::http::StatusCode;
use loco_rs::prelude::*;
use serde::Serialize;

use crate::models::posts;

#[derive(Debug, Serialize)]
pub struct PostSummary {
    pub title: String,
    pub url: String,
    pub excerpt: String,
    pub published_on: String,
    pub published_iso: String,
}

#[derive(Debug, Serialize)]
pub struct PostDetail {
    pub title: String,
    pub url: String,
    pub body_html: String,
    pub published_on: String,
    pub published_iso: String,
}

#[derive(Debug, Serialize)]
pub struct PaginationView {
    pub page: u64,
    pub total_pages: u64,
    pub total_items: u64,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
}

#[must_use]
pub fn post_url(slug: &str) -> String {
    format!("/posts/{slug}")
}

fn dates(post: &posts::Model) -> (String, String) {
    post.published_at.map_or_else(
        || ("Draft".to_string(), String::new()),
        |at| (at.format("%B %-d, %Y").to_string(), at.to_rfc3339()),
    )
}

impl From<&posts::Model> for PostSummary {
    fn from(post: &posts::Model) -> Self {
        let (published_on, published_iso) = dates(post);
        Self {
            title: post.title.clone(),
            url: post_url(&post.slug),
            excerpt: post.excerpt(),
            published_on,
            published_iso,
        }
    }
}

impl From<&posts::Model> for PostDetail {
    fn from(post: &posts::Model) -> Self {
        let (published_on, published_iso) = dates(post);
        Self {
            title: post.title.clone(),
            url: post_url(&post.slug),
            body_html: post.body_html(),
            published_on,
            published_iso,
        }
    }
}

impl PaginationView {
    /// `base` is the list URL without query string, e.g. `/`.
    #[must_use]
    pub fn new(meta: &PagerMeta, base: &str) -> Self {
        let link = |page: u64| {
            if page <= 1 {
                base.to_string()
            } else {
                format!("{base}?page={page}")
            }
        };
        Self {
            page: meta.page,
            total_pages: meta.total_pages,
            total_items: meta.total_items,
            prev_url: (meta.page > 1).then(|| link(meta.page - 1)),
            next_url: (meta.page < meta.total_pages).then(|| link(meta.page + 1)),
        }
    }
}

/// `GET /`
///
/// # Errors
/// Template rendering errors.
pub fn index(v: &impl ViewRenderer, page: &query::PageResponse<posts::Model>) -> Result<Response> {
    let posts: Vec<PostSummary> = page.page.iter().map(PostSummary::from).collect();
    format::render().view(
        v,
        "posts/index.html",
        data!({
            "posts": posts,
            "pagination": PaginationView::new(&page.meta, "/"),
        }),
    )
}

/// `GET /posts/{slug}`
///
/// # Errors
/// Template rendering errors.
pub fn show(v: &impl ViewRenderer, post: &posts::Model) -> Result<Response> {
    format::render().view(
        v,
        "posts/show.html",
        data!({ "post": PostDetail::from(post) }),
    )
}

/// An HTML 404 (the default `?` on `EntityNotFound` answers JSON).
///
/// # Errors
/// Template rendering errors.
pub fn not_found(v: &impl ViewRenderer) -> Result<Response> {
    format::render()
        .status(StatusCode::NOT_FOUND)
        .view(v, "errors/404.html", data!({}))
}
