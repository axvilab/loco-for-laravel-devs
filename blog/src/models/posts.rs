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

impl Model {
    #[must_use]
    pub fn body_html(&self) -> String {
        render_markdown(&self.body)
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
