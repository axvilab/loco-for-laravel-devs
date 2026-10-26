use chrono::Utc;
use loco_rs::prelude::*;
use serde::Deserialize;

pub use super::_entities::tags::{self, ActiveModel, Column, Entity, Model};
use super::posts::slugify;
pub type Tags = Entity;

#[derive(Debug, Validate, Deserialize)]
pub struct Validator {
    #[validate(length(min = 1, max = 40, message = "Tag must be 1–40 characters."))]
    pub name: String,
}

impl Validatable for ActiveModel {
    fn validator(&self) -> Box<dyn Validate> {
        let name = match &self.name {
            ActiveValue::Set(n) | ActiveValue::Unchanged(n) => n.clone(),
            ActiveValue::NotSet => String::new(),
        };
        Box::new(Validator { name })
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        self.validate()?;
        let mut this = self;
        if insert && this.slug.is_not_set() {
            if let ActiveValue::Set(name) = &this.name {
                this.slug = ActiveValue::Set(slugify(name));
            }
        }
        if !insert && this.updated_at.is_unchanged() {
            this.updated_at = ActiveValue::Set(Utc::now().into());
        }
        Ok(this)
    }
}

/// "rust, Loco ,, web" -> ["rust", "Loco", "web"] (trimmed, de-duplicated by slug)
#[must_use]
pub fn parse_list(input: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(slugify(s)))
        .map(ToString::to_string)
        .collect()
}

impl Model {
    /// # Errors
    /// `ModelError::EntityNotFound`
    pub async fn find_by_slug(db: &DatabaseConnection, slug: &str) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::Slug.eq(slug))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// `firstOrCreate` by slug, so "Rust" and "rust" are the same tag.
    /// Generic over the connection so it can run inside a transaction.
    ///
    /// # Errors
    /// DB errors / validation.
    pub async fn find_or_create<C: ConnectionTrait>(db: &C, name: &str) -> ModelResult<Self> {
        let slug = slugify(name);
        if let Some(tag) = Entity::find()
            .filter(Column::Slug.eq(&slug))
            .one(db)
            .await?
        {
            return Ok(tag);
        }
        Ok(ActiveModel {
            name: ActiveValue::Set(name.trim().to_string()),
            slug: ActiveValue::Set(slug),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// All tags, alphabetical.
    ///
    /// # Errors
    /// DB errors.
    pub async fn all_sorted(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
        Ok(Entity::find().order_by_asc(Column::Name).all(db).await?)
    }
}
