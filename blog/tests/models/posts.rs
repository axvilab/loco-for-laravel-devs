use blog::{
    app::App,
    models::posts::{self, slugify, PostParams},
};
use chrono::{Duration, Utc};
use loco_rs::prelude::*;
use rstest::rstest;
use serial_test::serial;

fn params(title: &str) -> PostParams {
    PostParams {
        title: title.to_string(),
        body: "Some *markdown* body".to_string(),
        ..Default::default()
    }
}

/// The generated "entity and migration agree" test, kept.
#[tokio::test]
#[serial]
async fn can_query_posts() {
    let boot = boot_test::<App>().await.unwrap();
    posts::Entity::find()
        .all(&boot.app_context.db)
        .await
        .expect("`posts` should be queryable — entity and migration must agree");
}

#[rstest]
#[case("Hello World", "hello-world")]
#[case("  Rust 2026: what's new?  ", "rust-2026-what-s-new")]
#[case("Привіт, світ", "post")]
#[case("Wörld---Cup", "w-rld-cup")]
#[case("!!!", "post")]
fn slugify_cases(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(slugify(input), expected);
}

#[tokio::test]
#[serial]
async fn create_derives_unique_slug() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let a = posts::Model::create(db, &params("Hello Loco"))
        .await
        .unwrap();
    let b = posts::Model::create(db, &params("Hello Loco"))
        .await
        .unwrap();
    let c = posts::Model::create(db, &params("Hello Loco"))
        .await
        .unwrap();

    assert_eq!(a.slug, "hello-loco");
    assert_eq!(b.slug, "hello-loco-2");
    assert_eq!(c.slug, "hello-loco-3");
}

#[tokio::test]
#[serial]
async fn explicit_slug_is_kept_and_survives_title_change() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("First title");
    p.slug = "custom-slug".into();
    let post = posts::Model::create(db, &p).await.unwrap();
    assert_eq!(post.slug, "custom-slug");

    let mut p2 = p.clone();
    p2.title = "Renamed".into();
    let updated = post.into_active_model().update_from(db, &p2).await.unwrap();
    assert_eq!(updated.title, "Renamed");
    assert_eq!(updated.slug, "custom-slug");
}

#[tokio::test]
#[serial]
async fn create_returns_structured_validation_errors() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut p = params("");
    p.slug = "Bad Slug!".into();
    p.body = String::new();

    let err = posts::Model::create(db, &p).await.unwrap_err();
    let ModelError::Validation(errors) = err else {
        panic!("expected ModelError::Validation, got {err:?}");
    };
    let mut fields: Vec<_> = errors.errors.keys().cloned().collect();
    fields.sort();
    assert_eq!(fields, vec!["body", "slug", "title"]);
}

/// Documents a Loco behaviour: validation that fails inside `before_save`
/// surfaces as `DbErr::Custom(<json>)`, i.e. `ModelError::DbErr` (HTTP 500),
/// NOT `ModelError::Validation` (HTTP 400). That's why `Model::create`
/// validates first.
#[tokio::test]
#[serial]
async fn before_save_validation_becomes_db_err_custom() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let res = posts::ActiveModel {
        title: Set("ok".into()),
        slug: Set("NOT VALID".into()),
        body: Set("b".into()),
        ..Default::default()
    }
    .insert(db)
    .await;

    match res {
        Err(DbErr::Custom(json)) => {
            assert!(json.contains("slug"), "{json}");
        }
        other => panic!("expected DbErr::Custom, got {other:?}"),
    }
}

#[tokio::test]
#[serial]
async fn published_excludes_drafts_and_scheduled() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let mut live = params("Live");
    live.published_at = Some((Utc::now() - Duration::hours(1)).into());
    let mut older = params("Older");
    older.published_at = Some((Utc::now() - Duration::days(2)).into());
    let mut future = params("Future");
    future.published_at = Some((Utc::now() + Duration::days(1)).into());
    let draft = params("Draft");

    for p in [&live, &older, &future, &draft] {
        posts::Model::create(db, p).await.unwrap();
    }

    let titles: Vec<String> = posts::Model::published(db)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.title)
        .collect();
    assert_eq!(titles, vec!["Live", "Older"]);

    assert!(posts::Model::find_published_by_slug(db, "future")
        .await
        .is_err());
    assert!(posts::Model::find_published_by_slug(db, "live")
        .await
        .is_ok());
    assert!(posts::Model::find_by_slug(db, "draft").await.is_ok());
}

#[tokio::test]
#[serial]
async fn publish_and_unpublish() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;

    let post = posts::Model::create(db, &params("Toggle")).await.unwrap();
    assert!(!post.is_published());

    let post = post.into_active_model().publish(db).await.unwrap();
    assert!(post.is_published());

    let post = post.into_active_model().unpublish(db).await.unwrap();
    assert!(!post.is_published());
}

#[test]
fn markdown_renders_and_escapes_html() {
    let html = posts::render_markdown(
        "# Title\n\nSome **bold** <b>raw</b>\n\n| a | b |\n|---|---|\n| 1 | 2 |",
    );
    assert!(html.contains("<h1>Title</h1>"));
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("&lt;b&gt;raw&lt;/b&gt;"));
    assert!(html.contains("<table>"));
}

#[test]
fn excerpt_skips_tables_and_truncates_on_word_boundary() {
    let md = "First paragraph here.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nSecond paragraph.";
    assert_eq!(
        posts::excerpt(md, 220),
        "First paragraph here. Second paragraph."
    );
    let long = "word ".repeat(100);
    let e = posts::excerpt(&long, 22);
    assert_eq!(e, "word word word word…");
}

#[tokio::test]
#[serial]
async fn post_belongs_to_author() {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    let db = &boot.app_context.db;

    let post = posts::Model::find_by_slug(db, "hello-loco").await.unwrap();
    let author = post
        .author(db)
        .await
        .unwrap()
        .expect("seeded post has an author");
    assert_eq!(author.email, "user1@example.com");

    let all = posts::Model::by_author(db, author.id).await.unwrap();
    assert_eq!(all.len(), 2);
    let authors = posts::Model::load_authors(db, &all).await.unwrap();
    assert!(authors
        .iter()
        .all(|a| a.as_ref().map(|u| u.id) == Some(author.id)));
}
