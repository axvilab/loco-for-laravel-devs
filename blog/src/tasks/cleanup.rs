//! `cargo loco task cleanup [min_age_minutes:60] [dry_run:true]`
//!
//! Deletes cover images in storage that no post references any more
//! (orphans from crashes between upload and DB update, or manual DB edits).
//! Files younger than `min_age_minutes` are kept: an upload in progress has
//! its file written a moment before the row points at it.
use std::path::Path;

use chrono::{Duration, Utc};
use loco_rs::prelude::*;

use crate::models::posts::{self, COVER_DIR};

pub struct Cleanup;

#[async_trait]
impl Task for Cleanup {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "cleanup".to_string(),
            detail: "Delete orphaned cover images (args: min_age_minutes:60 dry_run:true)"
                .to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &task::Vars) -> Result<()> {
        let min_age: i64 = vars
            .cli_arg("min_age_minutes")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let dry_run = vars.cli_arg("dry_run").is_ok_and(|v| v == "true");
        let cutoff = Utc::now() - Duration::minutes(min_age);

        let referenced = posts::Model::cover_keys(&ctx.db).await?;
        let entries = ctx.storage.list(Path::new(COVER_DIR), true).await?;

        let mut deleted = 0;
        for entry in entries.iter().filter(|e| !e.is_dir) {
            let key = entry.path.trim_start_matches('/');
            if referenced.contains(key) {
                continue;
            }
            if entry.last_modified.is_some_and(|t| t > cutoff) {
                continue; // too fresh: maybe an upload in flight
            }
            if dry_run {
                println!("would delete: {key}");
            } else {
                ctx.storage.delete(Path::new(key)).await?;
                deleted += 1;
            }
        }
        tracing::info!(
            scanned = entries.len(),
            deleted,
            dry_run,
            "cleanup finished"
        );
        println!("scanned {} entries, deleted {deleted}", entries.len());
        Ok(())
    }
}
