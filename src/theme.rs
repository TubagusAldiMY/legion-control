//! Sistem desain: palet, font, dan komponen kustom egui.
//!
//! Arah visual: netral zinc dengan SATU aksen (emerald, desaturasi), tipografi Outfit untuk
//! teks dan Geist Mono untuk setiap angka, tanpa kartu (pemisah 1px dan ruang kosong),
//! gerak lewat transform/opasitas saja. Semua warna ada di sini supaya halaman tidak
//! pernah memakai nilai mentah.

use eframe::egui::{
    self, emath::easing, epaint::StrokeKind, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Id, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use std::sync::Arc;

// Palet: zinc, satu keluarga abu dingin (tidak dicampur dengan abu hangat).
pub const BG: Color32 = Color32::from_rgb(9, 9, 11); // zinc-950, bukan hitam murni
pub const SIDEBAR: Color32 = Color32::from_rgb(13, 13, 16);
pub const SURFACE: Color32 = Color32::from_rgb(24, 24, 27); // zinc-900
pub const SURFACE_HI: Color32 = Color32::from_rgb(39, 39, 42); // zinc-800
pub const LINE: Color32 = Color32::from_rgb(34, 34, 38);
pub const TEXT: Color32 = Color32::from_rgb(228, 228, 231); // zinc-200
pub const TEXT_DIM: Color32 = Color32::from_rgb(161, 161, 170); // zinc-400
pub const TEXT_FAINT: Color32 = Color32::from_rgb(113, 113, 122); // zinc-500
/// Satu-satunya aksen: emerald 400, saturasi di bawah 80%.
pub const ACCENT: Color32 = Color32::from_rgb(52, 211, 153);
/// Aksen untuk teks di atas isian aksen (kontras tinggi, bukan hitam murni).
pub const ON_ACCENT: Color32 = Color32::from_rgb(6, 32, 23);
/// Semantik galat saja (bukan aksen dekoratif).
pub const DANGER: Color32 = Color32::from_rgb(248, 113, 113);

pub fn accent_a(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(52, 211, 153, alpha)
}

const OUTFIT: &str = "outfit";
const OUTFIT_BOLD: &str = "outfit-bold";
const MONO: &str = "geist-mono";
const MONO_BOLD: &str = "geist-mono-bold";

/// Pasang font dan gaya global. Dipanggil sekali saat aplikasi dibuat.
pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let files: [(&str, &[u8]); 4] = [
        (OUTFIT, include_bytes!("../assets/fonts/Outfit-Regular.ttf")),
        (OUTFIT_BOLD, include_bytes!("../assets/fonts/Outfit-Bold.ttf")),
        (MONO, include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
        (MONO_BOLD, include_bytes!("../assets/fonts/GeistMono-Bold.ttf")),
    ];
    for (name, bytes) in files {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
        fonts.families.insert(FontFamily::Name(name.into()), vec![name.into()]);
    }
    // Teks biasa & monospace bawaan egui diganti; font bawaan tetap sebagai cadangan glyph.
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, OUTFIT.into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, MONO.into());
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::ThemePreference::Dark);

    ctx.global_style_mut(|style| {
        use egui::TextStyle::*;
        style.text_styles = [
            (Heading, FontId::new(20.0, FontFamily::Name(OUTFIT_BOLD.into()))),
            (Body, FontId::new(14.0, FontFamily::Proportional)),
            (Button, FontId::new(14.0, FontFamily::Proportional)),
            (Small, FontId::new(11.5, FontFamily::Proportional)),
            (Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 8.0);
        style.spacing.interact_size.y = 30.0;
        // Scrollbar tipis dan menyisihkan ruang sendiri (tidak menimpa konten seperti grafik).
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.bar_inner_margin = 2.0;
        style.spacing.scroll.bar_outer_margin = 4.0;
        style.spacing.scroll.foreground_color = true;
        style.spacing.scroll.dormant_background_opacity = 0.0;
        style.spacing.scroll.active_background_opacity = 0.0;
        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = BG;
        v.extreme_bg_color = SURFACE;
        v.faint_bg_color = SURFACE;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = accent_a(60);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
            w.corner_radius = CornerRadius::same(8);
        }
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(63, 63, 70)); // handle scrollbar redup
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_FAINT);
        v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        v.widgets.inactive.bg_fill = SURFACE;
        v.widgets.inactive.weak_bg_fill = SURFACE;
        v.widgets.hovered.bg_fill = SURFACE_HI;
        v.widgets.hovered.weak_bg_fill = SURFACE_HI;
        v.widgets.active.bg_fill = SURFACE_HI;
        v.widgets.active.weak_bg_fill = SURFACE_HI;
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow::NONE;
    });
}

