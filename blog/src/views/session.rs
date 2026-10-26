use axum::http::StatusCode;
use loco_rs::prelude::*;

/// `GET /login`, and the re-rendered form after a failed `POST /login`.
///
/// # Errors
/// Template rendering errors.
pub fn login(
    v: &impl ViewRenderer,
    next: &str,
    email: &str,
    error: Option<&str>,
    status: StatusCode,
) -> Result<Response> {
    format::render().status(status).view(
        v,
        "session/login.html",
        data!({ "next": next, "email": email, "error": error }),
    )
}
