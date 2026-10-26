use chrono::Utc;
use loco_rs::prelude::*;
use serde::Deserialize;
use validator::ValidationError;

pub use super::_entities::posts::{self, ActiveModel, Column, Entity, Model};
use super::_entities::{post_tags, tags, users};
use sea_orm::{DatabaseTransaction, LoaderTrait};
pub type Posts = Entity;

/// Longest slug we generate from a title (before a `-N` suffix is added).
const MAX_SLUG_LEN: usize = 80;

/// Posts per page on the public index.
pub const PER_PAGE: u64 = 5;

/// Characters in a list-page excerpt.
const EXCERPT_LEN: usize = 220;

/// Storage "directory" for cover images. Keys look like
/// `uploads/covers/12-<uuid>.png`; the local driver is rooted at
/// `assets/static`, so the file is served at `/static/<key>`.
pub const COVER_DIR: &str = "uploads/covers";

/// Largest accepted cover. (The request-wide cap is `limit_payload` in config.)
pub const MAX_COVER_BYTES: usize = 4 * 1024 * 1024;

/// Detect an image by its magic bytes — never trust the client's filename
/// or `Content-Type`. Returns the file extension to store it under.
#[must_use]
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some("png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some("gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

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
    /// Comma-separated tag names: "rust, loco".
    #[serde(default)]
    pub tags: String,
    /// Author. Never read from a form (`skip`): the controller sets it from
    /// the logged-in user.
    #[serde(skip)]
    pub user_id: Option<i64>,
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

/// `posts.user_id -> users.id`, written by hand.
///
/// `sea-orm-cli` derives relations from FOREIGN KEY constraints. SQLite can't
/// add one to an existing table, so the regenerated entity has `user_id` but
/// no `Relation::Users`. This is the `belongsTo(User::class)` it couldn't see.
impl sea_orm::Related<users::Entity> for Entity {
    fn to() -> sea_orm::RelationDef {
        Entity::belongs_to(users::Entity)
            .from(Column::UserId)
            .to(users::Column::Id)
            .into()
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

/// A one-field `ModelError::Validation`, same shape the `validator` crate
/// produces, so forms render it like any other field error.
#[must_use]
pub fn field_error(field: &str, code: &str, message: &str) -> ModelError {
    let mut errors = std::collections::BTreeMap::new();
    errors.insert(
        field.to_string(),
        vec![validation::ValidationError {
            code: code.to_string(),
            message: Some(message.to_string()),
            params: std::collections::HashMap::new(),
        }],
    );
    ModelError::Validation(validation::ModelValidationErrors { errors })
}

/// An explicitly chosen slug must be free. (Derived slugs get `-2`, `-3`...
/// in `before_save`; a slug the user typed is respected or rejected.)
async fn ensure_slug_free(
    db: &DatabaseConnection,
    slug: &str,
    except_id: Option<i64>,
) -> ModelResult<()> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Ok(());
    }
    let mut q = Entity::find().filter(Column::Slug.eq(slug));
    if let Some(id) = except_id {
        q = q.filter(Column::Id.ne(id));
    }
    if q.one(db).await?.is_some() {
        return Err(field_error(
            "slug",
            "taken",
            "This slug is already used by another post.",
        ));
    }
    Ok(())
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
        // derived fields are recomputed by the `process_post` worker
        item.body_html = ActiveValue::Set(None);
        item.reading_minutes = ActiveValue::Set(None);
        if let Some(user_id) = self.user_id {
            item.user_id = ActiveValue::Set(Some(user_id));
        }
    }
}

/// Markdown -> HTML (`CommonMark` + tables, strikethrough, footnotes).
///
/// Raw HTML inside the markdown is **escaped**, not passed through: the
/// output is rendered with `| safe` in templates, so passing `<script>`
/// through would be an XSS hole. (Same idea as Laravel's
/// `Str::markdown($s, ['html_input' => 'escape'])`.)
#[must_use]
pub fn render_markdown(markdown: &str) -> String {
    use pulldown_cmark::{html, Event, Options, Parser};

    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_SMART_PUNCTUATION;
    let parser = Parser::new_ext(markdown, opts).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, parser);
    out
}

