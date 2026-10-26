#![allow(non_upper_case_globals)]

use loco_rs::prelude::*;
use serde_json::json;

use crate::{
    data::settings::Settings,
    models::{posts, subscribers},
};

static shared: Dir<'_> = include_dir!("src/mailers/shared");
static new_post: Dir<'_> = include_dir!("src/mailers/post/new_post");

/// Generated as `pub struct Post {}`; renamed so it doesn't read like the
/// `posts` model at call sites (the starter's own is `AuthMailer`).
pub struct PostMailer {}
impl Mailer for PostMailer {}

impl PostMailer {
    /// "New post" notification to one subscriber.
    ///
    /// # Errors
    /// When rendering or enqueueing the email fails.
    pub async fn send_new_post(
        ctx: &AppContext,
        subscriber: &subscribers::Model,
        post: &posts::Model,
    ) -> Result<()> {
        let base = Settings::from_context(ctx)?.base_url;
        Self::mail_template_with_shared(
            ctx,
            &new_post,
            &[&shared],
            mailer::Args {
                to: subscriber.email.clone(),
                locals: json!({
                    "title": post.title,
                    "excerpt": post.excerpt(),
                    "url": format!("{base}{}", crate::views::posts::post_url(&post.slug)),
                    "reading_minutes": post.reading_minutes,
                    "unsubscribe_url": format!("{base}/unsubscribe/{}", subscriber.token),
                }),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }
}
