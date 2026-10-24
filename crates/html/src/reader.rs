// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Local advanced reader policy: preserve layout; resource access remains host-controlled.
use crate::{
    escape, is_safe_url,
    tokenizer::{Token, Tokenizer},
};
#[derive(Clone, Debug)]
pub struct ReaderHtml {
    pub html: String,
    pub images: Vec<String>,
    pub backgrounds: Vec<String>,
}
fn allowed(name: &str) -> bool {
    matches!(
        name,
        "html"
            | "head"
            | "body"
            | "style"
            | "div"
            | "span"
            | "p"
            | "a"
            | "img"
            | "br"
            | "hr"
            | "wbr"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "colgroup"
            | "col"
            | "caption"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "b"
            | "strong"
            | "i"
            | "em"
            | "u"
            | "s"
            | "small"
            | "sup"
            | "sub"
            | "font"
            | "blockquote"
            | "pre"
            | "code"
            | "center"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
    )
}
fn void(name: &str) -> bool {
    matches!(name, "img" | "br" | "hr" | "wbr" | "col")
}
fn css_ok(css: &str) -> bool {
    let mut depth = 0usize;
    for c in css.chars() {
        match c {
            '{' | '(' | '[' => {
                depth += 1;
                if depth > 32 {
                    return false;
                }
            }
            '}' | ')' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    true
}
fn background_urls(css: &str, images: &mut Vec<String>, backgrounds: &mut Vec<String>) {
    for declaration in css.split([';', '{', '}']) {
        let Some((name, value)) = declaration.split_once(':') else { continue };
        if !matches!(name.trim().to_ascii_lowercase().as_str(), "background" | "background-image") {
            continue;
        }
        let lower = value.to_ascii_lowercase();
        let mut at = 0;
        while let Some(found) = lower.get(at..).and_then(|v| v.find("url(")) {
            let start = at + found + 4;
            let Some(end) = value.get(start..).and_then(|s| s.find(')')).map(|n| start + n) else { break };
            let url = value.get(start..end).unwrap_or("").trim().trim_matches(['\'', '"']);
            if is_safe_url(url)
                && ["http:", "https:", "cid:"].iter().any(|scheme| url.get(..scheme.len()).is_some_and(|s| s.eq_ignore_ascii_case(scheme)))
                && images.len() < 64
            {
                if !images.iter().any(|s| s == url) {
                    images.push(url.into());
                }
                if !backgrounds.iter().any(|s| s == url) {
                    backgrounds.push(url.into());
                }
            }
            at = end + 1;
        }
    }
}
pub fn reader_html(input: &str) -> Option<ReaderHtml> {
    if input.len() > 512 * 1024 {
        return None;
    }
    let mut out = String::from("<!doctype html>");
    let mut images = Vec::new();
    let mut backgrounds = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut blocked: Vec<String> = Vec::new();
    let mut in_style = false;
    let mut css = 0usize;
    for (count, token) in Tokenizer::with_styles(input).enumerate() {
        if count >= 20_000 {
            return None;
        }
        match token {
            Token::Open(tag) => {
                let name = tag.name.as_str();
                if !blocked.is_empty() {
                    if !matches!(name, "input" | "embed" | "param" | "source" | "track" | "meta" | "link" | "base") && !void(name) {
                        if blocked.len() >= 128 {
                            return None;
                        }
                        blocked.push(tag.name);
                    }
                    continue;
                }
                if matches!(name, "object" | "template" | "svg" | "math" | "textarea" | "xmp") {
                    blocked.push(tag.name);
                    continue;
                }
                if !allowed(name) {
                    continue;
                }
                if !void(name) {
                    if stack.len() >= 128 {
                        return None;
                    }
                    stack.push(tag.name.clone());
                }
                in_style = name == "style";
                out.push('<');
                out.push_str(name);
                for (key, value) in tag.attributes() {
                    let key = key.to_ascii_lowercase();
                    let accept = match key.as_str() {
                        "href" => name == "a" && is_safe_url(&value),
                        "src" => {
                            name == "img"
                                && is_safe_url(&value)
                                && ["http:", "https:", "cid:"].iter().any(|s| value.get(..s.len()).is_some_and(|p| p.eq_ignore_ascii_case(s)))
                        }
                        "style" => {
                            background_urls(&value, &mut images, &mut backgrounds);
                            css = css.saturating_add(value.len());
                            if css > 128 * 1024 || !css_ok(&value) {
                                return None;
                            }
                            true
                        }
                        "id" | "class" | "title" | "dir" | "lang" | "align" | "valign" | "width" | "height" | "bgcolor" | "border"
                        | "cellpadding" | "cellspacing" | "colspan" | "rowspan" | "scope" | "nowrap" | "alt" | "face" | "color" | "size" => true,
                        _ => false,
                    };
                    if accept {
                        if key == "src" && !images.contains(&value) && images.len() < 64 {
                            images.push(value.clone());
                        }
                        out.push(' ');
                        out.push_str(&key);
                        out.push_str("=\"");
                        out.push_str(&escape(&value).replace('"', "&quot;"));
                        out.push('"');
                    }
                }
                out.push('>');
            }
            Token::Close(name) => {
                if !blocked.is_empty() {
                    if let Some(at) = blocked.iter().rposition(|n| *n == name) {
                        blocked.truncate(at);
                    }
                    continue;
                }
                if !allowed(&name) || void(&name) {
                    continue;
                }
                if let Some(at) = stack.iter().rposition(|n| *n == name) {
                    stack.truncate(at);
                }
                out.push_str("</");
                out.push_str(&name);
                out.push('>');
                if name == "style" {
                    in_style = false;
                }
            }
            Token::Text(text) if blocked.is_empty() => {
                if in_style {
                    css = css.saturating_add(text.len());
                    if css > 128 * 1024 || !css_ok(text) {
                        return None;
                    }
                    background_urls(text, &mut images, &mut backgrounds);
                    out.push_str(text);
                } else {
                    out.push_str(text);
                }
            }
            _ => {}
        }
    }
    Some(ReaderHtml { html: out, images, backgrounds })
}
