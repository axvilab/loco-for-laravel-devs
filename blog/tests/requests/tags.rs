use blog::{
    app::App,
    models::posts::{self, PostParams},
};
use chrono::{Duration, Utc};
use loco_rs::testing::prelude::*;
use serial_test::serial;

async fn post(ctx: &loco_rs::app::AppContext, title: &str, tags: &str, published: bool) {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: title.into(),
            body: "body".into(),
            tags: tags.into(),
            published_at: published.then(|| (Utc::now() - Duration::hours(1)).into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
#[serial]
async fn tag_page_lists_only_published_posts_with_that_tag() {
    request::<App, _, _>(|request, ctx| async move {
        post(&ctx, "Rusty", "rust, web", true).await;
        post(&ctx, "Webby", "web", true).await;
        post(&ctx, "Rust draft", "rust", false).await;

        let res = request.get("/tags/rust").await;
        assert_eq!(res.status_code(), 200);
        let html = res.text();
        assert!(html.contains("Posts tagged #rust"));
        assert!(html.contains("Rusty"));
        assert!(!html.contains("Webby"));
        assert!(!html.contains("Rust draft"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn unknown_tag_is_html_404() {
    request::<App, _, _>(|request, _ctx| async move {
        let res = request.get("/tags/nope").await;
        assert_eq!(res.status_code(), 404);
        assert!(res.text().contains("404 — not found"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn index_and_show_link_tags() {
    request::<App, _, _>(|request, ctx| async move {
        post(&ctx, "Linked", "loco", true).await;

        let index = request.get("/").await.text();
        assert!(index.contains(r#"<a href="/tags/loco">#loco</a>"#));

        let show = request.get("/posts/linked").await.text();
        assert!(show.contains(r#"<a href="/tags/loco">#loco</a>"#));
    })
    .await;
}
