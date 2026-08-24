//! Toast 通知：底部居中滑入，停留后淡出消失。
//! 每条独立定位，淡出时同步收掉占位高度，避免堆叠中某一条消失时其余条目抽动。

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, Stroke};

use super::theme::anim;

const STAY: Duration = Duration::from_secs(4);
const GAP: f32 = 6.0;
const BOTTOM_PAD: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

struct Toast {
    id: u64,
    kind: ToastKind,
    text: String,
    at: Instant,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>) {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Toast {
            id,
            kind,
            text: text.into(),
            at: Instant::now(),
        });
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.push(ToastKind::Info, text);
    }

    pub fn success(&mut self, text: impl Into<String>) {
        self.push(ToastKind::Success, text);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push(ToastKind::Error, text);
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        self.items.retain(|t| t.at.elapsed() < STAY + anim::NORMAL);
        if self.items.is_empty() {
            return;
        }
        ctx.request_repaint();

        // 从底部往上排：新的在最下。淡出时 space_t→0，上方条目平滑下滑，不瞬间抽动。
        let mut offset = -BOTTOM_PAD;
        for t in self.items.iter().rev() {
            let age = t.at.elapsed();
            let appear = anim::ease_out((age.as_secs_f32() / anim::NORMAL.as_secs_f32()).min(1.0));
            let fade = if age > STAY {
                1.0 - ((age - STAY).as_secs_f32() / anim::NORMAL.as_secs_f32()).min(1.0)
            } else {
                1.0
            };
            let opacity = appear * fade;
            let space_t = appear * fade;
            let slide = (1.0 - appear) * 16.0 + (1.0 - fade) * 10.0;

            let inner = egui::Area::new(egui::Id::new(("toast", t.id)))
                .anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, offset + slide))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .show(ctx, |ui| {
                    ui.set_opacity(opacity);
                    toast_card(ui, t);
                });
            let h = inner.response.rect.height();
            offset -= (h + GAP) * space_t;
        }
    }
}

fn toast_card(ui: &mut egui::Ui, t: &Toast) {
    let p = *super::theme::palette(ui.ctx());
    let (icon, color) = match t.kind {
        ToastKind::Info => ("ℹ", p.accent),
        ToastKind::Success => ("✔", p.success),
        ToastKind::Error => ("✖", p.danger),
    };
    egui::Frame::default()
        .fill(p.card)
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 0,
            color: p.shadow_toast,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icon).color(color).size(14.0).strong());
                ui.label(egui::RichText::new(&t.text).color(p.text));
            });
        });
}
