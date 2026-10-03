//! Building and sending outgoing mail.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Attachment, Mailbox, MultiPart, SinglePart, header::ContentType},
    transport::smtp::authentication::Credentials,
};

use crate::{crypto, mail::sanitize, models::Account};

pub struct SmtpParams {
    pub host: String,
    pub port: u16,
    pub security: String,
    pub username: String,
    pub password: String,
}

impl SmtpParams {
    pub fn for_account(account: &Account, master_key: &[u8; 32]) -> Result<Self> {
        Ok(Self {
            host: account.smtp_host.clone(),
            port: account.smtp_port as u16,
            security: account.smtp_security.clone(),
            username: account.smtp_username.clone(),
            password: crypto::decrypt(master_key, &account.password_enc)?,
        })
    }

    fn transport(&self) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
        let builder = match self.security.as_str() {
            "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&self.host)?,
            "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.host)?,
            "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.host),
            other => bail!("unknown SMTP security mode {other}"),
        };
        Ok(builder
            .port(self.port)
            .credentials(Credentials::new(self.username.clone(), self.password.clone()))
            .timeout(Some(Duration::from_secs(30)))
            .build())
    }

    pub async fn test(&self) -> Result<()> {
        if self
            .transport()?
            .test_connection()
            .await
            .context("SMTP connection failed")?
        {
            Ok(())
        } else {
            Err(anyhow!("SMTP server did not accept the connection"))
        }
    }

    pub async fn send(&self, message: Message) -> Result<()> {
        self.transport()?.send(message).await.context("SMTP send failed")?;
        Ok(())
    }
}

/// Parses "a@b, Name <c@d>" into mailboxes.
pub fn parse_recipients(list: &str) -> Result<Vec<Mailbox>> {
    list.split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<Mailbox>().map_err(|_| anyhow!("invalid address: {s}")))
        .collect()
}

pub struct OutgoingAttachment {
    pub filename: String,
    pub mime: String,
    pub data: Vec<u8>,
}

pub struct Outgoing {
    pub from: Mailbox,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub bcc: Vec<Mailbox>,
    pub subject: String,
    pub html: String,
    /// Message-IDs without angle brackets.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<OutgoingAttachment>,
}

pub fn build(out: Outgoing) -> Result<Message> {
    let domain = out.from.email.domain().to_string();
    let mut builder = Message::builder()
        .from(out.from)
        .subject(out.subject)
        .message_id(Some(format!("<{}@{}>", crypto::random_token(), domain)));
    for mailbox in out.to {
        builder = builder.to(mailbox);
    }
    for mailbox in out.cc {
        builder = builder.cc(mailbox);
    }
    for mailbox in out.bcc {
        builder = builder.bcc(mailbox);
    }
    if let Some(parent) = &out.in_reply_to {
        builder = builder.in_reply_to(format!("<{parent}>"));
    }
    if !out.references.is_empty() {
        let refs: Vec<String> = out.references.iter().map(|r| format!("<{r}>")).collect();
        builder = builder.references(refs.join(" "));
    }

    let body = MultiPart::alternative_plain_html(sanitize::html_to_text(&out.html), out.html);
    let message = if out.attachments.is_empty() {
        builder.multipart(body)?
    } else {
        let mut mixed = MultiPart::mixed().multipart(body);
        for att in out.attachments {
            let content_type = ContentType::parse(&att.mime)
                .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("valid mime"));
            let part: SinglePart = Attachment::new(att.filename).body(att.data, content_type);
            mixed = mixed.singlepart(part);
        }
        builder.multipart(mixed)?
    };
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::parse;

    #[test]
    fn recipients() {
        let list = parse_recipients("a@example.com, Bob Smith <bob@example.org>; ").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].email.to_string(), "bob@example.org");
        assert!(parse_recipients("not an address").is_err());
    }

    #[test]
    fn reply_headers_and_attachment_survive_round_trip() {
        let message = build(Outgoing {
            from: "Me <me@example.org>".parse().unwrap(),
            to: parse_recipients("jane@example.com").unwrap(),
            cc: vec![],
            bcc: parse_recipients("hidden@example.com").unwrap(),
            subject: "Re: Hello".into(),
            html: "<p>Thanks</p>".into(),
            in_reply_to: Some("abc@example.com".into()),
            references: vec!["root@example.org".into(), "abc@example.com".into()],
            attachments: vec![OutgoingAttachment {
                filename: "note.txt".into(),
                mime: "text/plain".into(),
                data: b"hi".to_vec(),
            }],
        })
        .unwrap();
        let raw = message.formatted();
        let parsed = parse::parse(&raw).unwrap();
        assert_eq!(parsed.in_reply_to, "abc@example.com");
        assert_eq!(parsed.refs, vec!["root@example.org", "abc@example.com"]);
        assert_eq!(parsed.subject, "Re: Hello");
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].filename, "note.txt");
        assert!(parsed.body_text.contains("Thanks"));
        assert!(!parsed.message_id.is_empty());
        assert!(!String::from_utf8_lossy(&raw).contains("hidden@example.com"));
    }
}