/// Judul halaman rata kiri (tidak berteriak: 20px bold + keterangan redup).
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(FontId::new(20.0, FontFamily::Name(OUTFIT_BOLD.into()))).color(TEXT));
    ui.label(RichText::new(subtitle).size(13.0).color(TEXT_FAINT));
    ui.add_space(18.0);
}

pub fn bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name(OUTFIT_BOLD.into()))).color(TEXT)
}

pub fn mono(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name(MONO.into())))
}

/// Label kecil huruf kapital untuk judul bagian (tanpa kartu; hierarki lewat warna & ukuran).
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).size(11.0).color(TEXT_FAINT));
}

/// Pemisah 1px selebar area.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, LINE);
}

/// Angka yang menyusul targetnya dengan halus (rpm/suhu), bukan melompat. Memakai peluruhan
/// eksponensial dan repaint dibatasi ±30 fps selama bergerak: `animate_value_with_time` milik
/// egui memaksa repaint setinggi refresh rate monitor dan membuat CPU naik tanpa alasan.
pub fn tween(ctx: &egui::Context, id: impl std::hash::Hash + std::fmt::Debug, target: f32) -> f32 {
    let key = Id::new(("tween", id));
    let cur: f32 = ctx.data_mut(|d| *d.get_temp_mut_or(key, target));
    let dt = ctx.input(|i| i.stable_dt).min(0.1);
    let next = if (target - cur).abs() < 0.5 {
        target
    } else {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        cur + (target - cur) * (1.0 - (-dt * 9.0).exp())
    };
    ctx.data_mut(|d| d.insert_temp(key, next));
    next
}

/// Denyut lambat 0..1 untuk indikator "hidup". Animasi terus-menerus hanya berjalan saat jendela
/// difokus (±12 fps); tanpa fokus nilainya diam, karena aplikasi pengatur pendinginan tidak
/// boleh sendirinya membebani CPU.
pub fn breathe(ctx: &egui::Context, period_s: f64) -> f32 {
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
    if !focused {
        return 0.5;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(80));
    let t = ctx.input(|i| i.time);
    (0.5 + 0.5 * (t * std::f64::consts::TAU / period_s).sin()) as f32
}

/// Titik status (denyut halus bila `live`).
pub fn status_dot(ui: &mut Ui, color: Color32, live: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let c = rect.center();
    if live {
        let p = breathe(ui.ctx(), 2.4);
        ui.painter().circle_filled(c, 4.0 + 3.0 * p, color.gamma_multiply(0.22 * (1.0 - p)));
    }
    ui.painter().circle_filled(c, 3.5, color);
}

/// Saklar on/off dengan geser knob (ease-out cubic). Mengembalikan `true` saat diklik.
pub fn switch(ui: &mut Ui, on: bool, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let t = ui.ctx().animate_bool_with_easing(resp.id, on, easing::cubic_out);
    let track = lerp_color(SURFACE_HI, ACCENT, t);
    let alpha = if enabled { 1.0 } else { 0.4 };
    let p = ui.painter();
    p.rect_filled(rect, 11, track.gamma_multiply(alpha));
    let hover = if resp.hovered() && enabled { 1.0 } else { 0.0 };
    p.rect_stroke(rect, 11, Stroke::new(1.0, Color32::from_white_alpha((14.0 + 14.0 * hover) as u8)), StrokeKind::Inside);
    let x = rect.left() + 11.0 + t * 18.0;
    let press = if resp.is_pointer_button_down_on() { 1.0 } else { 0.0 };
    p.circle_filled(Pos2::new(x, rect.center().y), 7.5 - press, Color32::from_rgb(244, 244, 245).gamma_multiply(alpha));
    resp.clicked()
}

