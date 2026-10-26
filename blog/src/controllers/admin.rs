//! `/admin` — post CRUD for the logged-in author. Server-rendered, enhanced
//! with htmx (no JS build step). Every handler takes `CurrentUser`, so an
//! anonymous request never reaches the body.
//!
//! Every form also works without JavaScript: htmx attributes sit on top of
//! ordinary `method="post"` forms, and handlers branch on `HX-Request`.
use axum::http::{HeaderMap, StatusCode};
use chrono::{NaiveDateTime, Utc};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::session::CurrentUser,
    models::posts::{self, field_error, PostParams},
    views,
    workers::process_post,
};

/// Background: render the HTML cache, reading time, notify subscribers.
/// The HTTP response doesn't wait for any of it.
async fn enqueue_processing(ctx: &AppContext, post: &posts::Model) -> Result<()> {
    process_post::Worker::perform_later(ctx, process_post::WorkerArgs { post_id: post.id }).await?;
    Ok(())
}

pub const FLASH_COOKIE: &str = "flash";

fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

/// What the HTML form posts. Everything is a string: browsers send empty
/// strings, not missing fields, and `datetime-local` has its own format.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PostForm {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: String,
    /// `<input type="datetime-local">` sends `2026-10-26T14:30` — no zone.
    /// We treat it as UTC (a real app would store the author's timezone).
    #[serde(default)]
    pub published_at: String,
}

const DATETIME_LOCAL: &str = "%Y-%m-%dT%H:%M";

impl PostForm {
    #[must_use]
    pub fn from_model(post: &posts::Model, tags: String) -> Self {
        Self {
            title: post.title.clone(),
            slug: post.slug.clone(),
            body: post.body.clone(),
            tags,
            published_at: post
                .published_at
                .map(|at| at.with_timezone(&Utc).format(DATETIME_LOCAL).to_string())
                .unwrap_or_default(),
        }
    }

    /// Form strings -> model params. A malformed date becomes a normal
    /// field error **merged with** the model's own validation errors, so the
    /// user sees every problem at once.
    fn to_params(&self, user_id: i64) -> std::result::Result<PostParams, ModelError> {
        let mut date_error = None;
        let published_at = match self.published_at.trim() {
            "" => None,
            raw => NaiveDateTime::parse_from_str(raw, DATETIME_LOCAL)
                .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S"))
                .map(|naive| naive.and_utc().into())
                .map_err(|_| {
                    date_error = Some(field_error(
                        "published_at",
                        "date",
                        "Use the format YYYY-MM-DDTHH:MM.",
                    ));
                })
                .ok(),
        };
        let params = PostParams {
            title: self.title.clone(),
            slug: self.slug.clone(),
            body: self.body.clone(),
            tags: self.tags.clone(),
            published_at,
            user_id: Some(user_id),
        };
        match (date_error, params.validate_for_save()) {
            (None, Ok(())) => Ok(params),
            (None, Err(e)) | (Some(e), Ok(())) => Err(e),
            (Some(ModelError::Validation(mut date)), Err(ModelError::Validation(fields))) => {
                date.errors.extend(fields.errors);
                Err(ModelError::Validation(date))
            }
            (Some(_), Err(e)) => Err(e),
        }
    }
}

/// Flash message = a short-lived cookie scoped to `/admin`, shown once by
/// the next admin page and deleted in that same response.
fn flash_cookie(message: &str) -> cookie::Cookie<'static> {
    // `format::render().cookies()` percent-encodes values (`Cookie::encoded`)
    // and `CookieJar` decodes them, so spaces/quotes/UTF-8 are fine as-is.
    cookie::Cookie::build((FLASH_COOKIE, message.to_string()))
        .path("/admin")
        .http_only(true)
        .same_site(cookie::SameSite::Lax)
        .build()
}

