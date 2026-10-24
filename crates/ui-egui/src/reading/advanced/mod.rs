// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Local full-format reader. Native selection/find remain available through the fallback.
mod worker;
use super::{
    Place,
    images::{self, Pictures, Remote},
    remote::Access,
};
use crate::{MailApp, theme::Tokens};
use egui::{Id, Rect, Sense, TextureHandle, pos2, vec2};
use mailcraft_model::Message;
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};
use worker::{Action, Backend, Data, Frame, Job, Key};
#[derive(Default)]
struct Store {
    entries: Vec<(Place, State)>,
}
static NEXT_VIEW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
struct State {
    id: u64,
    message: Arc<Message>,
    source: Arc<mailcraft_html::ReaderHtml>,
    native: bool,
    failed: bool,
    requested: Option<Key>,
    output: Arc<Mutex<Option<Frame>>>,
    frame: Option<Arc<Data>>,
    texture: Option<TextureHandle>,
    action: Option<(Action, egui::Pos2)>,
    serial: u64,
    menu_point: Option<egui::Pos2>,
    anchor: Option<egui::Pos2>,
    selected: String,
    copy_requested: bool,
    pending_since: Option<std::time::Instant>,
}
impl State {
    fn new(message: Arc<Message>, source: Arc<mailcraft_html::ReaderHtml>) -> Self {
        Self {
            id: NEXT_VIEW.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            message,
            source,
            native: false,
            failed: false,
            requested: None,
            output: Arc::new(Mutex::new(None)),
            frame: None,
            texture: None,
            action: None,
            serial: 0,
            menu_point: None,
            anchor: None,
            selected: String::new(),
            copy_requested: false,
            pending_since: None,
        }
    }
    fn command(&mut self, action: Action, point: egui::Pos2) {
        self.serial = self.serial.wrapping_add(1);
        self.action = Some((action, point));
    }
}
fn store(ctx: &egui::Context) -> Arc<Mutex<Store>> {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<Store>>>(Id::unique("local-html-states")).clone())
}
fn backend(ctx: &egui::Context) -> Option<Arc<Backend>> {
    let key = Id::unique("local-html-worker");
    if let Some(existing) = ctx.data(|d| d.get_temp::<Arc<Backend>>(key)) {
        return Some(existing);
    }
    let backend = Backend::start()?;
    ctx.data_mut(|d| d.insert_temp(key, backend.clone()));
    Some(backend)
}
#[cfg(test)]
thread_local! {
    static NATIVE_READER_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(super) fn native_reader_test() -> bool {
    NATIVE_READER_TEST.with(std::cell::Cell::get)
}

/// Keep synchronous native-reader fixtures independent of the asynchronous backend.
/// Production builds and the advanced-reader integration fixture do not use this hook.
#[cfg(test)]
pub(super) fn use_native_for_tests(ctx: &egui::Context) {
    NATIVE_READER_TEST.with(|native| native.set(true));
    ctx.data_mut(|d| d.insert_temp(Id::unique("native-reader-test"), true));
}

/// Returns false when the native reader should handle this body.
pub(super) fn show(_app: &mut MailApp, ui: &mut egui::Ui, message: &Arc<Message>, place: Place, access: Access) -> bool {
    #[cfg(test)]
    if ui.ctx().data(|d| d.get_temp::<bool>(Id::unique("native-reader-test")).unwrap_or_default()) {
        return false;
    }
    if std::env::var_os("MAILCRAFT_NATIVE_READER").is_some() {
        return false;
    }
    let Some(html) = message.body.html.as_deref().filter(|h| !h.trim().is_empty()) else { return false };
    let ctx = ui.ctx().clone();
    let states = store(&ctx);
    let mut state =
        {
            let Ok(mut store) = states.lock() else { return false };
            if let Some(at) = store.entries.iter().position(|(p, s)| {
                *p == place && (Arc::ptr_eq(&s.message, message) || (s.message.id == message.id && s.message.body == message.body))
            }) {
                let mut state = store.entries.remove(at).1;
                state.message = message.clone();
                state
            } else {
                store.entries.retain(|(p, s)| *p != place || s.message.id != message.id);
                let Some(source) = mailcraft_html::reader_html(html) else { return false };
                State::new(message.clone(), Arc::new(source))
            }
        };
    let advanced = if state.native || state.failed {
        if ui.small_button(tl!("Show original formatting")).clicked() {
            state.native = false;
            state.failed = false;
            state.requested = None;
        }
        false
    } else {
        draw(ui, &mut state, access, &ctx)
    };
    if let Ok(mut store) = states.lock() {
        while store.entries.len() >= 2 {
            store.entries.remove(0);
        }
        store.entries.push((place, state));
    }
    advanced
}
fn draw(ui: &mut egui::Ui, state: &mut State, access: Access, ctx: &egui::Context) -> bool {
    let Some(backend) = backend(ctx) else { return false };
    if backend.is_stopped() {
        state.failed = true;
        return false;
    }
    if state.pending_since.is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(3)) {
        backend.stop();
        state.failed = true;
        return false;
    }
    if state.pending_since.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let dark = Tokens::get(ctx).dark;
    let allowed = access == Access::Allowed;
    let arrived = state.output.lock().ok().and_then(|mut slot| slot.take());
    if let Some(frame) =
        arrived.filter(|f| f.data.key.source == Arc::as_ptr(&state.source) as usize && f.data.key.dark == dark && f.data.key.allowed == allowed)
    {
        state.pending_since = None;
        if frame.data.failed {
            state.failed = true;
            return false;
        }
        if frame.data.key.action == state.serial {
            if let Some((action, href)) = frame.action {
                match action {
                    Action::Open => ctx.open_url(egui::OpenUrl::new_tab(href.to_string())),
                    Action::Copy => ctx.copy_text(href.to_string()),
                    Action::Select(_) | Action::Clear => {
                        state.selected = href.to_string();
                        if state.copy_requested {
                            ctx.copy_text(state.selected.clone());
                            state.copy_requested = false;
                        }
                    }
                }
            }
            state.action = None;
        }
        if let Some(pixels) = frame.pixels {
            if let Some(texture) = state.texture.as_mut() {
                texture.set(pixels, egui::TextureOptions::LINEAR);
            } else {
                state.texture = Some(ctx.load_texture("local-html-tile", pixels, egui::TextureOptions::LINEAR));
            }
        }
        state.frame = Some(Arc::new(frame.data));
    }
    let height = state.frame.as_ref().map_or(600.0, |f| f.height);
    let width = ui.available_width().floor().clamp(1.0, 4096.0);
    let (_, rect) = ui.allocate_space(vec2(width, height));
    let response = ui.interact(rect, Id::unique(("local-html-body", state.id, ui.scope_id())), Sense::click_and_drag());
    let clipped = rect.intersect(ui.clip_rect());
    let visible = if clipped.is_positive() {
        clipped
    } else if state.frame.is_none() {
        // The outer scroll area may restore an offset beyond our provisional
        // height. Discover the real document height even before any tile is visible.
        Rect::from_min_size(rect.min, vec2(width, ui.clip_rect().height().clamp(1.0, 4096.0)))
    } else {
        return true;
    };
    let tile_height = visible.height().ceil().clamp(1.0, 4096.0);
    let density = ctx.pixels_per_point().clamp(0.5, 2.0).min((4_000_000.0 / (width * tile_height)).sqrt()).max(0.1);
    let y = (visible.top() - rect.top()).max(0.0).floor() as u32;
    let mut pictures = BTreeMap::new();
    let cache = Pictures::of(ctx);
    let mut total = 0usize;
    if let Ok(mut cache) = cache.lock() {
        for url in &state.source.images {
            let bytes = if let Some(a) = images::attachment(&state.message, url) {
                Some(a.data.clone())
            } else if allowed {
                match cache.remote(url, images::frame(ctx)) {
                    Some(Remote::Ready(data)) => Some(data),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(bytes) = bytes.filter(|b| !b.is_empty() && b.len() <= 8 * 1024 * 1024) {
                if total.saturating_add(bytes.len()) > 16 * 1024 * 1024 {
                    break;
                }
                total = total.saturating_add(bytes.len());
                pictures.insert(url.clone(), bytes);
            }
        }
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (url, bytes) in &pictures {
        url.hash(&mut hasher);
        (Arc::as_ptr(bytes) as usize).hash(&mut hasher);
        bytes.len().hash(&mut hasher);
    }
    let mut key = Key {
        source: Arc::as_ptr(&state.source) as usize,
        view: state.id,
        width: width as u32,
        height: tile_height as u32,
        viewport_height: ui.clip_rect().height().ceil().clamp(1.0, 4096.0) as u32,
        y,
        density: density.to_bits(),
        dark,
        allowed,
        images: hasher.finish(),
        action: state.serial,
    };
    let ready = state.frame.clone();
    let matching = ready.as_ref().filter(|f| {
        f.key.source == key.source
            && f.key.images == key.images
            && f.key.width == key.width
            && f.key.height == key.height
            && f.key.viewport_height == key.viewport_height
            && f.key.y == key.y
            && f.key.density == key.density
            && f.key.dark == dark
            && f.key.allowed == allowed
    });
    // Reproject a still-valid tile during scrolling instead of blanking the
    // entire body while the worker catches up. Links require the exact frame.
    let paintable = ready.as_ref().filter(|f| {
        f.key.source == key.source
            && f.key.width == key.width
            && f.key.density == key.density
            && f.key.dark == key.dark
            && f.key.allowed == key.allowed
            && f.key.images == key.images
            && f.key.viewport_height == key.viewport_height
    });
    let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos()).map(|p| pos2(p.x - rect.left(), p.y - rect.top()));
    let href =
        matching.and_then(|frame| pointer.and_then(|point| frame.links.iter().rev().find(|(r, _)| r.contains(point)).map(|(_, url)| url.clone())));
    if href.is_some() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        response.request_focus();
        if let Some(point) = pointer {
            if href.is_some() {
                state.command(Action::Open, point);
            } else {
                state.command(Action::Clear, point);
            }
        }
    }
    if response.drag_started() {
        response.request_focus();
        state.anchor = ctx.input(|i| i.pointer.press_origin()).map(|p| pos2(p.x - rect.left(), p.y - rect.top()));
    }
    if response.dragged()
        && let (Some(anchor), Some(point)) = (state.anchor, pointer)
    {
        state.command(Action::Select(anchor), point);
    }
    if response.has_focus() && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
        if matches!(state.action, Some((Action::Select(_), _))) {
            state.copy_requested = true;
        } else {
            ctx.copy_text(state.selected.clone());
        }
    }
    if response.secondary_clicked() {
        state.menu_point = if href.is_some() { pointer } else { None };
    }
    response.context_menu(|ui| {
        if ui.add_enabled(!state.selected.is_empty(), egui::Button::new(tl!("Copy"))).clicked() {
            ctx.copy_text(state.selected.clone());
            ui.close();
        }

        if href.is_some() || state.menu_point.is_some() {
            if ui.button(tl!("Open link")).clicked() {
                if let Some(point) = state.menu_point {
                    state.command(Action::Open, point);
                }
                ui.close();
            }
            if ui.button(tl!("Copy link address")).clicked() {
                if let Some(point) = state.menu_point {
                    state.command(Action::Copy, point);
                }
                ui.close();
            }
            ui.separator();
        }
        if ui.button(tl!("Simplified layout")).clicked() {
            state.native = true;
            ui.close();
            ctx.request_repaint();
        }
    });
    // Native reader owns its established text selection, keyboard and find commands.
    if response.has_focus() && ctx.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::A) || i.key_pressed(egui::Key::F))) {
        state.native = true;
        ctx.request_repaint();
    }
    key.action = state.serial;
    if state.requested != Some(key) {
        if backend.submit(Job { key, source: state.source.clone(), pictures, ctx: ctx.clone(), output: state.output.clone(), action: state.action }) {
            state.requested = Some(key);
            if state.pending_since.is_none() {
                state.pending_since = Some(std::time::Instant::now());
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    if let Some(frame) = paintable
        && let Some(texture) = &state.texture
    {
        let target = Rect::from_min_size(pos2(rect.left(), rect.top() + frame.key.y as f32), vec2(frame.key.width as f32, frame.key.height as f32));
        ui.painter().image(texture.id(), target, Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0)), egui::Color32::WHITE);
    } else {
        ui.painter().text(
            visible.center(),
            egui::Align2::CENTER_CENTER,
            tl!("Preparing original formatting…"),
            crate::theme::regular(13.0),
            Tokens::get(ctx).text_dim,
        );
    }
    true
}

#[cfg(test)]
mod ui_tests;