/// Satu baris pengaturan: judul + petunjuk di kiri, saklar di kanan, pemisah di bawah.
/// `index` dipakai untuk reveal bertahap saat halaman dibuka.
pub fn toggle_row(ui: &mut Ui, title: &str, hint: &str, on: bool, enabled: bool) -> bool {
    let mut clicked = false;
    let w = ui.available_width();
    ui.horizontal(|ui| {
        // Teks dibatasi lebarnya supaya petunjuk panjang membungkus, bukan menimpa saklar.
        ui.allocate_ui_with_layout(Vec2::new((w - 56.0).max(80.0), 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(14.5).color(if enabled { TEXT } else { TEXT_FAINT }));
            ui.label(RichText::new(hint).size(12.0).color(TEXT_FAINT));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            clicked = switch(ui, on, enabled);
        });
    });
    ui.add_space(6.0);
    divider(ui);
    ui.add_space(6.0);
    clicked
}

/// Segmented control dengan pil yang meluncur ke pilihan aktif. Mengembalikan opsi yang diklik.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, id: &str, options: &[(T, &str)], selected: Option<T>) -> Option<T> {
    let font = FontId::new(13.5, FontFamily::Proportional);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, l)| ui.painter().layout_no_wrap((*l).into(), font.clone(), TEXT).size().x + 28.0)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 34.0), Sense::hover());
    ui.painter().rect_filled(rect, 10, SURFACE);
    ui.painter().rect_stroke(rect, 10, Stroke::new(1.0, LINE), StrokeKind::Inside);

    // Posisi pil aktif dianimasikan (x dan lebar) supaya meluncur antar opsi.
    let sel_idx = selected.and_then(|s| options.iter().position(|(o, _)| *o == s));
    let (mut x, mut target_x, mut target_w) = (rect.left() + 3.0, rect.left() + 3.0, 0.0);
    let mut spans = vec![];
    for w in &widths {
        spans.push((x, *w));
        x += w;
    }
    if let Some(i) = sel_idx {
        target_x = spans[i].0;
        target_w = spans[i].1;
    }
    let ctx = ui.ctx().clone();
    let ax = ctx.animate_value_with_time(Id::new((id, "x")), target_x, 0.22);
    let aw = ctx.animate_value_with_time(Id::new((id, "w")), target_w, 0.22);
    if sel_idx.is_some() {
        let pill = Rect::from_min_size(Pos2::new(ax, rect.top() + 3.0), Vec2::new(aw, 28.0));
        ui.painter().rect_filled(pill, 8, SURFACE_HI);
        ui.painter().rect_stroke(pill, 8, Stroke::new(1.0, Color32::from_white_alpha(18)), StrokeKind::Inside);
    }

    let mut clicked = None;
    for (i, (opt, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(spans[i].0, rect.top() + 3.0), Vec2::new(spans[i].1, 28.0));
        let resp = ui.interact(r, Id::new((id, i)), Sense::click());
        let active = sel_idx == Some(i);
        let color = if active { TEXT } else if resp.hovered() { TEXT_DIM } else { TEXT_FAINT };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), color);
        if resp.clicked() && !active {
            clicked = Some(*opt);
        }
    }
    clicked
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Ghost,
}

