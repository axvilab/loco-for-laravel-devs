#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;
mod m20220101_000001_users;

mod m20261026_132628_posts;
mod m20261026_134726_tags;
mod m20261026_135117_create_join_table_posts_and_tags;
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_users::Migration),
            Box::new(m20261026_132628_posts::Migration),
            Box::new(m20261026_134726_tags::Migration),
            Box::new(m20261026_135117_create_join_table_posts_and_tags::Migration),
            // inject-above (do not remove this comment)
        ]
    }
}
