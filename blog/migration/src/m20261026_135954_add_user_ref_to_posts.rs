use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

// Hand-edited. The generated `add_reference(m, "posts", "user?", "")` fails:
// `add_reference` ignores the `?` and always adds `BIGINT NOT NULL`, which
// SQLite refuses on ALTER TABLE ("Cannot add a NOT NULL column with default
// value NULL"). On SQLite it also skips the FOREIGN KEY entirely (SQLite can't
// add constraints to an existing table).
//
// So: a nullable `user_id`, an index, and a real FK where the backend can
// add one (Postgres in production).
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        add_column(m, "posts", "user_id", ColType::BigIntegerNull).await?;

        if m.get_database_backend() != sea_orm::DatabaseBackend::Sqlite {
            m.create_foreign_key(
                ForeignKey::create()
                    .name("fk-posts-user_id-to-users")
                    .from(Alias::new("posts"), Alias::new("user_id"))
                    .to(Alias::new("users"), Alias::new("id"))
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        }

        m.create_index(
            Index::create()
                .name("idx-posts-user_id")
                .table(Alias::new("posts"))
                .col(Alias::new("user_id"))
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx-posts-user_id")
                .table(Alias::new("posts"))
                .to_owned(),
        )
        .await?;
        if m.get_database_backend() != sea_orm::DatabaseBackend::Sqlite {
            m.drop_foreign_key(
                ForeignKey::drop()
                    .name("fk-posts-user_id-to-users")
                    .table(Alias::new("posts"))
                    .to_owned(),
            )
            .await?;
        }
        remove_column(m, "posts", "user_id").await
    }
}