/// Tombol dengan umpan balik fisik: saat ditekan bergeser 1px dan mengecil sedikit.
pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind, enabled: bool) -> egui::Response {
    let font = FontId::new(13.5, FontFamily::Name(if kind == ButtonKind::Primary { OUTFIT_BOLD } else { OUTFIT }.into()));
    let galley = ui.painter().layout_no_wrap(label.into(), font.clone(), TEXT);
    let size = Vec2::new(galley.size().x + 32.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let pressed = resp.is_pointer_button_down_on() && enabled;
    let hover = ctx_anim(ui, resp.id, resp.hovered() && enabled);
    let draw = if pressed { rect.shrink(1.0).translate(Vec2::new(0.0, 1.0)) } else { rect };
    let alpha = if enabled { 1.0 } else { 0.38 };
    match kind {
        ButtonKind::Primary => {
            ui.painter().rect_filled(draw, 9, lerp_color(ACCENT, Color32::from_rgb(110, 231, 183), hover).gamma_multiply(alpha));
            ui.painter().text(draw.center(), Align2::CENTER_CENTER, label, font, ON_ACCENT.gamma_multiply(alpha));
        }
        ButtonKind::Ghost => {
            ui.painter().rect_filled(draw, 9, lerp_color(Color32::TRANSPARENT, SURFACE_HI, hover));
            ui.painter().rect_stroke(draw, 9, Stroke::new(1.0, LINE.gamma_multiply(1.0 + hover)), StrokeKind::Inside);
            ui.painter().text(draw.center(), Align2::CENTER_CENTER, label, font, lerp_color(TEXT_DIM, TEXT, hover).gamma_multiply(alpha));
        }
    }
    resp
}

fn ctx_anim(ui: &Ui, id: Id, on: bool) -> f32 {
    ui.ctx().animate_bool_with_time(id, on, 0.14)
}

/// Blok statistik tanpa kartu: label kecil, angka mono besar, bilah tipis. `frac` 0..1.
pub fn stat(ui: &mut Ui, label: &str, value: Option<f32>, unit: &str, frac: f32, danger: bool, id: &str) {
    ui.spacing_mut().item_spacing.y = 3.0;
    section_label(ui, label);
    match value {
        Some(v) => {
            let v = tween(ui.ctx(), id, v);
            let color = if danger { DANGER } else { TEXT };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(mono(format!("{:.0}", v), 30.0).color(color));
                ui.label(RichText::new(unit).size(13.0).color(TEXT_FAINT));
            });
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(220.0), 3.0), Sense::hover());
            ui.painter().rect_filled(rect, 2, LINE);
            let fill = Rect::from_min_size(rect.min, Vec2::new(rect.width() * tween(ui.ctx(), (id, "bar"), frac.clamp(0.0, 1.0)), 3.0));
            ui.painter().rect_filled(fill, 2, if danger { DANGER } else { ACCENT });
        }
        None => {
            skeleton(ui, 90.0, 28.0);
            skeleton(ui, 180.0, 3.0);
        }
    }
}

/// Placeholder pemuat dengan kilau yang bergeser (pengganti spinner bundar).
pub fn skeleton(ui: &mut Ui, w: f32, h: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w.min(ui.available_width()), h), Sense::hover());
    ui.painter().rect_filled(rect, (h / 2.0).min(6.0) as u8, SURFACE);
    let t = ui.ctx().input(|i| i.time) as f32;
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    let phase = (t * 0.9).fract();
    let sheen_w = rect.width() * 0.35;
    let x = rect.left() - sheen_w + (rect.width() + sheen_w) * phase;
    let sheen = Rect::from_min_max(Pos2::new(x.max(rect.left()), rect.top()), Pos2::new((x + sheen_w).min(rect.right()), rect.bottom()));
    if sheen.width() > 0.0 {
        ui.painter().rect_filled(sheen, (h / 2.0).min(6.0) as u8, Color32::from_white_alpha(8));
    }
}

/// Blok kode dengan tombol salin.
pub fn code_block(ui: &mut Ui, text: &str) {
    egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, LINE)).corner_radius(10).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(mono(text, 12.0).color(TEXT_DIM));
        ui.add_space(6.0);
        if button(ui, "Salin perintah", ButtonKind::Ghost, true).clicked() {
            ui.ctx().copy_text(text.to_string());
        }
    });
}

