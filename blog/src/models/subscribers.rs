use chrono::Utc;
use loco_rs::prelude::*;
use serde::Deserialize;

pub use super::_entities::subscribers::{self, ActiveModel, Column, Entity, Model};
pub type Subscribers = Entity;

#[derive(Debug, Validate, Deserialize)]
pub struct Validator {
    #[validate(email(message = "Enter a valid email address."))]
    pub email: String,
}

impl Validatable for ActiveModel {
    fn validator(&self) -> Box<dyn Validate> {
        let email = match &self.email {
            ActiveValue::Set(e) | ActiveValue::Unchanged(e) => e.clone(),
            ActiveValue::NotSet => String::new(),
        };
        Box::new(Validator { email })
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
        if insert {
            // unguessable unsubscribe token, like `users.pid`
            this.token = ActiveValue::Set(Uuid::new_v4());
        } else if this.updated_at.is_unchanged() {
            this.updated_at = ActiveValue::Set(Utc::now().into());
        }
        Ok(this)
    }
}

impl Model {
    /// Idempotent: subscribing twice returns the existing row (and the
    /// response doesn't reveal whether the address was already known).
    ///
    /// # Errors
    /// `ModelError::Validation` for a bad address.
    pub async fn subscribe(db: &DatabaseConnection, email: &str) -> ModelResult<Self> {
        let email = email.trim().to_lowercase();
        ValidatorTrait::validate(&Validator {
            email: email.clone(),
        })
        .map_err(ModelError::Validation)?;

        if let Some(existing) = Entity::find()
            .filter(Column::Email.eq(&email))
            .one(db)
            .await?
        {
            return Ok(existing);
        }
        Ok(ActiveModel {
            email: ActiveValue::Set(email),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// # Errors
    /// `ModelError::EntityNotFound` for an unknown token.
    pub async fn unsubscribe(db: &DatabaseConnection, token: Uuid) -> ModelResult<Self> {
        let sub = Entity::find()
            .filter(Column::Token.eq(token))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        sub.clone().delete(db).await?;
        Ok(sub)
    }

    /// # Errors
    /// DB errors.
    pub async fn all(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
        Ok(Entity::find().order_by_asc(Column::Id).all(db).await?)
    }
}
