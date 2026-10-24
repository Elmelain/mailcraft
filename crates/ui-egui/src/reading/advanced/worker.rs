// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! One CPU renderer thread; one queued job, one reply and at most two retained DOMs.
use anyrender::ImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::HtmlDocument;
use blitz_traits::{
    net::{Bytes, NetHandler, NetProvider, Request, Url},
    shell::{ColorScheme, Viewport},
};
use egui::{ColorImage, Rect, pos2, vec2};
use mailcraft_html::ReaderHtml;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub source: usize,
    pub view: u64,
    pub width: u32,
    pub height: u32,
    pub viewport_height: u32,
    pub y: u32,
    pub density: u32,
    pub dark: bool,
    pub allowed: bool,
    pub images: u64,
    pub action: u64,
}
#[derive(Clone, Copy)]
pub enum Action {
    Open,
    Copy,
    Select(egui::Pos2),
    Clear,
}
pub struct Job {
    pub key: Key,
    pub source: Arc<ReaderHtml>,
    pub pictures: BTreeMap<String, Arc<Vec<u8>>>,
    pub ctx: egui::Context,
    pub output: Arc<Mutex<Option<Frame>>>,
    pub action: Option<(Action, egui::Pos2)>,
}
#[derive(Clone)]
pub struct Data {
    pub key: Key,
    pub height: f32,
    pub links: Vec<(Rect, Arc<str>)>,
    pub failed: bool,
}
pub struct Frame {
    pub data: Data,
    pub pixels: Option<ColorImage>,
    pub action: Option<(Action, Arc<str>)>,
}
pub struct Backend {
    tx: SyncSender<Job>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}
