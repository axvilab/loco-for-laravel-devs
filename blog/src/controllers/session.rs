//! Browser login/logout. The JSON API in `auth.rs` returns a token in the
//! body; browsers instead get it in an httpOnly cookie that the JWT extractor
//! reads (see `auth.jwt.location` in config).
use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::{request::Parts, StatusCode},
    response::Redirect,
};
use loco_rs::prelude::*;
use serde::Deserialize;

use crate::{data::settings::Settings, models::users};

/// Must match `auth.jwt.location[].name` in `config/*.yaml`.
pub const AUTH_COOKIE: &str = "auth_token";

/// The logged-in user for **HTML** routes.
///
/// `auth::JWTWithUser<users::Model>` answers a missing/invalid token with a
/// JSON 401 — right for an API, wrong for a browser. This wraps it and
/// redirects to `/login?next=<path>` instead (Laravel's `auth` middleware).
pub struct CurrentUser(pub users::Model);

impl FromRequestParts<AppContext> for CurrentUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppContext,
    ) -> std::result::Result<Self, Self::Rejection> {
        match auth::JWTWithUser::<users::Model>::from_request_parts(parts, state).await {
            Ok(jwt) => Ok(Self(jwt.user)),
            Err(_) => {
                let next = parts
                    .uri
                    .path_and_query()
                    .map_or("/admin", |pq| pq.as_str());
                let to = format!("/login?next={}", urlencode(next));
                // htmx requests can't follow a 303 to a full page usefully:
                // tell htmx to do a full-page navigation instead.
                if parts.headers.contains_key("hx-request") {
                    return Err((StatusCode::UNAUTHORIZED, [("hx-redirect", to)]).into_response());
                }
                Err(Redirect::to(&to).into_response())
            }
        }
    }
}

/// `Option<CurrentUser>`: logged-in user or `None`, never rejects.
impl OptionalFromRequestParts<AppContext> for CurrentUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppContext,
    ) -> std::result::Result<Option<Self>, Self::Rejection> {
        Ok(
            auth::JWTWithUser::<users::Model>::from_request_parts(parts, state)
                .await
                .ok()
                .map(|jwt| Self(jwt.user)),
        )
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Only same-site relative paths: blocks `?next=https://evil.example` and
/// `?next=//evil.example` (open redirect).
fn safe_next(next: Option<&str>) -> &str {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n,
        _ => "/admin",
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    pub next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
    pub next: Option<String>,
}

/// `GET /login`
#[debug_handler]
pub async fn new(
    ViewEngine(v): ViewEngine<TeraView>,
    State(_ctx): State<AppContext>,
    Query(q): Query<LoginQuery>,
) -> Result<Response> {
    crate::views::session::login(&v, safe_next(q.next.as_deref()), "", None, StatusCode::OK)
}

/// `POST /login` (form) — sets the cookie and redirects.
#[debug_handler]
pub async fn create(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Form(form): Form<LoginForm>,
) -> Result<Response> {
    let next = safe_next(form.next.as_deref());
    let Ok(user) = users::Model::authenticate(&ctx.db, &form.email, &form.password).await else {
        return crate::views::session::login(
            &v,
            next,
            &form.email,
            Some("Invalid email or password."),
            StatusCode::UNAUTHORIZED,
        );
    };

    let jwt = ctx.config.get_jwt_config()?;
    let token = user
        .generate_jwt(&jwt.secret, jwt.expiration)
        .or_else(|_| unauthorized("unauthorized!"))?;

    let settings = Settings::from_context(&ctx)?;
    let cookie = cookie::Cookie::build((AUTH_COOKIE, token))
        .path("/")
        .http_only(true)
        .secure(settings.cookie_secure)
        .same_site(cookie::SameSite::Lax)
        .build();

    format::render().cookies(&[cookie])?.redirect(next)
}

/// `POST /logout`
#[debug_handler]
pub async fn destroy(State(_ctx): State<AppContext>) -> Result<Response> {
    let cookie = cookie::Cookie::build((AUTH_COOKIE, ""))
        .path("/")
        .http_only(true)
        .removal()
        .build();
    format::render().cookies(&[cookie])?.redirect("/")
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/login", get(new))
        .add("/login", post(create))
        .add("/logout", post(destroy))
}
