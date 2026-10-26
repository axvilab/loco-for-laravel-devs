use blog::{
    app::App,
    models::{
        posts::{self, PostParams},
        subscribers,
    },
};
use chrono::{Duration, Utc};
use loco_rs::{boot::run_task, prelude::*, task};
use serial_test::serial;

async fn live_post(ctx: &AppContext, title: &str, published_ago: Duration) -> posts::Model {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: title.into(),
            body: "body".into(),
            published_at: Some((Utc::now() - published_ago).into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

fn vars(pairs: &[(&str, &str)]) -> task::Vars {
    task::Vars::from_cli_args(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
    )
}

#[tokio::test]
#[serial]
async fn announces_recently_published_posts_once() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = &boot.app_context;
    subscribers::Model::subscribe(&ctx.db, "fan@example.com")
        .await
        .unwrap();
    // created via the model (no worker ran), as if it was scheduled earlier
    let fresh = live_post(ctx, "Just went live", Duration::minutes(1)).await;
    let ancient = live_post(ctx, "From the archive", Duration::days(30)).await;

    let name = "publish_scheduled".to_string();
    run_task::<App>(ctx, Some(&name), &task::Vars::default())
        .await
        .unwrap();

    // ForegroundBlocking: the batch-enqueued jobs ran inline
    let fresh = posts::Model::find_by_id(&ctx.db, fresh.id).await.unwrap();
    assert!(fresh.notified_at.is_some());
    assert_eq!(fresh.reading_minutes, Some(1));
    let ancient = posts::Model::find_by_id(&ctx.db, ancient.id).await.unwrap();
    assert!(ancient.notified_at.is_none(), "outside the 24h window");
    assert_eq!(ctx.mailer.as_ref().unwrap().deliveries().count, 1);

    // second run: nothing left to do
    run_task::<App>(ctx, Some(&name), &task::Vars::default())
        .await
        .unwrap();
    assert_eq!(ctx.mailer.as_ref().unwrap().deliveries().count, 1);
}

#[tokio::test]
#[serial]
async fn dry_run_and_window_arguments() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = &boot.app_context;
    let old = live_post(ctx, "Two days old", Duration::days(2)).await;
    let name = "publish_scheduled".to_string();

    run_task::<App>(
        ctx,
        Some(&name),
        &vars(&[("window_hours", "72"), ("dry_run", "true")]),
    )
    .await
    .unwrap();
    let still = posts::Model::find_by_id(&ctx.db, old.id).await.unwrap();
    assert!(still.notified_at.is_none(), "dry run changes nothing");

    run_task::<App>(ctx, Some(&name), &vars(&[("window_hours", "72")]))
        .await
        .unwrap();
    let done = posts::Model::find_by_id(&ctx.db, old.id).await.unwrap();
    assert!(done.notified_at.is_some());
}

#[test]
fn scheduler_config_lists_both_jobs() {
    let config = loco_rs::scheduler::Scheduler::from_config::<App>(
        std::path::Path::new("config/scheduler.yaml"),
        &loco_rs::environment::Environment::Test,
    )
    .expect("config/scheduler.yaml parses");
    let mut names: Vec<_> = config.jobs.keys().cloned().collect();
    names.sort();
    assert_eq!(names, vec!["cleanup", "publish_scheduled"]);
    assert_eq!(config.jobs["publish_scheduled"].run, "publish_scheduled");
}
