use axum::http::{HeaderName, HeaderValue};
use blog::{models::users, views::auth::LoginResponse};
use loco_rs::{app::AppContext, TestServer};

const USER_EMAIL: &str = "test@loco.com";
const USER_PASSWORD: &str = "1234";

pub struct LoggedInUser {
    pub user: users::Model,
    pub token: String,
}

pub async fn init_user_login(request: &TestServer, ctx: &AppContext) -> LoggedInUser {
    let register_payload = serde_json::json!({
        "name": "loco",
        "email": USER_EMAIL,
        "password": USER_PASSWORD
    });

    //Creating a new user
    request
        .post("/api/auth/register")
        .json(&register_payload)
        .await;
    let user = users::Model::find_by_email(&ctx.db, USER_EMAIL)
        .await
        .unwrap();

    let verify_payload = serde_json::json!({
        "token": user.email_verification_token,
    });

    request.post("/api/auth/verify").json(&verify_payload).await;

    let response = request
        .post("/api/auth/login")
        .json(&serde_json::json!({
            "email": USER_EMAIL,
            "password": USER_PASSWORD
        }))
        .await;

    let login_response: LoginResponse = serde_json::from_str(&response.text()).unwrap();

    LoggedInUser {
        user: users::Model::find_by_email(&ctx.db, USER_EMAIL)
            .await
            .unwrap(),
        token: login_response.token,
    }
}

pub fn auth_header(token: &str) -> (HeaderName, HeaderValue) {
    let auth_header_value = HeaderValue::from_str(&format!("Bearer {token}")).unwrap();

    (HeaderName::from_static("authorization"), auth_header_value)
}

pub const ADMIN_EMAIL: &str = "admin@example.com";
pub const ADMIN_PASSWORD: &str = "secret123";

/// A user created through the model (no HTTP), for browser-login tests.
pub async fn create_user(ctx: &AppContext, email: &str, name: &str) -> users::Model {
    users::Model::create_with_password(
        &ctx.db,
        &users::RegisterParams {
            email: email.to_string(),
            password: ADMIN_PASSWORD.to_string(),
            name: name.to_string(),
        },
    )
    .await
    .unwrap()
}

/// Log in through the HTML form. With `save_cookies(true)` the server keeps
/// the `auth_token` cookie for the following requests.
pub async fn browser_login(request: &TestServer, email: &str) {
    let res = request
        .post("/login")
        .form(&[
            ("email", email),
            ("password", ADMIN_PASSWORD),
            ("next", "/admin"),
        ])
        .await;
    assert_eq!(res.status_code(), 303, "login failed: {}", res.text());
}
