// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;
use std::time::{Duration, Instant};

#[test]
fn advanced_ui_scroll_click_reaches_the_existing_browser_output() {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(2.0);
    let (tx, rx) = std::sync::mpsc::channel();
    ctx.set_request_repaint_callback(move |_| {
        let _ = tx.send(());
    });
    let mailbox = mailcraft_engine::sample::mailbox(0, 0);
    let mut app = crate::MailApp::new(mailcraft_engine::Session::new(mailbox), crate::Services::default());
    let message = Arc::new(Message {
        id:999999,
        body: mailcraft_model::Body {html:Some("<style>body{height:1200px}a{position:absolute;left:20px;top:620px;display:block;width:200px;height:60px}</style><a href='https://example.org/from-blitz'><span>Read article</span></a>".into()),..Default::default()},
        ..Default::default()
    });
    let mut position = egui::Pos2::ZERO;
    let mut opened = Vec::new();
    {
        let mut paint = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 400.0))), events, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                egui::ScrollArea::vertical().vertical_scroll_offset(600.0).show(ui, |ui| {
                    position = ui.cursor().min + vec2(30.0, 630.0);
                    assert!(show(&mut app, ui, &message, Place::Pane, Access::Blocked));
                });
            });
            for command in &output.platform_output.commands {
                if let egui::OutputCommand::OpenUrl(url) = command {
                    opened.push(url.url.clone());
                }
            }
            output.textures_delta.clear();
            position
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut attempts = 0;
        loop {
            attempts += 1;
            assert!(Instant::now() < deadline, "UI renderer did not become ready before deadline");
            paint(Vec::new());
            let ready = {
                let store = store(&ctx);
                let state = store.lock().unwrap();
                state
                    .entries
                    .last()
                    .is_some_and(|(_, s)| s.texture.is_some() && s.frame.as_ref().is_some_and(|f| Some(f.key) == s.requested && f.key.y >= 500))
            };
            if ready {
                break;
            }
            let states = store(&ctx);
            let states = states.lock().unwrap();
            if let Some((_, s)) = states.entries.last().filter(|_| attempts < 10) {
                eprintln!(
                    "frame={:?} requested={:?} failed={} native={} pending={}",
                    s.frame.as_ref().map(|f| f.key),
                    s.requested,
                    s.failed,
                    s.native,
                    s.action.is_some()
                );
            }
            drop(states);
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
        }
        let position = paint(Vec::new());
        {
            let states = store(&ctx);
            let states = states.lock().unwrap();
            let s = &states.entries.last().unwrap().1;
            eprintln!("CLICK position={position:?} rects={:?}", s.frame.as_ref().map(|f| f.links.clone()));
        }
        paint(vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton { pos: position, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            egui::Event::PointerButton { pos: position, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
        ]);
        let mut attempts = 0;
        loop {
            attempts += 1;
            assert!(Instant::now() < deadline, "UI renderer did not become ready before deadline");
            paint(Vec::new());
            let complete = {
                let store = store(&ctx);
                let state = store.lock().unwrap();
                state.entries.last().is_some_and(|(_, s)| s.serial > 0 && s.action.is_none())
            };
            if complete {
                break;
            }
            if attempts < 10 {
                let states = store(&ctx);
                let states = states.lock().unwrap();
                let s = &states.entries.last().unwrap().1;
                eprintln!("WAIT serial={} action={} requested={:?}", s.serial, s.action.is_some(), s.requested);
            }
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
        }
    }
    assert_eq!(opened, vec!["https://example.org/from-blitz"]);
}
