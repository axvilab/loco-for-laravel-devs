//! `cargo loco task seed_demo count:50`: realistic content for local
//! testing and load tests (Laravel: a factory + seeder).
use chrono::{Duration, Utc};
use loco_rs::prelude::*;

use crate::models::posts::{self, PostParams};

const TAGS: [&str; 6] = ["rust", "loco", "laravel", "sea-orm", "htmx", "testing"];

fn body(i: usize) -> String {
    let para = "Loco gives a Laravel developer the same shape of app in Rust: \
                models with validation, controllers, Tera views, workers, \
                mailers, a scheduler and a test harness. ";
    format!(
        "## Part {i}\n\n{}\n\n```rust\nfn main() {{ println!(\"post {i}\"); }}\n```\n\n\
         - one\n- two\n- three\n\n{}",
        para.repeat(6),
        para.repeat(8)
    )
}

pub struct SeedDemo;

#[async_trait]
impl Task for SeedDemo {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "seed_demo".to_string(),
            detail: "Create demo posts (args: count:50)".to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &task::Vars) -> Result<()> {
        let count: usize = vars
            .cli_arg("count")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);
        for i in 1..=count {
            let tags = format!("{}, {}", TAGS[i % TAGS.len()], TAGS[(i * 7) % TAGS.len()]);
            let post = posts::Model::create(
                &ctx.db,
                &PostParams {
                    title: format!("Demo post number {i}"),
                    body: body(i),
                    tags,
                    published_at: Some(
                        (Utc::now() - Duration::hours(i64::try_from(i).unwrap_or(0))).into(),
                    ),
                    ..Default::default()
                },
            )
            .await?;
            // derive html cache + reading time inline (no queue needed)
            post.refresh_derived(&ctx.db).await?;
        }
        println!("created {count} posts");
        Ok(())
    }
}
