use blog::{
    app::App,
    models::{
        _entities::post_tags,
        posts::{self, PostParams},
        tags,
    },
};
use loco_rs::prelude::*;
use serial_test::serial;

fn params(title: &str, tags: &str) -> PostParams {
    PostParams {
        title: title.into(),
        body: "body".into(),
        tags: tags.into(),
        ..Default::default()
    }
}

#[test]
fn parse_list_trims_and_dedupes() {
    assert_eq!(
        tags::parse_list(" rust, Loco ,, web , RUST,loco"),
        vec!["rust", "Loco", "web"]
    );
    assert!(tags::parse_list(" , ").is_empty());
}

#[tokio::test]
#[serial]
async fn find_or_create_is_case_insensitive_by_slug() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let a = tags::Model::find_or_create(db, "Rust").await.unwrap();
    let b = tags::Model::find_or_create(db, "rust").await.unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(a.slug, "rust");
    assert_eq!(b.name, "Rust", "first spelling wins");
}

#[tokio::test]
#[serial]
async fn create_and_update_sync_tags() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let post = posts::Model::create(db, &params("Tagged", "rust, loco"))
        .await
        .unwrap();
    let names: Vec<_> = post
        .tags(db)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["loco", "rust"]);

    let post = post
        .into_active_model()
        .update_from(db, &params("Tagged", "rust, web"))
        .await
        .unwrap();
    let names: Vec<_> = post
        .tags(db)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["rust", "web"]);

    // "loco" tag still exists (tags are shared), it's just detached
    assert!(tags::Model::find_by_slug(db, "loco").await.is_ok());
}

#[tokio::test]
#[serial]
async fn load_tags_is_index_aligned() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let p1 = posts::Model::create(db, &params("One", "b, a"))
        .await
        .unwrap();
    let p2 = posts::Model::create(db, &params("Two", "")).await.unwrap();
    let p3 = posts::Model::create(db, &params("Three", "c"))
        .await
        .unwrap();

    let loaded = posts::Model::load_tags(db, &[p1, p2, p3]).await.unwrap();
    let names: Vec<Vec<String>> = loaded
        .into_iter()
        .map(|ts| ts.into_iter().map(|t| t.name).collect())
        .collect();
    assert_eq!(names, vec![vec!["a", "b"], vec![], vec!["c"]]);
}

/// The join table's FKs are `ON DELETE CASCADE`; SQLite only enforces
/// that when `PRAGMA foreign_keys=ON` — which sqlx enables by default.
#[tokio::test]
#[serial]
async fn deleting_a_post_cascades_to_post_tags() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let post = posts::Model::create(db, &params("Doomed", "x, y"))
        .await
        .unwrap();
    assert_eq!(post_tags::Entity::find().count(db).await.unwrap(), 2);

    post.delete(db).await.unwrap();
    assert_eq!(post_tags::Entity::find().count(db).await.unwrap(), 0);
}
