use chrono::Utc;
use loco_rs::prelude::*;
use serde::Deserialize;
use validator::ValidationError;

pub use super::_entities::posts::{self, ActiveModel, Column, Entity, Model};
pub type Posts = Entity;

/// Longest slug we generate from a title (before a `-N` suffix is added).
const MAX_SLUG_LEN: usize = 80;

/// Input for creating/updating a post. Used by the admin (step 06) and tests.
/// `slug` is optional: when empty, it is derived from the title.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct PostParams {
    pub title: String,
    #[serde(default)]
    pub slug: String,
    pub body: String,
    #[serde(default)]
    pub published_at: Option<DateTimeWithTimeZone>,
}

/// Field rules. `ActiveModel` fields are `ActiveValue<T>`, so the `validator`
/// derive cannot sit on the entity — this plain struct carries the rules.
#[derive(Debug, Validate, Deserialize)]
pub struct Validator {
    #[validate(length(min = 1, max = 200, message = "Title must be 1–200 characters."))]
    pub title: String,
    #[validate(custom(function = "validate_slug"))]
    pub slug: String,
    #[validate(length(min = 1, message = "Body can't be empty."))]
    pub body: String,
}

fn validate_slug(slug: &str) -> Result<(), ValidationError> {
    let ok = !slug.is_empty()
        && slug.len() <= MAX_SLUG_LEN + 10
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-');
    if ok {
        Ok(())
    } else {
        let mut err = ValidationError::new("slug");
        err.message = Some("Slug may contain only a-z, 0-9 and single dashes.".into());
        Err(err)
    }
}

impl Validatable for ActiveModel {
    fn validator(&self) -> Box<dyn Validate> {
        Box::new(Validator {
            title: value_or_default(&self.title),
            slug: value_or_default(&self.slug),
            body: value_or_default(&self.body),
        })
    }
}

/// `ActiveValue::as_ref()` panics on `NotSet`; validation must not.
fn value_or_default<T: Clone + Default + Into<sea_orm::Value>>(v: &ActiveValue<T>) -> T {
    match v {
        ActiveValue::Set(x) | ActiveValue::Unchanged(x) => x.clone(),
        ActiveValue::NotSet => T::default(),
    }
}

/// "Hello, Wörld! Rust 2026" -> "hello-w-rld-rust-2026"
///
/// Deliberately ASCII-only and dependency-free. (Laravel's `Str::slug`
/// transliterates; add the `slug`/`deunicode` crate if you need that.)
#[must_use]
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut dash = false;
    for c in input.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let mut s = out.trim_end_matches('-').to_string();
    if s.len() > MAX_SLUG_LEN {
        s.truncate(MAX_SLUG_LEN);
        s = s.trim_end_matches('-').to_string();
    }
    if s.is_empty() {
        "post".to_string()
    } else {
        s
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    /// Runs on every `insert`/`update` (the Eloquent `saving` event).
    ///
    /// 1. derive a slug from the title when none was given
    /// 2. make the slug unique (`my-post`, `my-post-2`, ...)
    /// 3. validate — the last line of defence; controllers validate first
    ///    so they can render errors (see `PostParams::validate_for_save`)
    /// 4. bump `updated_at` (generated code)
    async fn before_save<C>(self, db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let mut this = self;

        let current_slug = value_or_default(&this.slug);
        if current_slug.trim().is_empty() {
            let base = slugify(&value_or_default(&this.title));
            let id = match &this.id {
                ActiveValue::Set(id) | ActiveValue::Unchanged(id) => Some(*id),
                ActiveValue::NotSet => None,
            };
            let unique = unique_slug(db, &base, id).await?;
            this.slug = ActiveValue::Set(unique);
        }

        this.validate()?;

        if !insert && this.updated_at.is_unchanged() {
            this.updated_at = ActiveValue::Set(Utc::now().into());
        }
        Ok(this)
    }
}

