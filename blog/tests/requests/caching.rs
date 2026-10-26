use blog::{
    app::App,
    models::posts::{self, PostParams},
    views::page_cache,
};
use chrono::{Duration, Utc};
use loco_rs::prelude::*;
use serial_test::serial;

use super::prepare_data::{browser_login, create_user, ADMIN_EMAIL};

async fn live(ctx: &AppContext, title: &str) {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: title.into(),
            body: "b".into(),
            published_at: Some((Utc::now() - Duration::minutes(1)).into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

/// `config/test.yaml` has `cache: { kind: InMem }`. Writing straight through
/// the model bypasses invalidation, which makes the cache observable.
#[tokio::test]
#[serial]
async fn index_is_cached_until_invalidated() {
    request::<App, _, _>(|request, ctx| async move {
        live(&ctx, "First").await;
        assert!(request.get("/").await.text().contains("First"));

        live(&ctx, "Second").await; // no invalidation
        let cached = request.get("/").await.text();
        assert!(!cached.contains("Second"), "served from cache");

        page_cache::invalidate(&ctx.cache).await;
        assert!(request.get("/").await.text().contains("Second"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn unknown_slugs_are_not_cached_and_feed_is() {
    request::<App, _, _>(|request, ctx| async move {
        assert_eq!(request.get("/posts/later").await.status_code(), 404);
        live(&ctx, "Later").await;
        // the 404 above was not cached, so the new post shows immediately
        assert_eq!(request.get("/posts/later").await.status_code(), 200);

        let feed1 = request.get("/feed.xml").await;
        assert_eq!(
            feed1.header("content-type"),
            "application/rss+xml; charset=utf-8"
        );
        live(&ctx, "Not yet in feed").await;
        assert!(!request
            .get("/feed.xml")
            .await
            .text()
            .contains("Not yet in feed"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_writes_invalidate_public_pages() {
    let config = RequestConfigBuilder::new().save_cookies(true).build();
    request_with_config::<App, _, _>(config, |request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        browser_login(&request, ADMIN_EMAIL).await;
        assert!(request.get("/").await.text().contains("No posts yet."));

        request
            .post("/admin/posts")
            .form(&[
                ("title", "Via admin"),
                ("body", "b"),
                ("published_at", "2026-01-01T00:00"),
            ])
            .await;
        assert!(request.get("/").await.text().contains("Via admin"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn etag_gives_304_on_revalidation() {
    request::<App, _, _>(|request, ctx| async move {
        live(&ctx, "Tagged").await;
        let first = request.get("/").await;
        let etag = first.header("etag");
        assert!(etag.to_str().unwrap().starts_with('"'));

        let again = request
            .get("/")
            .add_header("If-None-Match", etag.to_str().unwrap())
            .await;
        assert_eq!(again.status_code(), 304);
        assert!(again.text().is_empty());

        let feed = request.get("/feed.xml").await;
        let feed_etag = feed.header("etag");
        let feed_again = request
            .get("/feed.xml")
            .add_header("If-None-Match", feed_etag.to_str().unwrap())
            .await;
        assert_eq!(feed_again.status_code(), 304);
    })
    .await;
}
