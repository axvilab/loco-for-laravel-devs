use blog::{
    app::App,
    models::{
        posts::{self, PostParams},
        subscribers,
    },
};
use chrono::Utc;
use loco_rs::prelude::*;
use serial_test::serial;

#[test]
fn reading_minutes_rounds_up_with_minimum_one() {
    assert_eq!(posts::reading_minutes_for(""), 1);
    assert_eq!(posts::reading_minutes_for(&"w ".repeat(200)), 1);
    assert_eq!(posts::reading_minutes_for(&"w ".repeat(201)), 2);
}

#[tokio::test]
#[serial]
async fn subscribe_is_idempotent_and_normalizes_email() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let a = subscribers::Model::subscribe(db, " Reader@Example.COM ")
        .await
        .unwrap();
    let b = subscribers::Model::subscribe(db, "reader@example.com")
        .await
        .unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(a.email, "reader@example.com");
    assert!(matches!(
        subscribers::Model::subscribe(db, "not-an-email").await,
        Err(ModelError::Validation(_))
    ));

    subscribers::Model::unsubscribe(db, a.token).await.unwrap();
    assert!(subscribers::Model::all(db).await.unwrap().is_empty());
}

#[tokio::test]
#[serial]
async fn claim_notification_succeeds_once_and_only_when_published() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let draft = posts::Model::create(
        db,
        &PostParams {
            title: "t".into(),
            body: "b".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!posts::Model::claim_notification(db, draft.id)
        .await
        .unwrap());

    let live = draft.into_active_model().publish(db).await.unwrap();
    assert!(live.published_at.unwrap() <= Utc::now());
    assert!(posts::Model::claim_notification(db, live.id).await.unwrap());
    assert!(!posts::Model::claim_notification(db, live.id).await.unwrap());
}
