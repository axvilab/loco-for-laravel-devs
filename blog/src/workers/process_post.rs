//! Runs after every post save (create, update, publish toggle) and from the
//! `publish_scheduled` task:
//!
//! 1. render the Markdown once and cache it in `posts.body_html`, compute
//!    `reading_minutes`;
//! 2. if the post is live and subscribers haven't been told yet, claim the
//!    notification atomically and email every subscriber.
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    mailers::post::PostMailer,
    models::{posts, subscribers},
};

pub struct Worker {
    pub ctx: AppContext,
}

/// Job arguments are serialized into the queue: pass ids, not models.
#[derive(Deserialize, Debug, Serialize)]
pub struct WorkerArgs {
    pub post_id: i64,
}

#[async_trait]
impl BackgroundWorker<WorkerArgs> for Worker {
    fn build(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }

    fn class_name() -> String {
        "ProcessPost".to_string()
    }

    fn tags() -> Vec<String> {
        Vec::new()
    }

    async fn perform(&self, args: WorkerArgs) -> Result<()> {
        let db = &self.ctx.db;
        let post = match posts::Model::find_by_id(db, args.post_id).await {
            Ok(post) => post,
            // deleted before the job ran: nothing to do, not a failure
            Err(ModelError::EntityNotFound) => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        let post = post.refresh_derived(db).await?;
        // body_html / reading time changed. Effective when the worker runs in
        // the web process (`--server-and-worker`); otherwise the TTL bounds it.
        crate::views::page_cache::invalidate(&self.ctx.cache).await;
        tracing::info!(
            post_id = post.id,
            minutes = post.reading_minutes,
            "post processed"
        );

        if post.is_published() && posts::Model::claim_notification(db, post.id).await? {
            let subs = subscribers::Model::all(db).await?;
            for sub in &subs {
                PostMailer::send_new_post(&self.ctx, sub, &post).await?;
            }
            tracing::info!(
                post_id = post.id,
                count = subs.len(),
                "subscribers notified"
            );
        }
        Ok(())
    }
}
