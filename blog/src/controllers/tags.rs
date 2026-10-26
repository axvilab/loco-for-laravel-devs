//! Public tag pages.
use loco_rs::prelude::*;

use crate::{
    models::{posts, tags},
    views,
};

/// `GET /tags/{slug}?page=N` — published posts carrying a tag.
#[debug_handler]
pub async fn show(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(slug): Path<String>,
    Query(pagination): Query<query::PaginationQuery>,
) -> Result<Response> {
    let tag = match tags::Model::find_by_slug(&ctx.db, &slug).await {
        Ok(tag) => tag,
        Err(ModelError::EntityNotFound) => return views::posts::not_found(&v),
        Err(e) => return Err(e.into()),
    };
    let page = posts::Model::published_page_for_tag(&ctx.db, &tag, pagination.page).await?;
    let post_tags = posts::Model::load_tags(&ctx.db, &page.page).await?;
    let authors = posts::Model::load_authors(&ctx.db, &page.page).await?;
    views::posts::tag(&v, &tag, &page, &post_tags, &authors)
}

pub fn routes() -> Routes {
    Routes::new().add("/tags/{slug}", get(show))
}