/// Plain-text start of the markdown, cut at a word boundary.
///
/// Only paragraph/heading text counts: tables, code blocks and lists are
/// skipped so the excerpt never shows `|---|` noise.
#[must_use]
pub fn excerpt(markdown: &str, max_chars: usize) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    let mut text = String::new();
    let mut in_text_block = false;
    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Paragraph | Tag::Heading { .. }) => in_text_block = true,
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => {
                in_text_block = false;
                if text.chars().count() >= max_chars / 2 {
                    break;
                }
                text.push(' ');
            }
            Event::Start(Tag::Table(_) | Tag::CodeBlock(_) | Tag::List(_)) => {
                in_text_block = false;
            }
            Event::Text(t) | Event::Code(t) if in_text_block => text.push_str(&t),
            Event::SoftBreak | Event::HardBreak if in_text_block => text.push(' '),
            _ => {}
        }
        if text.chars().count() > max_chars {
            break;
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max_chars {
        return text;
    }
    let cut: String = text.chars().take(max_chars).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches(|c: char| !c.is_alphanumeric()))
}

/// ~200 words per minute, at least 1.
#[must_use]
pub fn reading_minutes_for(markdown: &str) -> i64 {
    let words = i64::try_from(markdown.split_whitespace().count()).unwrap_or(i64::MAX);
    ((words + 199) / 200).max(1)
}

impl Model {
    /// Cached HTML when the worker has rendered it, otherwise render now.
    #[must_use]
    pub fn body_html(&self) -> String {
        self.body_html
            .clone()
            .unwrap_or_else(|| render_markdown(&self.body))
    }

    /// Recompute the derived columns (`body_html` cache, `reading_minutes`).
    /// Called by the `process_post` worker after every save.
    ///
    /// # Errors
    /// DB errors.
    pub async fn refresh_derived(self, db: &DatabaseConnection) -> ModelResult<Self> {
        let html = render_markdown(&self.body);
        let minutes = reading_minutes_for(&self.body);
        let mut item = self.into_active_model();
        item.body_html = ActiveValue::Set(Some(html));
        item.reading_minutes = ActiveValue::Set(Some(minutes));
        Ok(item.update(db).await?)
    }

    /// Atomically mark the post as "subscribers notified".
    ///
    /// `UPDATE posts SET notified_at = now WHERE id = ? AND notified_at IS NULL`
    /// — only one caller can win, so two workers (or a worker and the
    /// scheduler) can never email subscribers twice. At-most-once delivery.
    ///
    /// # Errors
    /// DB errors.
    pub async fn claim_notification(db: &DatabaseConnection, id: i64) -> ModelResult<bool> {
        // same type as the column (DateTime<FixedOffset>) so it round-trips
        let now: DateTimeWithTimeZone = Utc::now().into();
        let res = Entity::update_many()
            .col_expr(Column::NotifiedAt, Expr::value(now))
            .filter(Column::Id.eq(id))
            .filter(Column::NotifiedAt.is_null())
            .filter(Self::published_condition())
            .exec(db)
            .await?;
        Ok(res.rows_affected == 1)
    }

    #[must_use]
    pub fn excerpt(&self) -> String {
        excerpt(&self.body, EXCERPT_LEN)
    }

    /// One page of published posts, newest first. `page_size` is fixed
    /// server-side: `PaginationQuery` would happily accept `?page_size=1000000`.
    ///
    /// Returns `loco_rs::Result` (not `ModelResult`) because `fetch_page` does.
    ///
    /// # Errors
    /// DB errors.
    pub async fn published_page(
        db: &DatabaseConnection,
        page: u64,
    ) -> Result<query::PageResponse<Self>> {
        let pagination = query::PaginationQuery {
            page: page.max(1),
            page_size: PER_PAGE,
        };
        let select = Entity::find()
            .filter(Self::published_condition())
            .order_by_desc(Column::PublishedAt)
            .order_by_desc(Column::Id);
        query::fetch_page(db, select, &pagination).await
    }

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

