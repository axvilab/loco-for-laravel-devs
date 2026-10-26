//! Public, server-rendered blog pages (cached, see `views::page_cache`).
use loco_rs::prelude::*;

use crate::{
    models::posts,
    views::{self, page_cache},
};

/// `GET /?page=N` — newest published posts.
#[debug_handler]
pub async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Query(pagination): Query<query::PaginationQuery>,
) -> Result<Response> {
    let page_no = pagination.page.max(1);
    let html = page_cache::html(&ctx.cache, &format!("index:{page_no}"), async {
        let page = posts::Model::published_page(&ctx.db, page_no).await?;
        let tags = posts::Model::load_tags(&ctx.db, &page.page).await?;
        let authors = posts::Model::load_authors(&ctx.db, &page.page).await?;
        views::posts::index(&v, &page, &tags, &authors)
    })
    .await?;
    page_cache::respond_html(&html)
}

/// `GET /posts/{slug}` — a single published post, HTML 404 otherwise.
#[debug_handler]
pub async fn show(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let rendered = page_cache::html(&ctx.cache, &format!("post:{slug}"), async {
        let post = posts::Model::find_published_by_slug(&ctx.db, &slug).await?;
        let tags = post.tags(&ctx.db).await?;
        let author = post.author(&ctx.db).await?;
        views::posts::show(&v, &post, &tags, author.as_ref())
    })
    .await;
    match rendered {
        Ok(html) => page_cache::respond_html(&html),
        // not cached: unknown slugs must not fill the cache
        Err(Error::Model(ModelError::EntityNotFound)) => views::posts::not_found(&v),
        Err(e) => Err(e),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/", get(index))
        .add("/posts/{slug}", get(show))
}
