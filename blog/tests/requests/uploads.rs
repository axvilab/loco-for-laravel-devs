use std::path::Path;

use axum_test::multipart::{MultipartForm, Part};
use blog::{
    app::App,
    models::posts::{self, PostParams},
};
use loco_rs::prelude::*;
use serial_test::serial;

use super::prepare_data::{browser_login, create_user, ADMIN_EMAIL};

/// A valid 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn cookies() -> RequestConfig {
    RequestConfigBuilder::new().save_cookies(true).build()
}

fn form(bytes: &[u8], name: &str) -> MultipartForm {
    MultipartForm::new().add_part(
        "cover",
        Part::bytes(bytes.to_vec())
            .file_name(name)
            .mime_type("image/png"),
    )
}

async fn setup(ctx: &AppContext) -> posts::Model {
    let me = create_user(ctx, ADMIN_EMAIL, "Admin").await;
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: "With cover".into(),
            body: "b".into(),
            published_at: Some(chrono::Utc::now().into()),
            user_id: Some(me.id),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[test]
fn sniffs_real_image_types_only() {
    assert_eq!(posts::sniff_image(PNG), Some("png"));
    assert_eq!(posts::sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
    assert_eq!(posts::sniff_image(b"GIF89a...."), Some("gif"));
    assert_eq!(posts::sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
    assert_eq!(posts::sniff_image(b"<?php echo 1; ?>"), None);
    assert_eq!(posts::sniff_image(b""), None);
}

#[tokio::test]
#[serial]
async fn upload_stores_file_and_shows_cover() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let post = setup(&ctx).await;
        browser_login(&request, ADMIN_EMAIL).await;

        let res = request
            .post(&format!("/admin/posts/{}/cover", post.id))
            .add_header("HX-Request", "true")
            .multipart(form(PNG, "me.png"))
            .await;
        assert_eq!(res.status_code(), 200);

        let post = posts::Model::find_by_id(&ctx.db, post.id).await.unwrap();
        let key = post.cover_image.clone().expect("cover saved");
        assert!(key.starts_with(&format!("uploads/covers/{}-", post.id)));
        assert!(key.ends_with(".png"));
        assert!(res.text().contains(&format!("/static/{key}")));

        // bytes really are in ctx.storage (the in-memory driver in tests)
        let stored: Vec<u8> = ctx.storage.download(Path::new(&key)).await.unwrap();
        assert_eq!(stored, PNG);

        let public = request.get("/posts/with-cover").await.text();
        assert!(public.contains(&format!(r#"<img class="cover" src="/static/{key}""#)));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn non_image_is_rejected_with_422() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let post = setup(&ctx).await;
        browser_login(&request, ADMIN_EMAIL).await;

        let res = request
            .post(&format!("/admin/posts/{}/cover", post.id))
            .add_header("HX-Request", "true")
            .multipart(form(b"<?php system($_GET['c']); ?>", "shell.png"))
            .await;
        assert_eq!(res.status_code(), 422);
        assert!(res.text().contains("Only PNG, JPEG, GIF or WebP images."));
        let post = posts::Model::find_by_id(&ctx.db, post.id).await.unwrap();
        assert!(post.cover_image.is_none());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn replacing_and_removing_cleans_up_old_files() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let post = setup(&ctx).await;
        browser_login(&request, ADMIN_EMAIL).await;
        let url = format!("/admin/posts/{}/cover", post.id);

        request.post(&url).multipart(form(PNG, "a.png")).await;
        let first = posts::Model::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .cover_image
            .unwrap();

        // plain (no-JS) form: 303 back to the edit page
        let res = request.post(&url).multipart(form(PNG, "b.png")).await;
        assert_eq!(res.status_code(), 303);
        let second = posts::Model::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .cover_image
            .unwrap();
        assert_ne!(first, second);
        assert!(!ctx.storage.exists(Path::new(&first)).await.unwrap());
        assert!(ctx.storage.exists(Path::new(&second)).await.unwrap());

        request
            .post(&format!("/admin/posts/{}/cover/delete", post.id))
            .await;
        assert!(!ctx.storage.exists(Path::new(&second)).await.unwrap());
        assert!(posts::Model::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .cover_image
            .is_none());
    })
    .await;
}
