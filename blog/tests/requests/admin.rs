use blog::{
    app::App,
    models::posts::{self, PostParams},
};
use loco_rs::testing::prelude::*;
use serial_test::serial;

use super::prepare_data::{browser_login, create_user, ADMIN_EMAIL};

fn cookies() -> RequestConfig {
    RequestConfigBuilder::new().save_cookies(true).build()
}

#[tokio::test]
#[serial]
async fn anonymous_browser_is_redirected_to_login() {
    request::<App, _, _>(|request, _ctx| async move {
        let res = request.get("/admin").await;
        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/login?next=/admin");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn anonymous_htmx_request_gets_hx_redirect() {
    request::<App, _, _>(|request, _ctx| async move {
        let res = request.get("/admin").add_header("HX-Request", "true").await;
        assert_eq!(res.status_code(), 401);
        assert_eq!(res.header("hx-redirect"), "/login?next=/admin");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn dashboard_lists_only_my_posts() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let me = create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        let other = create_user(&ctx, "other@example.com", "Other").await;
        for (title, author) in [("Mine", me.id), ("Theirs", other.id)] {
            posts::Model::create(
                &ctx.db,
                &PostParams {
                    title: title.into(),
                    body: "b".into(),
                    user_id: Some(author),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        browser_login(&request, ADMIN_EMAIL).await;
        let html = request.get("/admin").await.text();
        assert!(html.contains("Signed in as Admin"));
        assert!(html.contains("Mine"));
        assert!(!html.contains("Theirs"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn bearer_token_also_works_for_admin() {
    request::<App, _, _>(|request, ctx| async move {
        let user = create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        let jwt = ctx.config.get_jwt_config().unwrap();
        let token = user.generate_jwt(&jwt.secret, jwt.expiration).unwrap();

        let res = request.get("/admin").authorization_bearer(token).await;
        assert_eq!(res.status_code(), 200);
    })
    .await;
}
