// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
use super::reader_html;
#[test]
fn reader_preserves_styles_and_table_sections() {
    let doc =
        reader_html("<style>@media(max-width:600px){.card{width:100%}}</style><table><tbody><tr><td class='card'>Hello</td></tr></tbody></table>")
            .unwrap();
    assert!(doc.html.contains("@media"));
    assert!(doc.html.contains("<tbody>"));
    assert!(doc.html.contains("class=\"card\""));
}
#[test]
fn reader_drops_active_elements_and_privileged_urls() {
    let doc=reader_html("<script>evil()</script><object><object>secret</object>hidden</object><img src='file:///secret'><a onclick='evil()' href='javascript:evil()'>Visible</a><img src='cid:one'>").unwrap();
    for bad in ["evil", "secret", "hidden", "onclick", "file:", "javascript:", "<object"] {
        assert!(!doc.html.contains(bad), "{bad}");
    }
    assert_eq!(doc.images, vec!["cid:one"]);
}
#[test]
fn oversized_documents_fail_to_native() {
    assert!(reader_html(&"a".repeat(512 * 1024 + 1)).is_none());
    assert!(reader_html(&"<div>".repeat(129)).is_none());
}

#[test]
fn only_background_picture_urls_become_extra_capabilities() {
    let doc=reader_html("<style>@import url(https://example.org/style.css);@font-face{src:url(https://example.org/font.woff)}.a{background-image:url('https://example.org/bg.png')}</style>").unwrap();
    assert_eq!(doc.backgrounds, vec!["https://example.org/bg.png"]);
    assert_eq!(doc.images, doc.backgrounds);
}

#[test]
fn reader_accepts_bounded_complex_transactional_mail() {
    let styles = ".card{color:#123456;background:#ffffff}".repeat(2000);
    let html = format!("<style>{styles}</style>{}<a href='https://example.org/item'>Item</a>{}", "<div>".repeat(80), "</div>".repeat(80));
    let doc = reader_html(&html).expect("complex transactional mail stays in advanced reader");
    assert!(doc.html.contains("https://example.org/item"));
    let too_much_css = format!("<style>{}</style>", "a{}".repeat(44000));
    assert!(reader_html(&too_much_css).is_none());
}