/// Find a free slug: `base`, `base-2`, `base-3`, ... ignoring the row itself.
async fn unique_slug<C: ConnectionTrait>(
    db: &C,
    base: &str,
    except_id: Option<i64>,
) -> std::result::Result<String, DbErr> {
    let mut candidate = base.to_string();
    let mut n = 1;
    loop {
        let mut q = Entity::find().filter(Column::Slug.eq(&candidate));
        if let Some(id) = except_id {
            q = q.filter(Column::Id.ne(id));
        }
        if q.one(db).await?.is_none() {
            return Ok(candidate);
        }
        n += 1;
        candidate = format!("{base}-{n}");
    }
}

impl PostParams {
    /// Validate *before* touching the DB so the caller gets
    /// `ModelError::Validation` (structured, per field) instead of the
    /// `DbErr::Custom(json)` that a failing `before_save` produces.
    ///
    /// # Errors
    /// `ModelError::Validation` with per-field messages.
    pub fn validate_for_save(&self) -> ModelResult<()> {
        let slug = if self.slug.trim().is_empty() {
            slugify(&self.title)
        } else {
            self.slug.trim().to_string()
        };
        let v = Validator {
            title: self.title.trim().to_string(),
            slug,
            body: self.body.clone(),
        };
        // Both `validator::Validate` and loco's blanket `ValidatorTrait` are in
        // the prelude, so `v.validate()` is ambiguous (E0034). Loco's version
        // already returns `ModelValidationErrors`.
        ValidatorTrait::validate(&v).map_err(ModelError::Validation)
    }

    fn apply(&self, item: &mut ActiveModel) {
        item.title = ActiveValue::Set(self.title.trim().to_string());
        item.slug = ActiveValue::Set(self.slug.trim().to_string());
        item.body = ActiveValue::Set(self.body.clone());
        item.published_at = ActiveValue::Set(self.published_at);
    }
}

impl Model {
    /// The one definition of "publicly visible". Every public query goes
    /// through [`Model::published_condition`] so this can change in one place.
    #[must_use]
    pub fn is_published(&self) -> bool {
        self.published_at.is_some_and(|at| at <= Utc::now())
    }

    #[must_use]
    pub fn published_condition() -> sea_orm::Condition {
        sea_orm::Condition::all()
            .add(Column::PublishedAt.is_not_null())
            .add(Column::PublishedAt.lte(Utc::now()))
    }

    /// # Errors
    /// `ModelError::EntityNotFound` when no post has this slug.
    pub async fn find_by_slug(db: &DatabaseConnection, slug: &str) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::Slug.eq(slug))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// Like `find_by_slug`, but drafts and scheduled posts are "not found".
    ///
    /// # Errors
    /// `ModelError::EntityNotFound`
    pub async fn find_published_by_slug(db: &DatabaseConnection, slug: &str) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::Slug.eq(slug))
            .filter(Self::published_condition())
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// # Errors
    /// `ModelError::EntityNotFound`
    pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// Newest published first.
    ///
    /// # Errors
    /// DB errors.
    pub async fn published(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .filter(Self::published_condition())
            .order_by_desc(Column::PublishedAt)
            .all(db)
            .await?)
    }

    /// Validate, then insert. Slug is derived in `before_save` when empty.
    ///
    /// # Errors
    /// `ModelError::Validation` for bad input, DB errors otherwise.
    pub async fn create(db: &DatabaseConnection, params: &PostParams) -> ModelResult<Self> {
        params.validate_for_save()?;
        let mut item = ActiveModel {
            ..Default::default()
        };
        params.apply(&mut item);
        Ok(item.insert(db).await?)
    }
}

impl ActiveModel {
    /// # Errors
    /// `ModelError::Validation` for bad input, DB errors otherwise.
    pub async fn update_from(
        mut self,
        db: &DatabaseConnection,
        params: &PostParams,
    ) -> ModelResult<Model> {
        params.validate_for_save()?;
        params.apply(&mut self);
        Ok(self.update(db).await?)
    }

    /// Publish now.
    ///
    /// # Errors
    /// DB errors.
    pub async fn publish(mut self, db: &DatabaseConnection) -> ModelResult<Model> {
        self.published_at = ActiveValue::Set(Some(Utc::now().into()));
        Ok(self.update(db).await?)
    }

    /// # Errors
    /// DB errors.
    pub async fn unpublish(mut self, db: &DatabaseConnection) -> ModelResult<Model> {
        self.published_at = ActiveValue::Set(None);
        Ok(self.update(db).await?)
    }
}
