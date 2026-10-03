//! HTML cleaning for received mail. The result is still only ever shown inside a sandboxed
//! iframe with a restrictive CSP; this is the first of two layers.

use lol_html::{RewriteStrSettings, element, rewrite_str};

pub fn clean_html(html: &str) -> String {
    let mut builder = ammonia::Builder::default();
    builder
        .add_generic_attributes([
            "style",
            "class",
            "align",
            "valign",
            "bgcolor",
            "width",
            "height",
            "border",
            "cellpadding",
            "cellspacing",
            "dir",
            "color",
        ])
        .rm_clean_content_tags(["style"])
        .add_tags(["center", "font", "style"])
        .add_tag_attributes("font", ["face", "size", "color"])
        .add_url_schemes(["cid"])
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank");
    let cleaned = builder.clean(html).to_string();
    strip_trackers(&cleaned).unwrap_or(cleaned)
}

/// Removes images that cannot be meant to be seen: 0 or 1 pixel in either dimension, or hidden.
fn strip_trackers(html: &str) -> Option<String> {
    let tiny = |v: Option<String>| matches!(v.as_deref().map(str::trim), Some("0" | "1" | "0px" | "1px"));
    rewrite_str(
        html,
        RewriteStrSettings::new().append_element_content_handler(element!("img", |el| {
            let style = el
                .get_attribute("style")
                .unwrap_or_default()
                .to_lowercase()
                .replace(' ', "");
            let hidden = style.contains("display:none") || style.contains("visibility:hidden");
            if tiny(el.get_attribute("width")) || tiny(el.get_attribute("height")) || hidden {
                el.remove();
            }
            Ok(())
        })),
    )
    .ok()
}

/// Plain-text alternative for outgoing HTML mail.
pub fn html_to_text(html: &str) -> String {
    let mut with_breaks = html.to_string();
    for tag in [
        "<br>",
        "<br/>",
        "<br />",
        "</p>",
        "</div>",
        "</li>",
        "</blockquote>",
        "</h1>",
        "</h2>",
        "</h3>",
    ] {
        with_breaks = with_breaks.replace(tag, &format!("{tag}\n"));
    }
    let text = ammonia::Builder::empty().clean(&with_breaks).to_string();
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut out = String::new();
    let mut blank = 0;
    for line in lines {
        blank = if line.is_empty() { blank + 1 } else { 0 };
        if blank < 2 {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_scripts_and_handlers() {
        let out = clean_html(r#"<p onclick="evil()">hi</p><script>alert(1)</script><a href="javascript:x()">l</a>"#);
        assert!(!out.contains("script"));
        assert!(!out.contains("onclick"));
        assert!(!out.contains("javascript:"));
        assert!(out.contains("<p>hi</p>"));
    }

    #[test]
    fn keeps_layout_and_cid_images() {
        let out = clean_html(
            r#"<table width="600" style="color:red"><tr><td><img src="cid:logo" width="80"></td></tr></table>"#,
        );
        assert!(out.contains(r#"width="600""#));
        assert!(out.contains("color:red"));
        assert!(out.contains(r#"src="cid:logo""#));
    }

    #[test]
    fn strips_tracking_pixels() {
        let out = clean_html(
            r#"<img src="https://t.example/open.gif" width="1" height="1"><img src="https://x.example/a.png" width="200">"#,
        );
        assert!(!out.contains("open.gif"));
        assert!(out.contains("a.png"));
    }

    #[test]
    fn links_open_in_new_tab() {
        let out = clean_html(r#"<a href="https://example.com">x</a>"#);
        assert!(out.contains(r#"target="_blank""#));
        assert!(out.contains("noopener"));
    }

    #[test]
    fn text_alternative() {
        assert_eq!(
            html_to_text("<p>Hello &amp; welcome</p><p>Bye<br>now</p>"),
            "Hello & welcome\nBye\nnow"
        );
    }
}