    /// Validate, then insert the post and its tags in one transaction.
    /// Slug is derived in `before_save` when empty.
    ///
    /// # Errors
    /// `ModelError::Validation` for bad input, DB errors otherwise.
    pub async fn create(db: &DatabaseConnection, params: &PostParams) -> ModelResult<Self> {
        params.validate_for_save()?;
        ensure_slug_free(db, &params.slug, None).await?;
        let txn = db.begin().await?;
        let mut item = ActiveModel {
            ..Default::default()
        };
        params.apply(&mut item);
        let post = item.insert(&txn).await?;
        post.sync_tags(&txn, &super::tags::parse_list(&params.tags))
            .await?;
        txn.commit().await?;
        Ok(post)
    }

    /// Replace this post's tags with `names` (creating missing tags).
    ///
    /// # Errors
    /// DB errors.
    pub async fn sync_tags(
        &self,
        txn: &DatabaseTransaction,
        names: &[String],
    ) -> ModelResult<Vec<tags::Model>> {
        post_tags::Entity::delete_many()
            .filter(post_tags::Column::PostId.eq(self.id))
            .exec(txn)
            .await?;
        let mut attached = Vec::with_capacity(names.len());
        for name in names {
            attached.push(super::tags::Model::find_or_create(txn, name).await?);
        }
        if !attached.is_empty() {
            post_tags::Entity::insert_many(attached.iter().map(|tag| post_tags::ActiveModel {
                post_id: ActiveValue::Set(self.id),
                tag_id: ActiveValue::Set(tag.id),
            }))
            .exec(txn)
            .await?;
        }
        Ok(attached)
    }

    /// This post's tags (one query, through the `post_tags` join table).
    ///
    /// # Errors
    /// DB errors.
    pub async fn tags(&self, db: &DatabaseConnection) -> ModelResult<Vec<tags::Model>> {
        Ok(self
            .find_related(tags::Entity)
            .order_by_asc(tags::Column::Name)
            .all(db)
            .await?)
    }

    /// Public URL of the cover image, if any.
    #[must_use]
    pub fn cover_url(&self) -> Option<String> {
        self.cover_image
            .as_ref()
            .map(|key| format!("/static/{key}"))
    }

    /// Validate + store a new cover, then point the post at it. The previous
    /// file (if any) is deleted after the row is updated.
    ///
    /// Takes `&Storage`, not `&AppContext`: the model needs exactly the
    /// storage and the DB, nothing else from the context.
    ///
    /// # Errors
    /// `ModelError::Validation` (field `cover`) for empty/too big/not an
    /// image; storage and DB errors otherwise.
    pub async fn attach_cover(
        self,
        db: &DatabaseConnection,
        storage: &loco_rs::storage::Storage,
        bytes: &axum::body::Bytes,
    ) -> Result<Self> {
        if bytes.is_empty() {
            return Err(field_error("cover", "required", "Choose an image to upload.").into());
        }
        if bytes.len() > MAX_COVER_BYTES {
            return Err(field_error("cover", "too_big", "Images must be 4 MB or smaller.").into());
        }
        let Some(ext) = sniff_image(bytes) else {
            return Err(field_error("cover", "type", "Only PNG, JPEG, GIF or WebP images.").into());
        };
        let key = format!("{COVER_DIR}/{}-{}.{ext}", self.id, Uuid::new_v4());
        storage.upload(std::path::Path::new(&key), bytes).await?;

        let old = self.cover_image.clone();
        let mut item = self.into_active_model();
        item.cover_image = ActiveValue::Set(Some(key));
        let post = item.update(db).await?;

        if let Some(old) = old {
            if let Err(err) = storage.delete(std::path::Path::new(&old)).await {
                tracing::warn!(key = old, error = %err, "could not delete old cover");
            }
        }
        Ok(post)
    }

    /// # Errors
    /// DB errors (a missing file is only logged).
    pub async fn remove_cover(
        self,
        db: &DatabaseConnection,
        storage: &loco_rs::storage::Storage,
    ) -> Result<Self> {
        let Some(key) = self.cover_image.clone() else {
            return Ok(self);
        };
        let mut item = self.into_active_model();
        item.cover_image = ActiveValue::Set(None);
        let post = item.update(db).await?;
        if let Err(err) = storage.delete(std::path::Path::new(&key)).await {
            tracing::warn!(key, error = %err, "could not delete cover");
        }
        Ok(post)
    }

