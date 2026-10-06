//! Grafik fan curve interaktif: titik bisa diseret vertikal, titik aktif berdenyut.
//!
//! Sumbu X = nomor titik (merata, karena rentang suhu antar titik tumpang tindih),
//! sumbu Y = pwm 0–153 (0–6000 rpm, label rpm di kiri). Yang digambar hanya garis, isian
//! tipis, dan titik: tanpa bingkai kartu.

use crate::{
    legion::{pwm_to_rpm, Point},
    theme,
};
use eframe::egui::{
    epaint::{Mesh, Vertex, WHITE_UV},
    Align2, CursorIcon, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2,
};

const HEIGHT: f32 = 290.0;
const MARGIN: (f32, f32, f32, f32) = (40.0, 34.0, 18.0, 28.0); // kiri, kanan, atas, bawah
/// Kipas fisik mentok ±5100 rpm (uji kipas penuh), jadi skala berhenti di 6000 rpm; pwm di atas
/// ini tidak menambah kecepatan. Satuan: pwm ≈ rpm × 255 / 10000 (skala firmware).
pub const PWM_MAX: f32 = 153.0;

/// Gambar kurva. `active` = titik aktif kipas 1 dan 2; `ranges` = rentang suhu CPU tiap titik
/// (hanya untuk tooltip). Mengembalikan `true` bila user menyeret titik (buffer berubah).
pub fn curve_plot(ui: &mut Ui, curve: &mut [Point], active: (Option<usize>, Option<usize>), ranges: &[Option<(u8, u8)>]) -> bool {
    let n = curve.len();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    if n < 2 {
        return false;
    }
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + MARGIN.0, rect.top() + MARGIN.2),
        Pos2::new(rect.right() - MARGIN.1, rect.bottom() - MARGIN.3),
    );
    let x_of = |i: usize| plot.left() + plot.width() * i as f32 / (n - 1) as f32;
    let y_of = |pwm: u8| plot.bottom() - plot.height() * f32::from(pwm).min(PWM_MAX) / PWM_MAX;
    let mono = |s: f32| FontId::new(s, FontFamily::Name("geist-mono".into()));

    // Garis bantu tiap 2000 rpm; label di kiri.
    for k in 0..=3u32 {
        let rpm = k * 2000;
        let pwm = (rpm as f32 * PWM_MAX / 6_000.0).round() as u8;
        let y = y_of(pwm);
        ui.painter().line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, if k == 0 { theme::SURFACE_HI } else { theme::LINE }));
        let label = if k == 0 { "0".to_string() } else { format!("{}k", rpm / 1000) };
        ui.painter().text(Pos2::new(plot.left() - 10.0, y), Align2::RIGHT_CENTER, label, mono(10.5), theme::TEXT_FAINT);
    }

    // Isian gradien di bawah garis kipas 1 (alpha tinggi di garis, 0 di dasar).
    let pts1: Vec<Pos2> = curve.iter().enumerate().map(|(i, p)| Pos2::new(x_of(i), y_of(p.pwm1))).collect();
    let mut mesh = Mesh::default();
    for w in pts1.windows(2) {
        let base = mesh.vertices.len() as u32;
        let (top, bot) = (theme::accent_a(40), theme::accent_a(0));
        for (pos, color) in [(w[0], top), (w[1], top), (Pos2::new(w[1].x, plot.bottom()), bot), (Pos2::new(w[0].x, plot.bottom()), bot)] {
            mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color });
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    ui.painter().add(Shape::mesh(mesh));

    // Kipas 2 hanya digambar bila berbeda dari kipas 1 (umumnya identik).
    if curve.iter().any(|p| p.pwm1 != p.pwm2) {
        let pts2: Vec<Pos2> = curve.iter().enumerate().map(|(i, p)| Pos2::new(x_of(i), y_of(p.pwm2))).collect();
        ui.painter().add(Shape::line(pts2, Stroke::new(1.5, theme::TEXT_FAINT)));
    }
    ui.painter().add(Shape::line(pts1.clone(), Stroke::new(2.0, theme::ACCENT)));

    // Titik: seret vertikal mengubah kedua kipas dengan selisih yang sama.
    let mut changed = false;
    let pulse = theme::breathe(ui.ctx(), 1.8);
    for i in 0..n {
        let c = pts1[i];
        let resp = ui.interact(Rect::from_center_size(c, Vec2::splat(26.0)), ui.id().with(("pt", i)), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let new = ((plot.bottom() - p.y) / plot.height() * PWM_MAX).round().clamp(0.0, PWM_MAX) as i16;
                let diff = i16::from(curve[i].pwm2) - i16::from(curve[i].pwm1);
                curve[i].pwm1 = new as u8;
                curve[i].pwm2 = (new + diff).clamp(0, PWM_MAX as i16) as u8;
                changed = true;
            }
        }
        let is_active = active.0 == Some(i) || active.1 == Some(i);
        let hot = resp.hovered() || resp.dragged();
        let painter = ui.painter();
        if is_active {
            painter.circle_stroke(c, 8.0 + 5.0 * pulse, Stroke::new(1.5, theme::accent_a((110.0 * (1.0 - pulse)) as u8)));
            painter.circle_filled(c, 5.5, theme::ACCENT);
            let label = format!("{} rpm", pwm_to_rpm(curve[i].pwm1));
            let above = c.y - 20.0 > plot.top();
            let pos = Pos2::new(c.x, if above { c.y - 16.0 } else { c.y + 16.0 });
            painter.text(pos, Align2::CENTER_CENTER, label, mono(11.0), theme::ACCENT);
        } else {
            painter.circle_filled(c, if hot { 6.0 } else { 4.5 }, theme::BG);
            painter.circle_stroke(c, if hot { 6.0 } else { 4.5 }, Stroke::new(2.0, if hot { theme::TEXT } else { theme::ACCENT }));
        }
        // Nomor titik di sumbu X.
        painter.text(
            Pos2::new(c.x, plot.bottom() + 14.0),
            Align2::CENTER_CENTER,
            format!("{}", i + 1),
            mono(10.5),
            if is_active { theme::ACCENT } else { theme::TEXT_FAINT },
        );
        let range = ranges.get(i).copied().flatten();
        resp.on_hover_ui(|ui| {
            ui.label(theme::mono(format!("Titik {}", i + 1), 12.0).color(theme::TEXT));
            ui.label(theme::mono(format!("pwm {}  ·  {} rpm", curve[i].pwm1, pwm_to_rpm(curve[i].pwm1)), 11.5).color(theme::TEXT_DIM));
            if let Some((lo, hi)) = range {
                let hi = if hi >= 127 { "∞".to_string() } else { format!("{hi}") };
                ui.label(theme::mono(format!("CPU {lo}–{hi}°C"), 11.5).color(theme::TEXT_FAINT));
            }
        });
    }
    changed
}
