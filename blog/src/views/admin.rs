use std::collections::BTreeMap;

use axum::http::StatusCode;
use loco_rs::prelude::*;
use serde::Serialize;

use crate::{
    controllers::admin::{PostForm, FLASH_COOKIE},
    models::{posts, users},
};

#[derive(Debug, Serialize)]
pub struct AdminPostRow {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub status: &'static str,
    pub is_live: bool,
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
            is_live: p.published_at.is_some(),
        }
    }
}

/// Every form field -> first error message ("" when valid).
///
/// Tera 2 fails the render on an *undefined* key, so the map always holds
/// all fields and templates can say `{% if errors.title %}` safely.
fn error_messages(errors: &validation::ModelValidationErrors) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = ["title", "slug", "body", "tags", "published_at"]
        .into_iter()
        .map(|f| (f.to_string(), String::new()))
        .collect();
    for (field, list) in &errors.errors {
        if let Some(e) = list.first() {
            out.insert(
                field.clone(),
                e.message.clone().unwrap_or_else(|| e.code.clone()),
            );
        }
    }
    out
}

/// `GET /admin`. A flash message, if any, is shown once and its cookie
/// deleted in the same response.
///
/// # Errors
/// Template rendering errors.
pub fn dashboard(
    v: &impl ViewRenderer,
    user: &users::Model,
    posts: &[posts::Model],
    flash: Option<&str>,
) -> Result<Response> {
    let rows: Vec<AdminPostRow> = posts.iter().map(AdminPostRow::from).collect();
    let mut builder = format::render();
    if flash.is_some() {
        let clear = cookie::Cookie::build((FLASH_COOKIE, ""))
            .path("/admin")
            .removal()
            .build();
        builder = builder.cookies(&[clear])?;
    }
    builder.view(
        v,
        "admin/dashboard.html",
        data!({
            "user": { "name": user.name, "email": user.email },
            "posts": rows,
            "flash": flash,
        }),
    )
}

/// New/edit form. For htmx requests only the `<form>` fragment is returned
/// (it replaces itself); otherwise the full page.
///
/// # Errors
/// Template rendering errors.
pub fn post_form(
    v: &impl ViewRenderer,
    form: &PostForm,
    post_id: Option<i64>,
    errors: &validation::ModelValidationErrors,
    fragment: bool,
    status: StatusCode,
) -> Result<Response> {
    let template = if fragment {
        "admin/_form.html"
    } else {
        "admin/form.html"
    };
    let action = post_id.map_or_else(
        || "/admin/posts".to_string(),
        |id| format!("/admin/posts/{id}"),
    );
    format::render().status(status).view(
        v,
        template,
        data!({
            "form": form,
            "errors": error_messages(errors),
            "has_errors": !errors.errors.is_empty(),
            "post_id": post_id,
            "action": action,
            "is_edit": post_id.is_some(),
        }),
    )
}

/// htmx: the updated table row plus an out-of-band flash.
///
/// # Errors
/// Template rendering errors.
pub fn row_with_flash(
    v: &impl ViewRenderer,
    post: &posts::Model,
    message: &str,
) -> Result<Response> {
    format::render().view(
        v,
        "admin/_row.html",
        data!({ "post": AdminPostRow::from(post), "oob_flash": message }),
    )
}

/// htmx: nothing for the target (row removed) + an out-of-band flash.
///
/// # Errors
/// Never in practice; `Result` for symmetry with the other renderers.
pub fn flash_oob(message: &str) -> Result<Response> {
    let escaped = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format::html(&format!(
        r#"<div id="flash" class="flash ok" hx-swap-oob="true">{escaped}</div>"#
    ))
}