/// Pesan status satu baris dengan titik berwarna; galat ditandai merah.
pub fn status_line(ui: &mut Ui, text: &str) {
    if text.is_empty() {
        return;
    }
    let err = text.starts_with("Gagal");
    ui.horizontal(|ui| {
        status_dot(ui, if err { DANGER } else { ACCENT }, false);
        ui.label(RichText::new(text).size(12.5).color(if err { DANGER } else { TEXT_DIM }));
    });
}

/// Reveal bertahap: baris ke-`index` memudar masuk dengan jeda 45 ms setelah `epoch` (waktu
/// halaman dibuka). Hanya opasitas yang dianimasikan.
pub fn reveal<R>(ui: &mut Ui, epoch: f64, index: usize, add: impl FnOnce(&mut Ui) -> R) -> R {
    let now = ui.ctx().input(|i| i.time);
    let t = ((now - epoch - index as f64 * 0.045) / 0.28).clamp(0.0, 1.0) as f32;
    if t < 1.0 {
        ui.ctx().request_repaint();
    }
    ui.scope(|ui| {
        ui.set_opacity(easing::cubic_out(t));
        add(ui)
    })
    .inner
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Ikon navigasi yang digambar dengan primitif (tanpa font ikon), goresan 1.6px konsisten.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Sliders,
    Fan,
    Chip,
}

pub fn paint_icon(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = Stroke::new(1.6, color);
    match icon {
        Icon::Sliders => {
            for (i, (y, knob)) in [(-5.5, -3.0), (0.0, 4.0), (5.5, -1.0)].into_iter().enumerate() {
                let _ = i;
                p.line_segment([Pos2::new(c.x - 7.0, c.y + y), Pos2::new(c.x + 7.0, c.y + y)], s);
                p.circle_filled(Pos2::new(c.x + knob, c.y + y), 2.4, color);
            }
        }
        Icon::Fan => {
            p.circle_stroke(c, 8.0, s);
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3.0 - 0.6;
                let tip = c + Vec2::angled(a) * 6.0;
                let mid = c + Vec2::angled(a + 0.9) * 3.4;
                p.line_segment([c, mid], s);
                p.line_segment([mid, tip], s);
            }
            p.circle_filled(c, 1.6, color);
        }
        Icon::Chip => {
            let r = Rect::from_center_size(c, Vec2::splat(11.0));
            p.rect_stroke(r, 2, s, StrokeKind::Inside);
            for d in [-3.0, 3.0] {
                p.line_segment([Pos2::new(c.x + d, r.top() - 3.0), Pos2::new(c.x + d, r.top())], s);
                p.line_segment([Pos2::new(c.x + d, r.bottom()), Pos2::new(c.x + d, r.bottom() + 3.0)], s);
                p.line_segment([Pos2::new(r.left() - 3.0, c.y + d), Pos2::new(r.left(), c.y + d)], s);
                p.line_segment([Pos2::new(r.right(), c.y + d), Pos2::new(r.right() + 3.0, c.y + d)], s);
            }
        }
    }
}

/// Item navigasi sidebar: bilah aksen di kiri + latar halus untuk yang aktif.
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());
    let t = ctx_anim(ui, resp.id, active);
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let p = ui.painter();
    p.rect_filled(rect, 8, lerp_color(Color32::TRANSPARENT, SURFACE, t.max(hover * 0.6)));
    if t > 0.01 {
        let bar = Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - 9.0 * t), Vec2::new(3.0, 18.0 * t));
        p.rect_filled(bar, 2, ACCENT);
    }
    let col = lerp_color(TEXT_FAINT, TEXT, t.max(hover * 0.7));
    paint_icon(p, Rect::from_center_size(Pos2::new(rect.left() + 22.0, rect.center().y), Vec2::splat(18.0)), icon, lerp_color(col, ACCENT, t * 0.9));
    p.text(Pos2::new(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::new(14.5, FontFamily::Proportional), col);
    resp
}
