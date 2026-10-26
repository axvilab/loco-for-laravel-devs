use axum::http::StatusCode;
use loco_rs::prelude::*;

/// A one-message page for subscribe/unsubscribe outcomes.
///
/// # Errors
/// Template rendering errors.
pub fn result(
    v: &impl ViewRenderer,
    title: &str,
    message: &str,
    status: StatusCode,
) -> Result<Response> {
    format::render().status(status).view(
        v,
        "subscriptions/result.html",
        data!({ "title": title, "message": message }),
    )
}
