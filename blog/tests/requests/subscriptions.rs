use blog::{app::App, models::subscribers};
use loco_rs::prelude::*;
use serial_test::serial;

use super::prepare_data::{browser_login, create_user, ADMIN_EMAIL};

#[tokio::test]
#[serial]
async fn subscribe_and_unsubscribe() {
    request::<App, _, _>(|request, ctx| async move {
        let res = request
            .post("/subscribe")
            .form(&[("email", "fan@example.com")])
            .await;
        assert_eq!(res.status_code(), 200);
        assert!(
            res.text().contains("You&#39;re subscribed"),
            "apostrophe is autoescaped"
        );

        let bad = request.post("/subscribe").form(&[("email", "nope")]).await;
        assert_eq!(bad.status_code(), 422);

        let sub = &subscribers::Model::all(&ctx.db).await.unwrap()[0];
        let res = request.get(&format!("/unsubscribe/{}", sub.token)).await;
        assert_eq!(res.status_code(), 200);
        assert!(subscribers::Model::all(&ctx.db).await.unwrap().is_empty());

        let again = request.get(&format!("/unsubscribe/{}", sub.token)).await;
        assert_eq!(again.status_code(), 404);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn publishing_from_admin_emails_subscribers() {
    let config = RequestConfigBuilder::new().save_cookies(true).build();
    request_with_config::<App, _, _>(config, |request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        subscribers::Model::subscribe(&ctx.db, "fan@example.com")
            .await
            .unwrap();
        browser_login(&request, ADMIN_EMAIL).await;

        let res = request
            .post("/admin/posts")
            .form(&[
                ("title", "Fresh"),
                ("body", "Hello subscribers"),
                ("published_at", "2026-01-01T00:00"),
            ])
            .await;
        assert_eq!(res.status_code(), 303);

        let deliveries = ctx.mailer.as_ref().unwrap().deliveries();
        assert_eq!(deliveries.count, 1);
        assert!(deliveries.messages[0].contains("To: fan@example.com"));
        assert!(deliveries.messages[0].contains("Subject: New post: Fresh"));

        // and the public page shows the worker's reading time
        let html = request.get("/posts/fresh").await.text();
        assert!(html.contains("1 min read"));
    })
    .await;
}
