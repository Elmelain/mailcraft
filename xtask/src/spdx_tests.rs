// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

#[test]
fn finds_rust_files_without_the_lines() {
    let files = vec!["a.rs".to_string(), "b.rs".into(), "c.md".into(), "gone.rs".into(), "crlf.rs".into()];
    let read = |f: &str| match f {
        "a.rs" => Some(format!("{HEADER}fn a() {{}}\n")),
        "b.rs" => Some("//! b\n".into()),
        "crlf.rs" => Some(HEADER.replace('\n', "\r\n")),
        "c.md" => Some("# c\n".into()),
        _ => None,
    };
    assert_eq!(missing(&files, read), vec!["b.rs"]);
}

#[test]
fn accepts_third_party_copyright_without_relaxing_licence() {
    assert!(has_header("// SPDX-FileCopyrightText: Blitz contributors\n// SPDX-License-Identifier: MIT OR Apache-2.0\n"));
    assert!(!has_header("// SPDX-FileCopyrightText: \n// SPDX-License-Identifier: MIT OR Apache-2.0\n"));
    assert!(!has_header("// SPDX-FileCopyrightText: Blitz contributors\n// SPDX-License-Identifier: GPL-3.0-only\n"));
}
