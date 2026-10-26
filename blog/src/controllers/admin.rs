//! `/admin` — everything here requires a logged-in user (`CurrentUser`).
use loco_rs::prelude::*;

use crate::{controllers::session::CurrentUser, models::posts, views};

/// `GET /admin` — dashboard.
#[debug_handler]
pub async fn index(
    CurrentUser(user): CurrentUser,
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let posts = posts::Model::by_author(&ctx.db, user.id).await?;
    views::admin::dashboard(&v, &user, &posts)
}

pub fn routes() -> Routes {
    Routes::new().prefix("/admin").add("/", get(index))
}
