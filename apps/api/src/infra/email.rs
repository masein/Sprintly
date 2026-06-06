//! Outbound transactional email.
//!
//! Two backends: a real SMTP sender (lettre) when `SPRINTLY_SMTP_URL` is set,
//! and a log-only sender otherwise (the dev default). Sends are best-effort —
//! callers use [`spawn_send`] so a slow or down mail server never blocks an
//! HTTP response, and failures are logged rather than surfaced to the user.

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use lettre::{
    message::Mailbox, AsyncSmtpTransport, AsyncTransport, Message as LettreMessage, Tokio1Executor,
};
use tracing::{error, info, warn};

use crate::config::EmailConfig;

/// A plain-text message to send.
#[derive(Debug, Clone)]
pub struct Message {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, msg: Message) -> Result<()>;
}

/// Build the configured mailer. Falls back to log-only if SMTP isn't set or the
/// URL / `From` can't be parsed (logged, never panics).
pub fn build(cfg: &EmailConfig) -> Arc<dyn Mailer> {
    let from: Mailbox = match cfg.mail_from.parse() {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, from = %cfg.mail_from, "invalid SPRINTLY_MAIL_FROM; using log-only mail");
            return Arc::new(LogMailer);
        }
    };
    match &cfg.smtp_url {
        Some(url) => match AsyncSmtpTransport::<Tokio1Executor>::from_url(url) {
            Ok(builder) => {
                info!(from = %from, "email: SMTP transport configured");
                Arc::new(SmtpMailer {
                    transport: builder.build(),
                    from,
                })
            }
            Err(e) => {
                warn!(error = %e, "invalid SPRINTLY_SMTP_URL; using log-only mail");
                Arc::new(LogMailer)
            }
        },
        None => {
            info!("email: no SPRINTLY_SMTP_URL set; using log-only mailer");
            Arc::new(LogMailer)
        }
    }
}

/// Fire-and-forget send: spawns the send so the calling HTTP handler never
/// blocks on the mail server. Failures are logged, not returned.
pub fn spawn_send(mailer: Arc<dyn Mailer>, msg: Message) {
    tokio::spawn(async move {
        let (to, subject) = (msg.to.clone(), msg.subject.clone());
        if let Err(e) = mailer.send(msg).await {
            error!(error = %e, %to, %subject, "email send failed");
        }
    });
}

/// Log-only mailer for dev / when SMTP isn't configured.
pub struct LogMailer;

#[async_trait]
impl Mailer for LogMailer {
    async fn send(&self, msg: Message) -> Result<()> {
        info!(
            to = %msg.to,
            subject = %msg.subject,
            body = %msg.body,
            "email (log-only; SMTP not configured)"
        );
        Ok(())
    }
}

/// SMTP mailer backed by lettre.
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, msg: Message) -> Result<()> {
        let to: Mailbox = msg
            .to
            .parse()
            .with_context(|| format!("invalid recipient address: {}", msg.to))?;
        let email = LettreMessage::builder()
            .from(self.from.clone())
            .to(to)
            .subject(msg.subject)
            .body(msg.body)
            .context("building email message")?;
        self.transport
            .send(email)
            .await
            .context("sending email via SMTP")?;
        Ok(())
    }
}

// ─── templates ───────────────────────────────────────────────────────────────

/// Password-reset email. The link lands on the web reset page, which posts the
/// token to `/auth/password/reset/confirm`.
pub fn password_reset(public_url: &str, token: &str, to: &str) -> Message {
    let base = public_url.trim_end_matches('/');
    let link = format!("{base}/reset?token={token}");
    Message {
        to: to.to_string(),
        subject: "Reset your Sprintly password".into(),
        body: format!(
            "Someone (hopefully you) asked to reset your Sprintly password.\n\n\
             Open this link to set a new one — it expires in 30 minutes:\n\n  {link}\n\n\
             If it wasn't you, ignore this email; nothing changed.\n"
        ),
    }
}

/// Invite email. Links to the register page with the invite token prefilled.
pub fn invite(public_url: &str, token: &str, role: &str, to: &str) -> Message {
    let base = public_url.trim_end_matches('/');
    let link = format!("{base}/register?invite={token}");
    Message {
        to: to.to_string(),
        subject: "You've been invited to Sprintly".into(),
        body: format!(
            "You've been invited to Sprintly as a {role}.\n\n\
             Create your account here:\n\n  {link}\n\n\
             The invite expires; if the link stops working, ask whoever invited \
             you for a fresh one.\n"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Test mailer that records what it was asked to send.
    struct CaptureMailer {
        sent: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl Mailer for CaptureMailer {
        async fn send(&self, msg: Message) -> Result<()> {
            self.sent.lock().unwrap().push(msg);
            Ok(())
        }
    }

    #[tokio::test]
    async fn capture_mailer_records_payload() {
        let m = CaptureMailer {
            sent: Mutex::new(Vec::new()),
        };
        m.send(password_reset("https://x.test", "tok", "u@x.test"))
            .await
            .unwrap();
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "u@x.test");
        assert!(sent[0].body.contains("tok"));
    }

    #[tokio::test]
    async fn log_mailer_succeeds() {
        assert!(LogMailer
            .send(Message {
                to: "a@b.test".into(),
                subject: "s".into(),
                body: "b".into(),
            })
            .await
            .is_ok());
    }

    #[test]
    fn reset_template_has_token_link() {
        let msg = password_reset("https://sprintly.example/", "tok123", "u@x.test");
        assert_eq!(msg.to, "u@x.test");
        assert!(msg
            .body
            .contains("https://sprintly.example/reset?token=tok123"));
        assert!(msg.subject.to_lowercase().contains("reset"));
    }

    #[test]
    fn invite_template_has_link_and_role() {
        let msg = invite("https://sprintly.example", "inv9", "admin", "p@x.test");
        assert!(msg
            .body
            .contains("https://sprintly.example/register?invite=inv9"));
        assert!(msg.body.contains("admin"));
    }

    #[test]
    fn build_without_smtp_is_log_only() {
        let cfg = EmailConfig {
            smtp_url: None,
            mail_from: "Sprintly <noreply@localhost>".into(),
        };
        let _ = build(&cfg); // constructs without panicking
    }
}
