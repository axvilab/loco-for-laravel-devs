//! Snapshot tests (insta). Rendered output is pinned to `.snap` files under
//! `tests/requests/snapshots/`; review changes with `cargo insta review`
//! (or `INSTA_UPDATE=always cargo test` + `git diff`).
use blog::{
    app::App,
    models::{
        posts::{self, PostParams},
        subscribers,
    },
    workers::process_post,
};
use chrono::{TimeZone, Utc};
use insta::{assert_snapshot, with_settings};
use loco_rs::{bgworker::BackgroundWorker, prelude::*};
use serial_test::serial;

use super::prepare_data::{browser_login, create_user, ADMIN_EMAIL};

const BODY: &str = "Intro with **bold**, `code` and a [link](https://loco.rs).\n\n\
## A heading\n\n| Laravel | Loco |\n|---|---|\n| Eloquent | Sea-ORM |\n\n\
```rust\nfn main() {}\n```\n\nA footnote.[^1]\n\n[^1]: The note.";

/// Fixed timestamps make the output deterministic.
async fn fixed_post(ctx: &AppContext) -> posts::Model {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: "Snapshot <post> & co".into(),
            body: BODY.into(),
            tags: "rust, loco".into(),
            published_at: Some(Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap().into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
#[serial]
async fn post_page_article() {
    request::<App, _, _>(|request, ctx| async move {
        fixed_post(&ctx).await;
        let html = request.get("/posts/snapshot-post-co").await.text();

        // structural assertions with loco's CSS-selector helpers ...
        assert_css_eq(&html, "article.post h1", "Snapshot <post> & co");
        assert_count(&html, "article.post .tags li", 2);
        assert_link(&html, ".tags a", "/tags/loco");

        // ... and the exact markup pinned by a snapshot
        let article = select(&html, "article.post").join("\n");
        with_settings!({ prepend_module_to_snapshot => false }, {
            assert_snapshot!("post_page_article", article);
        });
    })
    .await;
}

#[tokio::test]
#[serial]
async fn rss_feed_xml() {
    request::<App, _, _>(|request, ctx| async move {
        fixed_post(&ctx).await;
        let xml = request.get("/feed.xml").await.text();
        with_settings!({ prepend_module_to_snapshot => false }, {
            assert_snapshot!("rss_feed_xml", xml);
        });
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_form_errors_fragment() {
    let config = RequestConfigBuilder::new().save_cookies(true).build();
    request_with_config::<App, _, _>(config, |request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        browser_login(&request, ADMIN_EMAIL).await;
        let res = request
            .post("/admin/posts")
            .add_header("HX-Request", "true")
            .form(&[
                ("title", ""),
                ("slug", "Not A Slug"),
                ("body", ""),
                ("tags", "x"),
                ("published_at", "soon"),
            ])
            .await;
        assert_eq!(res.status_code(), 422);
        with_settings!({ prepend_module_to_snapshot => false }, {
            assert_snapshot!("admin_form_errors_fragment", res.text());
        });
    })
    .await;
}

#[tokio::test]
#[serial]
async fn new_post_email() {
    request::<App, _, _>(|_request, ctx| async move {
        let sub = subscribers::Model::subscribe(&ctx.db, "fan@example.com")
            .await
            .unwrap();
        // a fixed token: quoted-printable wraps long lines with `=\n`, which
        // would split a random UUID and defeat any redaction regex
        let mut sub = sub.into_active_model();
        sub.token = Set(Uuid::nil());
        sub.update(&ctx.db).await.unwrap();
        let post = fixed_post(&ctx).await;
        process_post::Worker::perform_later(&ctx, process_post::WorkerArgs { post_id: post.id })
            .await
            .unwrap();

        let deliveries = ctx.mailer.as_ref().unwrap().deliveries();
        assert_eq!(deliveries.count, 1);
        // Date and MIME boundaries change every run: redact them with the
        // filters loco ships for mail.
        // The HTML part must show `&lt;post&gt;` (escaped), see html.t.
        assert!(deliveries.messages[0].contains("Snapshot &lt;post&gt; &amp; co"));
        let filters: Vec<(&str, &str)> = get_cleanup_mail().clone();
        with_settings!({ filters => filters, prepend_module_to_snapshot => false }, {
            assert_snapshot!("new_post_email", deliveries.messages[0]);
        });
    })
    .await;
}

/// Starter-template issue: user-controlled `name` is rendered into the
/// HTML part of the welcome email. Mail templates (`.t`) are not
/// autoescaped, so it must be escaped explicitly.
#[tokio::test]
#[serial]
async fn welcome_email_escapes_user_name() {
    request::<App, _, _>(|request, ctx| async move {
        request
            .post("/api/auth/register")
            .json(&serde_json::json!({
                "name": "<img src=x onerror=alert(1)>",
                "email": "evil@example.com",
                "password": "12341234"
            }))
            .await;
        let deliveries = ctx.mailer.as_ref().unwrap().deliveries();
        assert_eq!(deliveries.count, 1);
        // only the HTML part matters; the text/plain part may show it raw
        let html_part = deliveries.messages[0]
            .split("Content-Type: text/html")
            .nth(1)
            .expect("multipart mail has an HTML part");
        assert!(
            !html_part.contains("<img src=3Dx"),
            "raw HTML from user input in email:\n{html_part}"
        );
        assert!(html_part.contains("&lt;img"));
    })
    .await;
}
