// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;
fn job(html: &str) -> Job {
    let source = Arc::new(mailcraft_html::reader_html(html).unwrap());
    let key = Key {
        source: Arc::as_ptr(&source) as usize,
        view: 1,
        width: 800,
        height: 400,
        viewport_height: 400,
        y: 0,
        density: 1.0f32.to_bits(),
        dark: false,
        allowed: false,
        images: 0,
        action: 0,
    };
    Job { key, source, pictures: BTreeMap::new(), ctx: egui::Context::default(), output: Arc::new(Mutex::new(None)), action: None }
}
#[test]
fn advanced_engine_keeps_styles_columns_and_verified_links() {
    let mut job = job(
        "<style>body{margin:0}.card{width:600px;margin:0 auto}td{width:50%}</style><table class='card'><tr><td><a href='https://example.org/read'><span>Left column</span></a></td><td>Right column</td></tr></table>",
    );
    let fonts = fonts();
    let mut cache = Vec::new();
    let frame = render(&job, &fonts, &mut cache);
    assert!(!frame.data.failed);
    assert!(frame.pixels.is_some());
    assert!(!frame.data.links.is_empty());
    assert_eq!(frame.data.links[0].1.as_ref(), "https://example.org/read");
    assert!(frame.data.links[0].0.left() >= 99.0);
    job.action = Some((Action::Open, frame.data.links[0].0.center()));
    let frame = render(&job, &fonts, &mut cache);
    assert_eq!(frame.action.unwrap().1.as_ref(), "https://example.org/read");
}
#[test]
fn advanced_engine_scrolled_links_and_overlapping_content_hit_the_real_target() {
    let mut job = job(
        "<style>body{height:1200px}a{position:absolute;left:20px;top:650px;display:block;width:200px;height:60px}</style><a href='https://example.org/scrolled'>Read more</a>",
    );
    job.key.y = 600;
    job.action = Some((Action::Copy, pos2(30.0, 660.0)));
    let frame = render(&job, &fonts(), &mut Vec::new());
    assert_eq!(frame.action.unwrap().1.as_ref(), "https://example.org/scrolled");
    let mut hidden = job_for_overlay();
    hidden.action = Some((Action::Open, pos2(30.0, 30.0)));
    let frame = render(&hidden, &fonts(), &mut Vec::new());
    assert!(frame.action.is_none());
}
fn job_for_overlay() -> Job {
    job(
        "<a href='https://example.org/hidden' style='position:absolute;top:0;left:0;width:200px;height:100px'>Link</a><div style='position:absolute;z-index:10;top:0;left:0;width:300px;height:200px;background:white'>Cover</div>",
    )
}
fn colored_bytes(color: [u8; 4]) -> Arc<Vec<u8>> {
    let mut data = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(80, 60, image::Rgba(color)).write_to(&mut data, image::ImageFormat::Png).unwrap();
    Arc::new(data.into_inner())
}
#[test]
fn advanced_engine_remote_permission_revocation_and_refresh() {
    let mut job = job("<img src='https://example.org/picture.png' width='80' height='60'>");
    job.pictures.insert("https://example.org/picture.png".into(), colored_bytes([255, 0, 0, 255]));
    job.key.images = 1;
    job.key.allowed = true;
    let fonts = fonts();
    let mut cache = Vec::new();
    let frame = render(&job, &fonts, &mut cache);
    let color = frame.pixels.unwrap().pixels[20 * 800 + 20];
    assert!(color.r() > 200 && color.g() < 50);
    job.pictures.insert("https://example.org/picture.png".into(), colored_bytes([0, 0, 255, 255]));
    job.key.images = 2;
    let frame = render(&job, &fonts, &mut cache);
    let color = frame.pixels.unwrap().pixels[20 * 800 + 20];
    assert!(color.b() > 200 && color.r() < 50);
    job.key.allowed = false;
    let frame = render(&job, &fonts, &mut cache);
    let color = frame.pixels.unwrap().pixels[20 * 800 + 20];
    assert!(!(color.b() > 200 && color.r() < 50));
    assert!(cache.last().unwrap().loaded.is_empty());
    assert!(cache.len() <= 2);
    assert!(normal("file:///secret").is_none());
    assert!(normal("https://user:pass@example.org/p.png").is_none());
}
#[test]
fn advanced_engine_selection_is_copied_from_rendered_text() {
    let mut job = job("<p style='margin:20px'>A simple text selection for the reader</p>");
    job.action = Some((Action::Select(pos2(25.0, 30.0)), pos2(170.0, 30.0)));
    let frame = render(&job, &fonts(), &mut Vec::new());
    let text = frame.action.unwrap().1;
    assert!(!text.is_empty());
    assert!("A simple text selection for the reader".contains(text.as_ref()));
}
#[test]
fn advanced_engine_worker_returns_a_frame_without_ui_thread_layout() {
    let (tx, rx) = std::sync::mpsc::channel();
    let job = job("<p>Hello from the worker</p>");
    let output = job.output.clone();
    job.ctx.set_request_repaint_callback(move |_| {
        let _ = tx.send(());
    });
    let backend = Backend::start().unwrap();
    assert!(backend.submit(job));
    rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    let frame = output.lock().unwrap().take().unwrap();
    assert!(frame.pixels.is_some());
    assert!(!frame.data.failed);
}

#[test]
fn advanced_engine_synthetic_corpus_and_optional_captures() {
    let fonts = fonts();
    let mut cache = Vec::new();
    for (name, html) in [
        ("bank", include_str!("fixtures/bank_css.html")),
        ("newsletter", include_str!("fixtures/newsletter.html")),
        ("nested", include_str!("fixtures/nested_table.html")),
    ] {
        let mut job = job(html);
        job.key.width = 900;
        job.key.height = 800;
        job.key.viewport_height = 800;
        let started = std::time::Instant::now();
        let frame = render(&job, &fonts, &mut cache);
        println!("{name}: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0);
        assert!(!frame.data.failed);
        assert!(frame.data.height > 80.0);
        if let Ok(path) = std::env::var("MAILCRAFT_READER_CAPTURE_DIR") {
            let pixels = frame.pixels.unwrap();
            let mut bytes = Vec::with_capacity(pixels.pixels.len() * 4);
            for pixel in pixels.pixels {
                bytes.extend_from_slice(&pixel.to_srgba_unmultiplied());
            }
            std::fs::create_dir_all(&path).unwrap();
            image::save_buffer(
                std::path::Path::new(&path).join(format!("{name}.png")),
                &bytes,
                pixels.size[0] as u32,
                pixels.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
    assert!(cache.len() <= 2);
}

#[test]
fn advanced_progress_tables_connect_without_cell_gaps() {
    let mut j = job(include_str!("fixtures/progress.html"));
    j.key.width = 568;
    j.key.height = 140;
    let mut cache = Vec::new();
    let frame = render(&j, &fonts(), &mut cache);
    assert!(!frame.data.failed);
    let doc = &cache[0].doc;
    let sides = doc.query_selector_all("td[width='50%']").unwrap();
    let left = doc.node_client_rects(sides[0])[0];
    let right = doc.node_client_rects(sides[1])[0];
    assert!((left.width - right.width).abs() < 1.0);
    assert!((right.x - left.x - left.width - 18.0).abs() < 1.0, "circle must retain its intrinsic width");
    let image = frame.pixels.unwrap();
    // The rail between the first and last circle must have no white seams.
    for x in 80..490 {
        let c = image.pixels[9 * 568 + x];
        assert!(!(c.r() > 245 && c.g() > 245 && c.b() > 245), "gap at {x}");
    }
}
