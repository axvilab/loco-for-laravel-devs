use blog::app::App;
use loco_rs::testing::prelude::*;
use serial_test::serial;

use super::prepare_data::{browser_login, create_user, ADMIN_EMAIL, ADMIN_PASSWORD};

fn cookies() -> RequestConfig {
    RequestConfigBuilder::new().save_cookies(true).build()
}

#[tokio::test]
#[serial]
async fn login_page_renders() {
    request::<App, _, _>(|request, _ctx| async move {
        let res = request.get("/login?next=/admin").await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains(r#"name="next" value="/admin""#));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn login_sets_httponly_cookie_and_redirects() {
    request::<App, _, _>(|request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;

        let res = request
            .post("/login")
            .form(&[
                ("email", ADMIN_EMAIL),
                ("password", ADMIN_PASSWORD),
                ("next", "/admin"),
            ])
            .await;
        assert_eq!(res.status_code(), 303);
        assert_eq!(res.header("location"), "/admin");
        let set_cookie = res.header("set-cookie");
        let set_cookie = set_cookie.to_str().unwrap();
        assert!(set_cookie.starts_with("auth_token="));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn wrong_password_rerenders_form_with_401() {
    request::<App, _, _>(|request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;

        let res = request
            .post("/login")
            .form(&[("email", ADMIN_EMAIL), ("password", "wrong")])
            .await;
        assert_eq!(res.status_code(), 401);
        let html = res.text();
        assert!(html.contains("Invalid email or password."));
        assert!(html.contains(r#"value="admin@example.com""#), "email kept");
        assert!(res.maybe_header("set-cookie").is_none());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn next_param_cannot_redirect_off_site() {
    request::<App, _, _>(|request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        for evil in ["//evil.example", "https://evil.example", "/\\evil.example"] {
            let res = request
                .post("/login")
                .form(&[
                    ("email", ADMIN_EMAIL),
                    ("password", ADMIN_PASSWORD),
                    ("next", evil),
                ])
                .await;
            assert_eq!(res.header("location"), "/admin", "{evil}");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn cookie_authenticates_json_api_too() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        browser_login(&request, ADMIN_EMAIL).await;

        // `auth.jwt.location` lists Cookie first, then Bearer
        let res = request.get("/api/auth/current").await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains(ADMIN_EMAIL));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logout_clears_cookie() {
    request_with_config::<App, _, _>(cookies(), |request, ctx| async move {
        create_user(&ctx, ADMIN_EMAIL, "Admin").await;
        browser_login(&request, ADMIN_EMAIL).await;
        assert_eq!(request.get("/admin").await.status_code(), 200);

        let res = request.post("/logout").await;
        assert_eq!(res.status_code(), 303);
        assert!(res
            .header("set-cookie")
            .to_str()
            .unwrap()
            .contains("Max-Age=0"));

        assert_eq!(request.get("/admin").await.status_code(), 303);
    })
    .await;
}
