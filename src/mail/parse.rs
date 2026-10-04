//! Turns raw RFC 5322 bytes into the fields Portier stores.

use mail_parser::{Address, MessageParser, MimeHeaders};

use crate::{mail::sanitize, models::Addr};

#[derive(Debug, Default)]
pub struct ParsedAttachment {
    pub filename: String,
    pub mime: String,
    pub size: i64,
    pub content_id: Option<String>,
    pub inline: bool,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub message_id: String,
    pub in_reply_to: String,
    pub refs: Vec<String>,
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub date: Option<i64>,
    pub snippet: String,
    pub body_text: String,
    pub body_html: String,
    pub attachments: Vec<ParsedAttachment>,
}

fn addrs(address: Option<&Address>) -> Vec<Addr> {
    address
        .map(|a| {
            a.iter()
                .filter_map(|addr| {
                    Some(Addr {
                        name: addr.name().unwrap_or_default().to_string(),
                        address: addr.address()?.to_lowercase(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn snippet(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(200).collect()
}

pub fn parse(raw: &[u8]) -> Option<Parsed> {
    let msg = MessageParser::default().parse(raw)?;

    let id_list = |value: &mail_parser::HeaderValue| -> Vec<String> {
        match value.as_text_list() {
            Some(list) => list.iter().map(|s| s.to_string()).collect(),
            None => value.as_text().map(|s| vec![s.to_string()]).unwrap_or_default(),
        }
    };

    let body_text = msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default();
    let body_html = msg.body_html(0).map(|h| sanitize::clean_html(&h)).unwrap_or_default();

    let attachments = msg
        .attachments()
        .map(|part| {
            let mime = part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(sub) => format!("{}/{}", ct.ctype(), sub),
                    None => ct.ctype().to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".into());
            ParsedAttachment {
                filename: part.attachment_name().unwrap_or("attachment").to_string(),
                mime,
                size: part.contents().len() as i64,
                content_id: part.content_id().map(|c| c.trim_matches(['<', '>']).to_string()),
                inline: part.content_disposition().is_some_and(|d| d.is_inline()),
            }
        })
        .collect();

    Some(Parsed {
        message_id: msg.message_id().unwrap_or_default().to_string(),
        in_reply_to: id_list(msg.in_reply_to()).into_iter().next().unwrap_or_default(),
        refs: id_list(msg.references()),
        from: addrs(msg.from()).into_iter().next().unwrap_or_default(),
        to: addrs(msg.to()),
        cc: addrs(msg.cc()),
        subject: msg.subject().unwrap_or_default().to_string(),
        date: msg.date().map(|d| d.to_timestamp()),
        snippet: snippet(&body_text),
        body_text,
        body_html,
        attachments,
    })
}

/// Returns (filename, mime, bytes) of the attachment at `idx`.
pub fn attachment(raw: &[u8], idx: u32) -> Option<(String, String, Vec<u8>)> {
    let msg = MessageParser::default().parse(raw)?;
    let part = msg.attachment(idx)?;
    let mime = part
        .content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".into());
    Some((
        part.attachment_name().unwrap_or("attachment").to_string(),
        mime,
        part.contents().to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"From: Jane Doe <Jane@Example.com>\r\n\
To: me@example.org, Bob <bob@example.org>\r\n\
Subject: Hello there\r\n\
Message-ID: <abc@example.com>\r\n\
In-Reply-To: <parent@example.org>\r\n\
References: <root@example.org> <parent@example.org>\r\n\
Date: Sat, 03 Oct 2026 10:00:00 +0000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\n\
Line one.\r\n   Line   two.\r\n";

    #[test]
    fn parses_headers_and_body() {
        let p = parse(SAMPLE).unwrap();
        assert_eq!(
            p.from,
            Addr {
                name: "Jane Doe".into(),
                address: "jane@example.com".into()
            }
        );
        assert_eq!(p.to.len(), 2);
        assert_eq!(p.to[1].address, "bob@example.org");
        assert_eq!(p.subject, "Hello there");
        assert_eq!(p.message_id, "abc@example.com");
        assert_eq!(p.in_reply_to, "parent@example.org");
        assert_eq!(p.refs, vec!["root@example.org", "parent@example.org"]);
        assert_eq!(p.snippet, "Line one. Line two.");
        assert!(p.date.is_some());
        assert!(p.attachments.is_empty());
    }
}
