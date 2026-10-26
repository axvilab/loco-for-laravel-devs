//! `cargo loco task publish_scheduled [window_hours:24] [dry_run:true]`
//!
//! Posts with a future `published_at` become visible on their own (the
//! public queries compare with `now()`), but nothing *happens* at that
//! moment. This task — run every minute by the scheduler — finds posts that
//! went live and haven't been announced, and enqueues `process_post` for
//! each, which caches the HTML and emails subscribers.
use chrono::{Duration, Utc};
use loco_rs::prelude::*;

use crate::{models::posts, workers::process_post};

pub struct PublishScheduled;

#[async_trait]
impl Task for PublishScheduled {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "publish_scheduled".to_string(),
            detail:
                "Announce posts whose published_at has passed (args: window_hours:24 dry_run:true)"
                    .to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &task::Vars) -> Result<()> {
        let window_hours: i64 = vars
            .cli_arg("window_hours")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(24);
        let dry_run = vars.cli_arg("dry_run").is_ok_and(|v| v == "true");

        let since = (Utc::now() - Duration::hours(window_hours)).into();
        let due = posts::Model::due_for_announcement(&ctx.db, since).await?;
        tracing::info!(count = due.len(), window_hours, dry_run, "posts due");

        if dry_run || due.is_empty() {
            for post in &due {
                println!("would announce: {} ({})", post.title, post.slug);
            }
            return Ok(());
        }

        // one round trip, atomic (new in loco 1.2)
        let args: Vec<_> = due
            .iter()
            .map(|p| process_post::WorkerArgs { post_id: p.id })
            .collect();
        let ids = process_post::Worker::perform_all_later(ctx, args).await?;
        println!("enqueued {} job(s)", ids.len());
        Ok(())
    }
}
