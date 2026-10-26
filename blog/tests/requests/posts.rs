use blog::{
    app::App,
    models::posts::{self, PostParams},
};
use chrono::{Duration, Utc};
use loco_rs::{app::AppContext, testing::prelude::*};
use serial_test::serial;

/// Create a post published `hours_ago` hours ago (`None` = draft).
pub async fn make_post(ctx: &AppContext, title: &str, hours_ago: Option<i64>) -> posts::Model {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: title.to_string(),
            body: format!("Body of **{title}**."),
            published_at: hours_ago.map(|h| (Utc::now() - Duration::hours(h)).into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
#[serial]
async fn index_shows_empty_state() {
    request::<App, _, _>(|request, _ctx| async move {
        let res = request.get("/").await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("No posts yet."));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn index_lists_published_posts_newest_first() {
    request::<App, _, _>(|request, ctx| async move {
        make_post(&ctx, "Older post", Some(48)).await;
        make_post(&ctx, "Newer post", Some(1)).await;
        make_post(&ctx, "Secret draft", None).await;

        let res = request.get("/").await;
        assert_eq!(res.status_code(), 200);
        let html = res.text();
        let newer = html.find("Newer post").expect("newer post listed");
        let older = html.find("Older post").expect("older post listed");
        assert!(newer < older, "newest first");
        assert!(!html.contains("Secret draft"));
        assert!(html.contains(r#"href="/posts/newer-post""#));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn index_paginates() {
    request::<App, _, _>(|request, ctx| async move {
        for i in 1..=7 {
            make_post(&ctx, &format!("Post number {i}"), Some(i)).await;
        }

        let page1 = request.get("/").await.text();
        assert_eq!(page1.matches("class=\"post-card\"").count(), 5);
        assert!(page1.contains("Page 1 of 2"));
        assert!(page1.contains(r#"href="/?page=2""#));

        let page2 = request.get("/?page=2").await.text();
        assert_eq!(page2.matches("class=\"post-card\"").count(), 2);
        assert!(page2.contains("Post number 7"));
        assert!(page2.contains(r#"rel="prev" href="/""#));

        // page_size from the query string is ignored (fixed server-side)
        let big = request.get("/?page=1&page_size=1000").await.text();
        assert_eq!(big.matches("class=\"post-card\"").count(), 5);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn show_renders_markdown_and_escapes_raw_html() {
    request::<App, _, _>(|request, ctx| async move {
        posts::Model::create(
            &ctx.db,
            &PostParams {
                title: "Markdown <test>".into(),
                body: "Hello **bold**\n\n<script>alert(1)</script>".into(),
                published_at: Some((Utc::now() - Duration::hours(1)).into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let res = request.get("/posts/markdown-test").await;
        assert_eq!(res.status_code(), 200);
        let html = res.text();
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("&lt;script&gt;"), "raw HTML must be escaped");
        assert!(!html.contains("<script>alert"));
        // the title is autoescaped by Tera
        assert!(html.contains("<h1>Markdown &lt;test&gt;</h1>"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn show_returns_html_404_for_drafts_and_unknown_slugs() {
    request::<App, _, _>(|request, ctx| async move {
        make_post(&ctx, "Draft", None).await;
        make_post(&ctx, "Tomorrow", Some(-24)).await;

        for path in ["/posts/draft", "/posts/tomorrow", "/posts/nope"] {
            let res = request.get(path).await;
            assert_eq!(res.status_code(), 404, "{path}");
            assert!(res.text().contains("404 — not found"), "{path}");
        }
    })
    .await;
}
