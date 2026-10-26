use blog::{app::App, models::posts};
use loco_rs::{boot::run_task, prelude::*, task};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn seeds_published_posts_with_tags_and_html() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = &boot.app_context;
    let vars = task::Vars::from_cli_args(vec![("count".into(), "3".into())]);
    run_task::<App>(ctx, Some(&"seed_demo".to_string()), &vars)
        .await
        .unwrap();

    let all = posts::Model::published(&ctx.db).await.unwrap();
    assert_eq!(all.len(), 3);
    assert!(all
        .iter()
        .all(|p| p.body_html.is_some() && p.reading_minutes.is_some()));
    assert!(!all[0].tags(&ctx.db).await.unwrap().is_empty());
}