impl Backend {
    pub fn start() -> Option<Arc<Self>> {
        let (tx, rx) = mpsc::sync_channel::<Job>(1);
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        std::thread::Builder::new()
            .name("mailcraft-html-cpu".into())
            .spawn(move || {
                let fonts = fonts();
                let mut cache: Vec<Parsed> = Vec::new();
                while let Ok(job) = rx.recv() {
                    if worker_stopped.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(&job, &fonts, &mut cache)));
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(_) => {
                            cache.clear();
                            Frame { data: Data { key: job.key, height: 1.0, links: Vec::new(), failed: true }, pixels: None, action: None }
                        }
                    };
                    if let Ok(mut slot) = job.output.lock() {
                        *slot = Some(frame);
                    }
                    job.ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Arc::new(Self { tx, stopped }))
    }
    pub fn stop(&self) {
        self.stopped.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn submit(&self, job: Job) -> bool {
        if self.is_stopped() {
            return false;
        }
        match self.tx.try_send(job) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }
}
struct Pending {
    url: String,
    handler: Box<dyn NetHandler>,
}
struct Resources {
    allowed: BTreeSet<String>,
    pending: Mutex<Vec<Pending>>,
}
fn normal(src: &str) -> Option<String> {
    let u = Url::parse(src).ok()?;
    if !matches!(u.scheme(), "http" | "https" | "cid") || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    Some(u.to_string())
}
impl NetProvider for Resources {
    fn is_noop(&self) -> bool {
        false
    }
    fn fetch(&self, _id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();
        if !self.allowed.contains(&url) {
            handler.bytes(url, Bytes::new());
            return;
        }
        if let Ok(mut pending) = self.pending.lock()
            && pending.len() < 64
        {
            pending.push(Pending { url, handler });
        }
    }
}
fn fonts() -> FontContext {
    let mut fonts = FontContext::new();
    for (data, family) in [
        (include_bytes!("../../../../../assets/reader-fonts/LiberationSans-Regular.ttf").as_slice(), fontique::GenericFamily::SansSerif),
        (include_bytes!("../../../../../assets/reader-fonts/LiberationSans-Bold.ttf").as_slice(), fontique::GenericFamily::SansSerif),
        (include_bytes!("../../../../../assets/reader-fonts/LiberationSerif-Regular.ttf").as_slice(), fontique::GenericFamily::Serif),
        (include_bytes!("../../../../../assets/reader-fonts/LiberationSerif-Bold.ttf").as_slice(), fontique::GenericFamily::Serif),
        (include_bytes!("../../../../../assets/reader-fonts/LiberationMono-Regular.ttf").as_slice(), fontique::GenericFamily::Monospace),
        (include_bytes!("../../../../../assets/reader-fonts/LiberationMono-Bold.ttf").as_slice(), fontique::GenericFamily::Monospace),
    ] {
        let ids = fonts.collection.register_fonts(fontique::Blob::new(Arc::new(data.to_vec())), None);
        fonts.collection.append_generic_families(family, ids.iter().map(|(id, _)| *id));
        if family == fontique::GenericFamily::SansSerif {
            fonts.collection.append_generic_families(fontique::GenericFamily::SystemUi, ids.iter().map(|(id, _)| *id));
        }
    }
    fonts
}
struct Parsed {
    view: u64,
    source: Arc<ReaderHtml>,
    picture_version: u64,
    allowed: bool,
    dark: bool,
    doc: HtmlDocument,
    resources: Arc<Resources>,
    viewport: Viewport,
    loaded: BTreeSet<String>,
    decoded: usize,
    fresh: bool,
    painter: VelloCpuImageRenderer,
}
fn viewport(key: Key) -> Viewport {
    let density = f32::from_bits(key.density);
    Viewport::new(
        (key.width as f32 * density).ceil() as u32,
        (key.viewport_height as f32 * density).ceil() as u32,
        density,
        if key.dark { ColorScheme::Dark } else { ColorScheme::Light },
    )
}
impl Parsed {
    fn new(job: &Job, fonts: &FontContext) -> Self {
        let viewport = viewport(job.key);
        let resources =
            Arc::new(Resources { allowed: job.source.images.iter().filter_map(|s| normal(s)).collect(), pending: Mutex::new(Vec::new()) });
        let mut doc = HtmlDocument::from_html(
            &job.source.html,
            DocumentConfig {
                viewport: Some(viewport.clone()),
                base_url: Some("https://mailcraft.invalid/".into()),
                font_ctx: Some(fonts.clone()),
                net_provider: Some(resources.clone()),
                ..Default::default()
            },
        );
        doc.add_user_agent_stylesheet("html,body{margin:0;padding:0}html{font:16px sans-serif}table[align=center]{margin-left:auto;margin-right:auto}table[cellspacing=\"0\"]{border-spacing:0}table[cellpadding=\"0\"]>*>tr>td,table[cellpadding=\"0\"]>*>tr>th,table[cellpadding=\"0\"]>tr>td,table[cellpadding=\"0\"]>tr>th{padding:0}td[align=center],th[align=center]{text-align:center}td[align=right],th[align=right]{text-align:right}img{max-width:100%}*{animation:none!important;transition:none!important}");
        doc.add_user_agent_stylesheet(if job.key.dark { "html{background:#202124;color:#e8eaed}" } else { "html{background:white;color:#202124}" });
        Self {
            view: job.key.view,
            source: job.source.clone(),
            picture_version: job.key.images,
            allowed: job.key.allowed,
            dark: job.key.dark,
            doc,
            resources,
            viewport,
            loaded: BTreeSet::new(),
            decoded: 0,
            fresh: true,
            painter: VelloCpuImageRenderer::new(1, 1),
        }
    }
    fn resources(&mut self, job: &Job) -> bool {
        let pending = match self.resources.pending.lock() {
            Ok(mut p) => std::mem::take(&mut *p),
            Err(_) => return false,
        };
        let mut retry = Vec::new();
        let mut dirty = false;
        for request in pending {
            if !job.key.allowed && (request.url.starts_with("http://") || request.url.starts_with("https://")) {
                retry.push(request);
                continue;
            }
            if self.loaded.contains(&request.url) {
                continue;
            }
            let bytes = job.pictures.iter().find(|(url, _)| normal(url).as_deref() == Some(&request.url)).map(|(_, bytes)| bytes);
            let Some(bytes) = bytes else {
                retry.push(request);
                continue;
            };
            if bytes.len() > 8 * 1024 * 1024 {
                continue;
            }
            let reader = image::ImageReader::new(std::io::Cursor::new(bytes.as_slice())).with_guessed_format();
            let Some((w, h)) = reader.ok().and_then(|r| r.into_dimensions().ok()) else { continue };
            let cost = (w as usize).saturating_mul(h as usize).saturating_mul(4);
            if w == 0 || h == 0 || w > 8192 || h > 8192 || self.decoded.saturating_add(cost) > 16 * 1024 * 1024 {
                continue;
            }
            self.decoded = self.decoded.saturating_add(cost);
            self.loaded.insert(request.url.clone());
            request.handler.bytes(request.url, Bytes::copy_from_slice(bytes));
            dirty = true;
        }
        if let Ok(mut p) = self.resources.pending.lock() {
            p.extend(retry);
        }
        if dirty {
            self.doc.handle_messages();
        }
        dirty
    }
}
fn render(job: &Job, fonts: &FontContext, cache: &mut Vec<Parsed>) -> Frame {
    let at = cache.iter().position(|p| {
        p.view == job.key.view
            && Arc::ptr_eq(&p.source, &job.source)
            && (p.picture_version == job.key.images || p.loaded.is_empty())
            && p.allowed == job.key.allowed
            && p.dark == job.key.dark
    });
    let mut parsed = at.map(|at| cache.remove(at)).unwrap_or_else(|| Parsed::new(job, fonts));
    parsed.picture_version = job.key.images;
    let vp = viewport(job.key);
    let mut dirty = parsed.resources(job);
    if vp != parsed.viewport {
        parsed.doc.set_viewport(vp.clone());
        parsed.viewport = vp;
        dirty = true;
    }
    if dirty || parsed.fresh {
        parsed.doc.resolve(0.0);
        parsed.fresh = false;
    }
    if parsed.resources(job) {
        parsed.doc.resolve(0.0);
    }
    parsed.doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
    let height = parsed.doc.root_element().scroll_height().max(parsed.doc.root_element().final_layout().size.height).clamp(1.0, 200_000.0);
    let mut links = Vec::new();
    if let Ok(nodes) = parsed.doc.query_selector_all("a[href]") {
        for node in nodes.into_iter().take(2048) {
            let href = parsed
                .doc
                .get_node(node)
                .and_then(|n| n.element_data())
                .and_then(|e| e.attr(blitz_dom::local_name!("href")))
                .filter(|s| mailcraft_html::is_safe_url(s))
                .map(Arc::<str>::from);
            if let Some(href) = href {
                for r in parsed.doc.node_client_rects(node).into_iter().take(32) {
                    if links.len() >= 4096 {
                        break;
                    }
                    if r.width <= 0.0 || r.height <= 0.0 {
                        continue;
                    }
                    links.push((Rect::from_min_size(pos2(r.x as f32, r.y as f32), vec2(r.width as f32, r.height as f32)), href.clone()));
                }
            }
        }
    }
    let density = f32::from_bits(job.key.density);
    let width = (job.key.width as f32 * density).ceil().max(1.0) as u32;
    let tile = (job.key.height as f32 * density).ceil().max(1.0) as u32;
    parsed.doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: f64::from(job.key.y) });
    let action = job.action.and_then(|(action, point)| {
        if let Action::Select(anchor) = action {
            if let Some((node, offset)) = parsed.doc.find_text_position(anchor.x, anchor.y) {
                parsed.doc.set_text_selection(node, offset, node, offset);
                parsed.doc.extend_text_selection_to_point(point.x, point.y);
            }
            return Some((action, Arc::<str>::from(parsed.doc.get_selected_text().unwrap_or_default())));
        }
        if let Action::Clear = action {
            parsed.doc.clear_text_selection();
            return Some((action, Arc::<str>::from("")));
        }
        let mut node = parsed.doc.hit(point.x, point.y).map(|hit| hit.node_id);
        for _ in 0..128 {
            let current = parsed.doc.get_node(node?)?;
            if let Some(href) =
                current.element_data().and_then(|e| e.attr(blitz_dom::local_name!("href"))).filter(|href| mailcraft_html::is_safe_url(href))
            {
                return Some((action, Arc::<str>::from(href)));
            }
            node = current.parent;
        }
        None
    });
    parsed.painter.resize(width, tile);
    parsed.painter.reset();
    let mut rgba = Vec::new();
    parsed.painter.render_to_vec(|scene| blitz_paint::paint_scene(scene, &mut parsed.doc, f64::from(density), width, tile, 0, 0), &mut rgba);
    let pixels = ColorImage::from_rgba_premultiplied([width as usize, tile as usize], &rgba);
    while cache.len() >= 2 {
        cache.remove(0);
    }
    cache.push(parsed);
    Frame { data: Data { key: job.key, height, links, failed: false }, pixels: Some(pixels), action }
}
#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
