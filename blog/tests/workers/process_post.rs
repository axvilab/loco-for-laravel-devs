use blog::{
    app::App,
    models::{
        posts::{self, PostParams},
        subscribers,
    },
    workers::process_post::{Worker, WorkerArgs},
};
use chrono::{Duration, Utc};
use loco_rs::{bgworker::BackgroundWorker, prelude::*};
use serial_test::serial;

async fn post(ctx: &AppContext, title: &str, published: bool) -> posts::Model {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: title.into(),
            body: "word ".repeat(450),
            published_at: published.then(|| (Utc::now() - Duration::minutes(5)).into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

/// `config/test.yaml` has `workers.mode: ForegroundBlocking` and
/// `mailer.stub: true`: `perform_later` runs the job inline, and mail is
/// recorded in memory instead of sent.
#[tokio::test]
#[serial]
async fn renders_cache_and_notifies_subscribers_once() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = &boot.app_context;
    subscribers::Model::subscribe(&ctx.db, "a@example.com")
        .await
        .unwrap();
    subscribers::Model::subscribe(&ctx.db, "b@example.com")
        .await
        .unwrap();
    let p = post(ctx, "Live post", true).await;

    Worker::perform_later(ctx, WorkerArgs { post_id: p.id })
        .await
        .unwrap();

    let p = posts::Model::find_by_id(&ctx.db, p.id).await.unwrap();
    assert_eq!(p.reading_minutes, Some(3));
    assert!(p.body_html.as_deref().unwrap().starts_with("<p>word word"));
    assert!(p.notified_at.is_some());

    let deliveries = ctx.mailer.as_ref().unwrap().deliveries();
    assert_eq!(deliveries.count, 2);
    let first = &deliveries.messages[0];
    assert!(first.contains("Subject: New post: Live post"));
    assert!(first.contains("http://localhost:5150/posts/live-post"));
    assert!(first.contains("/unsubscribe/"));

    // running the job again (retry, scheduler, double click) sends nothing
    Worker::perform_later(ctx, WorkerArgs { post_id: p.id })
        .await
        .unwrap();
    assert_eq!(ctx.mailer.as_ref().unwrap().deliveries().count, 2);
}

#[tokio::test]
#[serial]
async fn drafts_are_processed_but_not_announced() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = &boot.app_context;
    subscribers::Model::subscribe(&ctx.db, "a@example.com")
        .await
        .unwrap();
    let p = post(ctx, "Draft", false).await;

    Worker::perform_later(ctx, WorkerArgs { post_id: p.id })
        .await
        .unwrap();

    let p = posts::Model::find_by_id(&ctx.db, p.id).await.unwrap();
    assert_eq!(p.reading_minutes, Some(3));
    assert!(p.notified_at.is_none());
    assert_eq!(ctx.mailer.as_ref().unwrap().deliveries().count, 0);
}

#[tokio::test]
#[serial]
async fn missing_post_is_not_an_error() {
    let boot = boot_test::<App>().await.unwrap();
    assert!(
        Worker::perform_later(&boot.app_context, WorkerArgs { post_id: 999_999 })
            .await
            .is_ok()
    );
}
