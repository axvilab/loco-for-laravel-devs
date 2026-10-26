//! Public blog pages. Controllers hand models to these functions; these
//! functions turn them into template data. Templates never see an entity.
use axum::http::StatusCode;
use loco_rs::prelude::*;
use serde::Serialize;

use crate::models::{posts, tags};

#[derive(Debug, Serialize)]
pub struct TagLink {
    pub name: String,
    pub url: String,
}

impl From<&tags::Model> for TagLink {
    fn from(tag: &tags::Model) -> Self {
        Self {
            name: tag.name.clone(),
            url: format!("/tags/{}", tag.slug),
        }
    }
}

fn tag_links(tags: &[tags::Model]) -> Vec<TagLink> {
    tags.iter().map(TagLink::from).collect()
}

#[derive(Debug, Serialize)]
pub struct PostSummary {
    pub title: String,
    pub url: String,
    pub excerpt: String,
    pub published_on: String,
    pub published_iso: String,
    pub tags: Vec<TagLink>,
}

#[derive(Debug, Serialize)]
pub struct PostDetail {
    pub title: String,
    pub url: String,
    pub body_html: String,
    pub published_on: String,
    pub published_iso: String,
    pub tags: Vec<TagLink>,
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

impl PostSummary {
    #[must_use]
    pub fn new(post: &posts::Model, tags: &[tags::Model]) -> Self {
        let (published_on, published_iso) = dates(post);
        Self {
            title: post.title.clone(),
            url: post_url(&post.slug),
            excerpt: post.excerpt(),
            published_on,
            published_iso,
            tags: tag_links(tags),
        }
    }

    /// Zip a page of posts with their eager-loaded tags.
    #[must_use]
    pub fn list(posts: &[posts::Model], tags: &[Vec<tags::Model>]) -> Vec<Self> {
        posts
            .iter()
            .zip(tags)
            .map(|(post, tags)| Self::new(post, tags))
            .collect()
    }
}

impl PostDetail {
    #[must_use]
    pub fn new(post: &posts::Model, tags: &[tags::Model]) -> Self {
        let (published_on, published_iso) = dates(post);
        Self {
            title: post.title.clone(),
            url: post_url(&post.slug),
            body_html: post.body_html(),
            published_on,
            published_iso,
            tags: tag_links(tags),
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
pub fn index(
    v: &impl ViewRenderer,
    page: &query::PageResponse<posts::Model>,
    tags: &[Vec<tags::Model>],
) -> Result<Response> {
    let posts = PostSummary::list(&page.page, tags);
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
pub fn show(v: &impl ViewRenderer, post: &posts::Model, tags: &[tags::Model]) -> Result<Response> {
    format::render().view(
        v,
        "posts/show.html",
        data!({ "post": PostDetail::new(post, tags) }),
    )
}

/// `GET /tags/{slug}`
///
/// # Errors
/// Template rendering errors.
pub fn tag(
    v: &impl ViewRenderer,
    tag: &tags::Model,
    page: &query::PageResponse<posts::Model>,
    tags: &[Vec<tags::Model>],
) -> Result<Response> {
    let base = format!("/tags/{}", tag.slug);
    format::render().view(
        v,
        "tags/show.html",
        data!({
            "tag": TagLink::from(tag),
            "posts": PostSummary::list(&page.page, tags),
            "pagination": PaginationView::new(&page.meta, &base),
        }),
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
