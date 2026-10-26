use blog::{
    app::App,
    models::posts::{self, PostParams},
};
use loco_rs::prelude::*;
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

async fn my_post(ctx: &loco_rs::app::AppContext, user_id: i64, title: &str) -> posts::Model {
    posts::Model::create(
        &ctx.db,
        &PostParams {
            title: title.into(),
            body: "body".into(),
            tags: "rust".into(),
            user_id: Some(user_id),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
#[serial]
async fn create_via_plain_form_redirects_with_flash() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let me = create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        browser_login(&request, ADMIN_EMAIL).await;

        let res = request
            .post("/admin/posts")
            .form(&[
                ("title", "From the form"),
                ("slug", ""),
                ("body", "**hi**"),
                ("tags", "rust, admin"),
                ("published_at", "2026-01-02T03:04"),
            ])
            .await;
        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/admin");

        let post = posts::Model::find_by_slug(&ctx.db, "from-the-form")
            .await
            .unwrap();
        assert_eq!(post.user_id, Some(me.id), "author comes from the session");
        assert_eq!(post.tag_list(&ctx.db).await.unwrap(), "admin, rust");
        assert_eq!(
            post.published_at.unwrap().to_rfc3339(),
            "2026-01-02T03:04:00+00:00"
        );

        // the flash shows once, then it's gone
        let first = request.get("/admin").await.text();
        assert!(first.contains("Created “From the form”."));
        let second = request.get("/admin").await.text();
        assert!(!second.contains("Created “From the form”."));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn invalid_form_is_422_with_all_field_errors() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        browser_login(&request, ADMIN_EMAIL).await;

        let form = [
            ("title", ""),
            ("slug", "Bad Slug"),
            ("body", ""),
            ("tags", ""),
            ("published_at", "tomorrow"),
        ];

        // plain form: full page
        let res = request.post("/admin/posts").form(&form).await;
        assert_eq!(res.status_code(), 422);
        let html = res.text();
        assert!(html.contains("<html"));
        for msg in [
            "Title must be 1–200 characters.",
            "Slug may contain only a-z, 0-9 and single dashes.",
            "Body can&#39;t be empty.",
            "Use the format YYYY-MM-DDTHH:MM.",
        ] {
            assert!(html.contains(msg), "missing: {msg}");
        }
        assert!(html.contains(r#"value="Bad Slug""#), "input is kept");

        // htmx: only the <form> fragment
        let res = request
            .post("/admin/posts")
            .add_header("HX-Request", "true")
            .form(&form)
            .await;
        assert_eq!(res.status_code(), 422);
        let html = res.text();
        assert!(html.trim_start().starts_with("<form"));
        assert!(!html.contains("<html"));
        assert_eq!(posts::Entity::find().count(&ctx.db).await.unwrap(), 0);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn taken_slug_is_a_field_error_not_a_500() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let me = create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        my_post(&ctx, me.id, "Existing").await;
        browser_login(&request, ADMIN_EMAIL).await;

        let res = request
            .post("/admin/posts")
            .form(&[("title", "New"), ("slug", "existing"), ("body", "b")])
            .await;
        assert_eq!(res.status_code(), 422);
        assert!(res
            .text()
            .contains("This slug is already used by another post."));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn update_via_htmx_put() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let me = create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        let post = my_post(&ctx, me.id, "Before").await;
        browser_login(&request, ADMIN_EMAIL).await;

        let edit = request.get(&format!("/admin/posts/{}/edit", post.id)).await;
        assert_eq!(edit.status_code(), 200);
        assert!(edit.text().contains(r#"value="Before""#));

        let res = request
            .put(&format!("/admin/posts/{}", post.id))
            .add_header("HX-Request", "true")
            .form(&[
                ("title", "After"),
                ("slug", "before"),
                ("body", "new body"),
                ("tags", "loco"),
                ("published_at", ""),
            ])
            .await;
        assert_eq!(res.status_code(), 200);
        assert_eq!(res.header("hx-redirect"), "/admin");

        let post = posts::Model::find_by_id(&ctx.db, post.id).await.unwrap();
        assert_eq!(post.title, "After");
        assert_eq!(post.slug, "before", "slug kept");
        assert_eq!(post.tag_list(&ctx.db).await.unwrap(), "loco");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn toggle_publish_returns_row_with_oob_flash() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let me = create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        let post = my_post(&ctx, me.id, "Toggle me").await;
        browser_login(&request, ADMIN_EMAIL).await;

        let res = request
            .post(&format!("/admin/posts/{}/publish", post.id))
            .add_header("HX-Request", "true")
            .await;
        assert_eq!(res.status_code(), 200);
        let html = res.text();
        assert!(html
            .trim_start()
            .starts_with(&format!(r#"<tr id="post-{}">"#, post.id)));
        assert!(html.contains("Unpublish"));
        assert!(html.contains(r#"hx-swap-oob="true">Published “Toggle me”."#));
        assert!(posts::Model::find_by_id(&ctx.db, post.id)
            .await
            .unwrap()
            .is_published());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn delete_via_htmx_and_plain_form() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        let me = create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        let a = my_post(&ctx, me.id, "Gone A").await;
        let b = my_post(&ctx, me.id, "Gone B").await;
        browser_login(&request, ADMIN_EMAIL).await;

        let res = request
            .delete(&format!("/admin/posts/{}", a.id))
            .add_header("HX-Request", "true")
            .await;
        assert_eq!(res.status_code(), 200);
        assert!(res
            .text()
            .contains(r#"hx-swap-oob="true">Deleted “Gone A”."#));

        let res = request.post(&format!("/admin/posts/{}/delete", b.id)).await;
        assert_eq!(res.status_code(), 303);

        assert_eq!(posts::Entity::find().count(&ctx.db).await.unwrap(), 0);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn cannot_touch_someone_elses_post() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        let other = create_user(&ctx, "other@example.com", "Other").await;
        let theirs = my_post(&ctx, other.id, "Theirs").await;
        browser_login(&request, ADMIN_EMAIL).await;

        let id = theirs.id;
        assert_eq!(
            request
                .get(&format!("/admin/posts/{id}/edit"))
                .await
                .status_code(),
            404
        );
        assert_eq!(
            request
                .put(&format!("/admin/posts/{id}"))
                .form(&[("title", "hacked"), ("body", "x")])
                .await
                .status_code(),
            404
        );
        assert_eq!(
            request
                .delete(&format!("/admin/posts/{id}"))
                .await
                .status_code(),
            404
        );
        assert_eq!(
            request
                .post(&format!("/admin/posts/{id}/publish"))
                .await
                .status_code(),
            404
        );
        assert_eq!(
            posts::Model::find_by_id(&ctx.db, id).await.unwrap().title,
            "Theirs"
        );
    })
    .await;
}
