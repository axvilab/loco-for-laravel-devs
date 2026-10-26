//! Typed view of the free-form `settings:` block in `config/<env>.yaml`.
use loco_rs::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// Public base URL, used for absolute links (RSS, sitemap, emails).
    pub base_url: String,
    /// Mark the auth cookie `Secure` (HTTPS only). `true` in production.
    #[serde(default)]
    pub cookie_secure: bool,
    /// `From:` for outgoing mail. Loco's default is
    /// `System <system@example.com>` (`mailer::DEFAULT_FROM_SENDER`).
    #[serde(default = "default_mail_from")]
    pub mail_from: String,
}

fn default_mail_from() -> String {
    "Loco Blog <noreply@localhost>".to_string()
}

impl Settings {
    /// # Errors
    /// When `settings:` is missing or doesn't match this struct.
    pub fn from_context(ctx: &AppContext) -> Result<Self> {
        let raw = ctx
            .config
            .settings
            .clone()
            .ok_or_else(|| Error::string("missing `settings:` in config"))?;
        Ok(serde_json::from_value(raw)?)
    }
}
