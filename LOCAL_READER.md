# Local 0.7.0 advanced reader

This is a personal variant of UCS-Care-Group MailCraft 0.7.0. It intentionally
extends the upstream presentation whitelist with a desktop-only Blitz reader.
Nothing has been published or proposed upstream. The source archive is preserved
separately in ucs-fork-20261024/mailcraft-main.

The renderer has no JavaScript or HTTP client. Sanitized HTML retains style sheets,
classes, media queries and table structure. Embedded documents and active content
are removed. The resource provider accepts only declared images; CID images and
consented remote images are supplied by the host's existing cache and transport.
Background images follow the same consent path. External stylesheets and fonts
are denied. Data-URL pictures and SVG are not currently supported.

Layout and CPU painting run on one dedicated worker, never on the UI thread. The
queue holds one pending request, the reply one frame per retained view. There are
two retained view states and two DOMs. Raster tiles are capped at four million
pixels; decoded pictures at 16 MiB per DOM. Documents over 512 KiB, 20,000 tokens,
128 element levels or 128 KiB CSS fall back to the native reader. A stalled worker
causes native fallback after three seconds; it is not forcibly killed, and no
additional worker is spawned in that app session.

Links are verified using the DOM hit test, including after scrolling and under
CSS overlays. Text drag-selection and copying use Blitz's text selection. Select
All and Find use the native reader. The context menu offers Simplified layout;
the native view offers Show original formatting. MAILCRAFT_NATIVE_READER=1 forces
the native reader for troubleshooting and native-reader regression tests.

The local build uses crates.io winit instead of the unavailable GitHub-patched
revision. The upstream activation-token fix for bringing a browser forward under
KWin is therefore not included. Rust 1.95 or later is required. Development builds
have build id "dev"; upstream update installation is disabled by its existing
development-build check. Keep the unmodified 0.7.0 installation for rollback.

MPL exceptions are package-and-version-specific, approved by the owner in the
previous reader work. See THIRD_PARTY_BLITZ.md and licenses/MPL-2.0.txt. Fonts are
bundled OFL Liberation faces with their licence and ATTRIBUTION.md entries.

Transactional-mail compatibility: CSS budget 128 KiB and nesting depth 128, within the unchanged 512 KiB total HTML and 20,000 token caps. Offline reproduction confirmed that a real 124 KiB transactional message had previously fallen back to native rendering. Regression fixture contains synthetic content only.

Progress rail compatibility: local MIT/Apache Blitz DOM copy in vendor/blitz-dom preserves relative percentage tracks in automatic tables rather than discarding their widths. Reader UA rules honor zero cellpadding without overriding author CSS. This improves nested presentation tables; it remains an approximation of browser table layout. Regression checks intrinsic marker width and uninterrupted rail pixels on synthetic content.

Redistribution cleanup: vendored Blitz keeps its original copyright notices. Its default stylesheet remains MPL-2.0 (licence in vendor/blitz-dom/LICENSE-MPL-2.0.txt). The embedded Mozilla bullet font and its CSS data URL are removed; bullet fallback uses the existing OFL Liberation Sans font. SPDX checking accepts named third-party copyright holders while retaining the exact MIT OR Apache-2.0 requirement for Rust files.
