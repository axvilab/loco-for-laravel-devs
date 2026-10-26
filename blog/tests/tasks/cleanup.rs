use std::path::Path;

use blog::{
    app::App,
    models::posts::{self, PostParams},
};
use loco_rs::{boot::run_task, prelude::*, task};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn deletes_only_unreferenced_covers() {
    let boot = boot_test::<App>().await.unwrap();
    let ctx = &boot.app_context;

    let post = posts::Model::create(
        &ctx.db,
        &PostParams {
            title: "Has cover".into(),
            body: "b".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let png = axum::body::Bytes::from_static(b"\x89PNG\r\n\x1a\n....");
    let post = post
        .attach_cover(&ctx.db, &ctx.storage, &png)
        .await
        .unwrap();
    let kept = post.cover_image.clone().unwrap();
    let orphan = "uploads/covers/999-orphan.png";
    ctx.storage.upload(Path::new(orphan), &png).await.unwrap();

    let name = "cleanup".to_string();
    let dry = task::Vars::from_cli_args(vec![
        ("min_age_minutes".into(), "0".into()),
        ("dry_run".into(), "true".into()),
    ]);
    run_task::<App>(ctx, Some(&name), &dry).await.unwrap();
    assert!(ctx.storage.exists(Path::new(orphan)).await.unwrap());

    let real = task::Vars::from_cli_args(vec![("min_age_minutes".into(), "0".into())]);
    run_task::<App>(ctx, Some(&name), &real).await.unwrap();
    assert!(!ctx.storage.exists(Path::new(orphan)).await.unwrap());
    assert!(ctx.storage.exists(Path::new(&kept)).await.unwrap());
}
