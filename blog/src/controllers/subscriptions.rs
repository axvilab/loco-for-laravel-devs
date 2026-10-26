//! Public newsletter: subscribe from the footer form, unsubscribe by token.
use axum::http::StatusCode;
use loco_rs::prelude::*;
use serde::Deserialize;

use crate::{models::subscribers, views};

#[derive(Debug, Deserialize)]
pub struct SubscribeForm {
    #[serde(default)]
    pub email: String,
}

/// `POST /subscribe`
#[debug_handler]
pub async fn create(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Form(form): Form<SubscribeForm>,
) -> Result<Response> {
    match subscribers::Model::subscribe(&ctx.db, &form.email).await {
        Ok(_) => views::subscriptions::result(
            &v,
            "You're subscribed",
            "You'll get an email when a new post is published.",
            StatusCode::OK,
        ),
        Err(ModelError::Validation(_)) => views::subscriptions::result(
            &v,
            "That didn't work",
            "Please enter a valid email address.",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        Err(e) => Err(e.into()),
    }
}

/// `GET /unsubscribe/{token}`
#[debug_handler]
pub async fn destroy(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(token): Path<Uuid>,
) -> Result<Response> {
    match subscribers::Model::unsubscribe(&ctx.db, token).await {
        Ok(_) => views::subscriptions::result(
            &v,
            "Unsubscribed",
            "You won't get any more emails from us.",
            StatusCode::OK,
        ),
        Err(ModelError::EntityNotFound) => views::posts::not_found(&v),
        Err(e) => Err(e.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/subscribe", post(create))
        .add("/unsubscribe/{token}", get(destroy))
}