/// After a successful write: 303 for plain forms, `HX-Redirect` for htmx
/// (htmx then does a full page load, which picks up the flash).
fn redirect_with_flash(headers: &HeaderMap, to: &str, message: &str) -> Result<Response> {
    let builder = format::render().cookies(&[flash_cookie(message)])?;
    if is_htmx(headers) {
        builder.header("hx-redirect", to).empty()
    } else {
        builder.redirect(to)
    }
}

/// Render the form again with errors and **422 Unprocessable Content**.
fn invalid(
    v: &TeraView,
    headers: &HeaderMap,
    form: &PostForm,
    post: Option<&posts::Model>,
    err: ModelError,
) -> Result<Response> {
    match err {
        ModelError::Validation(errors) => views::admin::post_form(
            v,
            form,
            post,
            &errors,
            is_htmx(headers),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        other => Err(other.into()),
    }
}

/// `GET /admin` — my posts.
#[debug_handler]
pub async fn index(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    jar: cookie::CookieJar,
) -> Result<Response> {
    let posts = posts::Model::by_author(&ctx.db, user.id).await?;
    let flash = jar.get(FLASH_COOKIE).map(|c| c.value().to_string());
    views::admin::dashboard(&v, &user, &posts, flash.as_deref())
}

/// `GET /admin/posts/new`
#[debug_handler]
pub async fn new(
    CurrentUser(_user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(_ctx): State<AppContext>,
) -> Result<Response> {
    views::admin::post_form(
        &v,
        &PostForm::default(),
        None,
        &validation::ModelValidationErrors {
            errors: std::collections::BTreeMap::new(),
        },
        false,
        StatusCode::OK,
    )
}

/// `POST /admin/posts`
#[debug_handler]
pub async fn create(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Form(form): Form<PostForm>,
) -> Result<Response> {
    let params = match form.to_params(user.id) {
        Ok(p) => p,
        Err(e) => return invalid(&v, &headers, &form, None, e),
    };
    match posts::Model::create(&ctx.db, &params).await {
        Ok(post) => {
            enqueue_processing(&ctx, &post).await?;
            redirect_with_flash(&headers, "/admin", &format!("Created “{}”.", post.title))
        }
        Err(e) => invalid(&v, &headers, &form, None, e),
    }
}

/// `GET /admin/posts/{id}/edit`
#[debug_handler]
pub async fn edit(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
) -> Result<Response> {
    let post = posts::Model::find_owned(&ctx.db, id, user.id).await?;
    let form = PostForm::from_model(&post, post.tag_list(&ctx.db).await?);
    views::admin::post_form(
        &v,
        &form,
        Some(&post),
        &validation::ModelValidationErrors {
            errors: std::collections::BTreeMap::new(),
        },
        false,
        StatusCode::OK,
    )
}

/// `PUT /admin/posts/{id}` (htmx) and `POST /admin/posts/{id}` (no-JS form).
#[debug_handler]
pub async fn update(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<PostForm>,
) -> Result<Response> {
    let post = posts::Model::find_owned(&ctx.db, id, user.id).await?;
    let params = match form.to_params(user.id) {
        Ok(p) => p,
        Err(e) => return invalid(&v, &headers, &form, Some(&post), e),
    };
    match post
        .clone()
        .into_active_model()
        .update_from(&ctx.db, &params)
        .await
    {
        Ok(post) => {
            enqueue_processing(&ctx, &post).await?;
            redirect_with_flash(&headers, "/admin", &format!("Saved “{}”.", post.title))
        }
        Err(e) => invalid(&v, &headers, &form, Some(&post), e),
    }
}

/// `DELETE /admin/posts/{id}` (htmx: the row disappears) or
/// `POST /admin/posts/{id}/delete` (no-JS fallback).
#[debug_handler]
pub async fn destroy(
    CurrentUser(user): CurrentUser,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response> {
    let post = posts::Model::find_owned(&ctx.db, id, user.id).await?;
    let title = post.title.clone();
    let post = post.remove_cover(&ctx.db, &ctx.storage).await?;
    post.delete(&ctx.db).await?;
    let message = format!("Deleted “{title}”.");
    if is_htmx(&headers) {
        // Empty main content replaces the row (`hx-swap="outerHTML"`);
        // the out-of-band flash updates the message box.
        views::admin::flash_oob(&message)
    } else {
        redirect_with_flash(&headers, "/admin", &message)
    }
}

/// `POST /admin/posts/{id}/publish` — toggle published/draft.
#[debug_handler]
pub async fn toggle_publish(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response> {
    let post = posts::Model::find_owned(&ctx.db, id, user.id).await?;
    let post = if post.published_at.is_some() {
        post.into_active_model().unpublish(&ctx.db).await?
    } else {
        post.into_active_model().publish(&ctx.db).await?
    };
    enqueue_processing(&ctx, &post).await?;
    let message = if post.published_at.is_some() {
        format!("Published “{}”.", post.title)
    } else {
        format!("Unpublished “{}”.", post.title)
    };
    if is_htmx(&headers) {
        views::admin::row_with_flash(&v, &post, &message)
    } else {
        redirect_with_flash(&headers, "/admin", &message)
    }
}

/// Read the `cover` file part of a multipart body.
///
/// `limit_payload` makes the body stream fail mid-read when it's too big;
/// axum reports that as a `MultipartError` whose `status()` is 413.
async fn read_cover(
    mut multipart: Multipart,
) -> std::result::Result<axum::body::Bytes, axum::extract::multipart::MultipartError> {
    while let Some(field) = multipart.next_field().await? {
        if field.name() == Some("cover") {
            return field.bytes().await;
        }
    }
    Ok(axum::body::Bytes::new())
}

/// `POST /admin/posts/{id}/cover` (multipart, field `cover`).
#[debug_handler]
pub async fn upload_cover(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response> {
    let post = posts::Model::find_owned(&ctx.db, id, user.id).await?;
    let bytes = match read_cover(multipart).await {
        Ok(bytes) => bytes,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            let msg = "Images must be 4 MB or smaller.";
            return views::admin::cover(&v, &post, Some(msg), StatusCode::UNPROCESSABLE_ENTITY);
        }
        Err(e) => return Err(Error::BadRequest(format!("invalid upload: {e}"))),
    };
    match post
        .clone()
        .attach_cover(&ctx.db, &ctx.storage, &bytes)
        .await
    {
        Ok(post) if is_htmx(&headers) => views::admin::cover(&v, &post, None, StatusCode::OK),
        Ok(_) => redirect_with_flash(
            &headers,
            &format!("/admin/posts/{id}/edit"),
            "Cover updated.",
        ),
        Err(Error::Model(ModelError::Validation(errors))) => {
            let message = errors
                .errors
                .get("cover")
                .and_then(|list| list.first())
                .and_then(|e| e.message.clone())
                .unwrap_or_else(|| "Invalid image.".to_string());
            views::admin::cover(&v, &post, Some(&message), StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => Err(e),
    }
}

/// `POST /admin/posts/{id}/cover/delete`
#[debug_handler]
pub async fn delete_cover(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response> {
    let post = posts::Model::find_owned(&ctx.db, id, user.id).await?;
    let post = post.remove_cover(&ctx.db, &ctx.storage).await?;
    if is_htmx(&headers) {
        views::admin::cover(&v, &post, None, StatusCode::OK)
    } else {
        redirect_with_flash(
            &headers,
            &format!("/admin/posts/{id}/edit"),
            "Cover removed.",
        )
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/admin")
        .add("/", get(index))
        .add("/posts/new", get(new))
        .add("/posts", post(create))
        .add("/posts/{id}/edit", get(edit))
        .add("/posts/{id}", put(update))
        .add("/posts/{id}", post(update))
        .add("/posts/{id}", delete(destroy))
        .add("/posts/{id}/delete", post(destroy))
        .add("/posts/{id}/publish", post(toggle_publish))
        .add("/posts/{id}/cover", post(upload_cover))
        .add("/posts/{id}/cover/delete", post(delete_cover))
}
