use blog::{
    app::App,
    models::posts::{self, PostParams},
};
use chrono::{Duration, Utc};
use loco_rs::prelude::*;
use serial_test::serial;

async fn post(ctx: &AppContext, title: &str, tags: &str, published: bool) {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: title.into(),
            body: "Some **bold** text".into(),
            tags: tags.into(),
            published_at: published.then(|| (Utc::now() - Duration::hours(2)).into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
#[serial]
async fn rss_feed_lists_published_posts_as_xml() {
    request::<App, _, _>(|request, ctx| async move {
        post(&ctx, "Fish & <Chips>", "", true).await;
        post(&ctx, "Hidden draft", "", false).await;

        let res = request.get("/feed.xml").await;
        assert_eq!(res.status_code(), 200);
        assert_eq!(
            res.header("content-type"),
            "application/rss+xml; charset=utf-8"
        );
        let xml = res.text();
        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(
            xml.contains("<title>Fish &amp; &lt;Chips&gt;</title>"),
            "escaped"
        );
        assert!(xml.contains("<link>http://localhost:5150/posts/fish-chips</link>"));
        // the HTML body travels escaped inside <description>
        // (Tera 2 doesn't escape `/`; Tera 1 wrote `&#x2F;`)
        assert!(xml.contains("&lt;strong&gt;bold&lt;/strong&gt;"));
        assert!(!xml.contains("Hidden draft"));
        // RFC 2822 date
        assert!(xml.contains("<pubDate>") && xml.contains(" +0000</pubDate>"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn sitemap_has_home_posts_and_tags() {
    request::<App, _, _>(|request, ctx| async move {
        post(&ctx, "Mapped", "rust", true).await;
        post(&ctx, "Unmapped draft", "secret-tag", false).await;

        let res = request.get("/sitemap.xml").await;
        assert_eq!(res.status_code(), 200);
        assert_eq!(res.header("content-type"), "application/xml; charset=utf-8");
        let xml = res.text();
        for loc in [
            "http://localhost:5150/",
            "http://localhost:5150/posts/mapped",
            "http://localhost:5150/tags/rust",
        ] {
            assert!(xml.contains(&format!("<loc>{loc}</loc>")), "missing {loc}");
        }
        assert!(!xml.contains("unmapped-draft"));
        assert!(
            !xml.contains("secret-tag"),
            "tags of drafts only are not listed"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn robots_txt_points_to_sitemap() {
    request::<App, _, _>(|request, _ctx| async move {
        let res = request.get("/robots.txt").await;
        assert_eq!(res.status_code(), 200);
        assert!(res
            .text()
            .contains("Sitemap: http://localhost:5150/sitemap.xml"));
        assert!(res.text().contains("Disallow: /admin"));
    })
    .await;
}
