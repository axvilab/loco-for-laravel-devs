use loco_rs::prelude::*;
use serde::Serialize;

use crate::models::{posts, users};

#[derive(Debug, Serialize)]
pub struct AdminPostRow {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub status: &'static str,
}

impl From<&posts::Model> for AdminPostRow {
    fn from(p: &posts::Model) -> Self {
        let status = match p.published_at {
            None => "draft",
            Some(_) if p.is_published() => "published",
            Some(_) => "scheduled",
        };
        Self {
            id: p.id,
            title: p.title.clone(),
            url: crate::views::posts::post_url(&p.slug),
            status,
        }
    }
}

/// # Errors
/// Template rendering errors.
pub fn dashboard(
    v: &impl ViewRenderer,
    user: &users::Model,
    posts: &[posts::Model],
) -> Result<Response> {
    let rows: Vec<AdminPostRow> = posts.iter().map(AdminPostRow::from).collect();
    format::render().view(
        v,
        "admin/dashboard.html",
        data!({ "user": { "name": user.name, "email": user.email }, "posts": rows }),
    )
}
