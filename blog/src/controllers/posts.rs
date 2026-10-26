//! Public, server-rendered blog pages.
use loco_rs::prelude::*;

use crate::{models::posts, views};

/// `GET /?page=N` — newest published posts.
#[debug_handler]
pub async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Query(pagination): Query<query::PaginationQuery>,
) -> Result<Response> {
    let page = posts::Model::published_page(&ctx.db, pagination.page).await?;
    let tags = posts::Model::load_tags(&ctx.db, &page.page).await?;
    let authors = posts::Model::load_authors(&ctx.db, &page.page).await?;
    views::posts::index(&v, &page, &tags, &authors)
}

/// `GET /posts/{slug}` — a single published post, HTML 404 otherwise.
#[debug_handler]
pub async fn show(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(slug): Path<String>,
) -> Result<Response> {
    match posts::Model::find_published_by_slug(&ctx.db, &slug).await {
        Ok(post) => {
            let tags = post.tags(&ctx.db).await?;
            let author = post.author(&ctx.db).await?;
            views::posts::show(&v, &post, &tags, author.as_ref())
        }
        Err(ModelError::EntityNotFound) => views::posts::not_found(&v),
        Err(e) => Err(e.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/", get(index))
        .add("/posts/{slug}", get(show))
}