    /// Live posts whose subscribers haven't been notified yet, published
    /// after `since` (so turning the feature on doesn't announce the whole
    /// archive).
    ///
    /// # Errors
    /// DB errors.
    pub async fn due_for_announcement(
        db: &DatabaseConnection,
        since: DateTimeWithTimeZone,
    ) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .filter(Self::published_condition())
            .filter(Column::PublishedAt.gt(since))
            .filter(Column::NotifiedAt.is_null())
            .order_by_asc(Column::PublishedAt)
            .all(db)
            .await?)
    }

    /// Every storage key currently referenced by a post.
    ///
    /// # Errors
    /// DB errors.
    pub async fn cover_keys(
        db: &DatabaseConnection,
    ) -> ModelResult<std::collections::HashSet<String>> {
        Ok(Entity::find()
            .filter(Column::CoverImage.is_not_null())
            .all(db)
            .await?
            .into_iter()
            .filter_map(|p| p.cover_image)
            .collect())
    }

    /// A post **owned by** `user_id`. Someone else's post is "not found"
    /// (404), not "forbidden": we don't confirm that the id exists.
    /// (Laravel: a Policy, or `$user->posts()->findOrFail($id)`.)
    ///
    /// # Errors
    /// `ModelError::EntityNotFound`
    pub async fn find_owned(db: &DatabaseConnection, id: i64, user_id: i64) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .filter(Column::UserId.eq(user_id))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// Tags as the comma-separated string the admin form edits.
    ///
    /// # Errors
    /// DB errors.
    pub async fn tag_list(&self, db: &DatabaseConnection) -> ModelResult<String> {
        Ok(self
            .tags(db)
            .await?
            .into_iter()
            .map(|t| t.name)
            .collect::<Vec<_>>()
            .join(", "))
    }

    /// All posts by one author, newest first (drafts included).
    ///
    /// # Errors
    /// DB errors.
    pub async fn by_author(db: &DatabaseConnection, user_id: i64) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .order_by_desc(Column::CreatedAt)
            .order_by_desc(Column::Id)
            .all(db)
            .await?)
    }

    /// The author, if any.
    ///
    /// # Errors
    /// DB errors.
    pub async fn author(&self, db: &DatabaseConnection) -> ModelResult<Option<users::Model>> {
        Ok(self.find_related(users::Entity).one(db).await?)
    }

    /// Eager-load authors for many posts (one `WHERE id IN (..)` query).
    ///
    /// # Errors
    /// DB errors.
    pub async fn load_authors(
        db: &DatabaseConnection,
        posts: &[Self],
    ) -> ModelResult<Vec<Option<users::Model>>> {
        Ok(posts.load_one(users::Entity, db).await?)
    }

    /// Eager-load tags for many posts: **two queries total**, not N+1.
    /// Result is index-aligned with `posts`.
    ///
    /// # Errors
    /// DB errors.
    pub async fn load_tags(
        db: &DatabaseConnection,
        posts: &[Self],
    ) -> ModelResult<Vec<Vec<tags::Model>>> {
        let mut all = posts.load_many(tags::Entity, db).await?;
        for list in &mut all {
            list.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Ok(all)
    }

    /// One page of published posts carrying `tag`.
    ///
    /// # Errors
    /// DB errors.
    pub async fn published_page_for_tag(
        db: &DatabaseConnection,
        tag: &tags::Model,
        page: u64,
    ) -> Result<query::PageResponse<Self>> {
        let pagination = query::PaginationQuery {
            page: page.max(1),
            page_size: PER_PAGE,
        };
        let select = tag
            .find_related(Entity)
            .filter(Self::published_condition())
            .order_by_desc(Column::PublishedAt)
            .order_by_desc(Column::Id);
        query::fetch_page(db, select, &pagination).await
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
        let id = match &self.id {
            ActiveValue::Set(id) | ActiveValue::Unchanged(id) => Some(*id),
            ActiveValue::NotSet => None,
        };
        ensure_slug_free(db, &params.slug, id).await?;
        params.apply(&mut self);
        let txn = db.begin().await?;
        let post = self.update(&txn).await?;
        post.sync_tags(&txn, &super::tags::parse_list(&params.tags))
            .await?;
        txn.commit().await?;
        Ok(post)
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
