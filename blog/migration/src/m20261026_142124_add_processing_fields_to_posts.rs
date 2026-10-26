use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        add_column(m, "posts", "reading_minutes", ColType::BigIntegerNull).await?;
        add_column(m, "posts", "body_html", ColType::TextNull).await?;
        add_column(
            m,
            "posts",
            "notified_at",
            ColType::TimestampWithTimeZoneNull,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        remove_column(m, "posts", "reading_minutes").await?;
        remove_column(m, "posts", "body_html").await?;
        remove_column(m, "posts", "notified_at").await?;
        Ok(())
    }
}
