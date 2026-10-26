use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Hooks, Initializer},
    bgworker::{BackgroundWorker, Queue},
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    db::{self, truncate_table},
    environment::Environment,
    storage::{self, drivers},
    task::Tasks,
    Result,
};
use migration::Migrator;
use std::path::Path;

#[allow(unused_imports)]
use crate::{
    controllers, initializers,
    models::_entities::{post_tags, posts, subscribers, tags, users},
    tasks,
    workers::downloader::DownloadWorker,
};

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    /// Storage has no YAML config in Loco: it's wired here, in code.
    ///
    /// * tests: in-memory (nothing touches the disk, nothing to clean up)
    /// * otherwise: local disk rooted at `assets/static`, so a key like
    ///   `uploads/covers/x.png` is served by the `static` middleware at
    ///   `/static/uploads/covers/x.png`.
    async fn after_context(ctx: AppContext) -> Result<AppContext> {
        let driver = if ctx.environment == Environment::Test {
            drivers::mem::new()
        } else {
            drivers::local::new_with_prefix("assets/static")?
        };
        Ok(ctx
            .into_builder()
            .storage(storage::Storage::single(driver).into())
            .build())
    }

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![Box::new(
            initializers::view_engine::ViewEngineInitializer,
        )])
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes() // controller routes below
            .add_route(controllers::feeds::routes())
            .add_route(controllers::subscriptions::routes())
            .add_route(controllers::admin::routes())
            .add_route(controllers::session::routes())
            .add_route(controllers::tags::routes())
            .add_route(controllers::posts::routes())
            .add_route(controllers::auth::routes())
    }
    async fn connect_workers(ctx: &AppContext, queue: &Queue) -> Result<()> {
        queue
            .register(crate::workers::process_post::Worker::build(ctx))
            .await?;
        queue.register(DownloadWorker::build(ctx)).await?;
        Ok(())
    }

    #[allow(unused_variables)]
    fn register_tasks(tasks: &mut Tasks) {
        tasks.register(tasks::publish_scheduled::PublishScheduled);
        tasks.register(tasks::cleanup::Cleanup);
        // tasks-inject (do not remove)
        tasks.register(tasks::user_create::UserCreate);
        tasks.register(tasks::user_delete::UserDelete);
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        // children before parents once foreign keys exist
        truncate_table(&ctx.db, subscribers::Entity).await?;
        truncate_table(&ctx.db, post_tags::Entity).await?;
        truncate_table(&ctx.db, tags::Entity).await?;
        truncate_table(&ctx.db, posts::Entity).await?;
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
    async fn seed(ctx: &AppContext, base: &Path) -> Result<()> {
        db::seed::<users::ActiveModel>(&ctx.db, &base.join("users.yaml").display().to_string())
            .await?;
        db::seed::<posts::ActiveModel>(&ctx.db, &base.join("posts.yaml").display().to_string())
            .await?;
        db::seed::<tags::ActiveModel>(&ctx.db, &base.join("tags.yaml").display().to_string())
            .await?;
        db::seed::<post_tags::ActiveModel>(
            &ctx.db,
            &base.join("post_tags.yaml").display().to_string(),
        )
        .await?;
        Ok(())
    }
}
