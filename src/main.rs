#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod crypto;
mod store;

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use eframe::egui::{self, Color32, CornerRadius, FontId, RichText, Stroke};
use serde::{Deserialize, Serialize};
use store::{VaultData, VaultEntry, VaultStore};
use zeroize::{Zeroize, Zeroizing};

const APP_ID: &str = "io.github.eulcau.helm";
const CONTENT_WIDTH: f32 = 1040.0;
const ICON_BYTES: &[u8] = include_bytes!("../assets/icons/helm-512.png");
const CJK_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/SourceHanSansCN-Regular.otf");

#[derive(Deserialize, Serialize)]
struct AppSettings {
    dark_mode: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { dark_mode: true }
    }
}

fn main() -> eframe::Result {
    let icon = load_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_title("Helm")
            .with_icon(icon)
            .with_inner_size([1080.0, 780.0])
            .with_min_inner_size([760.0, 560.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Helm",
        options,
        Box::new(|cc| Ok(Box::new(VaultApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    let image = image::load_from_memory(ICON_BYTES)
        .expect("内置图标无法解码")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "source_han_sans_cn".into(),
        Arc::new(egui::FontData::from_static(CJK_FONT_BYTES)),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .expect("默认比例字体族不存在")
        .insert(0, "source_han_sans_cn".into());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .expect("默认等宽字体族不存在")
        .push("source_han_sans_cn".into());
    ctx.set_fonts(fonts);
}

#[derive(Clone, Copy)]
struct Palette {
    background: Color32,
    card: Color32,
    raised: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    accent_hover: Color32,
    danger: Color32,
    success: Color32,
}

impl Palette {
    fn for_mode(dark: bool) -> Self {
        if dark {
            Self {
                background: Color32::from_rgb(10, 14, 25),
                card: Color32::from_rgb(18, 24, 40),
                raised: Color32::from_rgb(25, 33, 52),
                border: Color32::from_rgb(43, 53, 76),
                text: Color32::from_rgb(242, 245, 252),
                muted: Color32::from_rgb(148, 160, 184),
                accent: Color32::from_rgb(111, 126, 255),
                accent_hover: Color32::from_rgb(130, 143, 255),
                danger: Color32::from_rgb(242, 104, 119),
                success: Color32::from_rgb(72, 196, 145),
            }
        } else {
            Self {
                background: Color32::from_rgb(244, 246, 251),
                card: Color32::WHITE,
                raised: Color32::from_rgb(241, 244, 249),
                border: Color32::from_rgb(220, 226, 236),
                text: Color32::from_rgb(27, 35, 52),
                muted: Color32::from_rgb(105, 116, 137),
                accent: Color32::from_rgb(82, 99, 222),
                accent_hover: Color32::from_rgb(69, 84, 202),
                danger: Color32::from_rgb(207, 67, 82),
                success: Color32::from_rgb(35, 151, 105),
            }
        }
    }
}

struct VaultApp {
    store: VaultStore,
    data: VaultData,
    master_password: String,
    derived_key: Option<Zeroizing<[u8; 32]>>,
    new_name: String,
    new_password: String,
    dark_mode: bool,
    status: Option<(String, bool)>,
    pending_delete: Option<usize>,
    icon_texture: egui::TextureHandle,
    settings_path: PathBuf,
}

impl VaultApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_fonts(&cc.egui_ctx);
        let icon = load_icon();
        let icon_image = egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        );
        let icon_texture =
            cc.egui_ctx
                .load_texture("helm-app-icon", icon_image, egui::TextureOptions::LINEAR);
        let store = VaultStore::new(vault_data_path());
        let fresh_data = || VaultData::new(crypto::random_salt().expect("系统安全随机数不可用"));
        let (data, status) = match store.load() {
            Ok(Some(data)) if data.decode_salt().is_some() => (data, None),
            Ok(Some(_)) => (
                fresh_data(),
                Some(("保险箱 KDF salt 损坏, 已创建空保险箱".into(), true)),
            ),
            Ok(None) => (fresh_data(), None),
            Err(error) => (
                fresh_data(),
                Some((format!("无法读取保险箱: {error}"), true)),
            ),
        };
        let settings_path = app_data_dir().join("settings.json");
        let dark_mode = load_settings(&settings_path).dark_mode;
        cc.egui_ctx.set_theme(if dark_mode {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        Self {
            store,
            data,
            master_password: String::new(),
            derived_key: None,
            new_name: String::new(),
            new_password: String::new(),
            dark_mode,
            status,
            pending_delete: None,
            icon_texture,
            settings_path,
        }
    }

    fn save(&mut self) {
        self.status = match self.store.save(&self.data) {
            Ok(()) => Some(("已安全保存到本机".into(), false)),
            Err(error) => Some((format!("保存失败: {error}"), true)),
        };
    }

    fn add_entry(&mut self) {
        let name = self.new_name.trim();
        if name.is_empty() || self.new_password.is_empty() || self.master_password.is_empty() {
            self.status = Some(("名称, 统一密码和待保存密码均不能为空".into(), true));
            return;
        }
        if self.data.entries.iter().any(|entry| entry.name == name) {
            self.status = Some(("名称已存在, 请使用其他名称".into(), true));
            return;
        }
        let Some(key) = &self.derived_key else {
            self.status = Some(("请先输入统一密码".into(), true));
            return;
        };
        match crypto::encrypt(key, self.new_password.as_bytes()) {
            Ok((nonce, ciphertext)) => {
                self.data
                    .entries
                    .push(VaultEntry::from_bytes(name.to_owned(), nonce, ciphertext));
                self.new_name.clear();
                self.new_password.zeroize();
                self.pending_delete = None;
                self.save();
            }
            Err(_) => self.status = Some(("无法取得安全随机数或初始化加密参数".into(), true)),
        }
    }

    fn value_for(&self, entry: &VaultEntry) -> String {
        if self.master_password.is_empty() {
            return entry.ciphertext.clone();
        }
        let Some(key) = &self.derived_key else {
            return entry.ciphertext.clone();
        };
        let Some((nonce, ciphertext)) = entry.decode() else {
            return "[损坏的加密数据]".into();
        };
        let bytes = crypto::decrypt(key, &nonce, &ciphertext);
        match std::str::from_utf8(&bytes) {
            Ok(value) => value.to_owned(),
            Err(_) => format!("[非 UTF-8] {}", STANDARD.encode(&*bytes)),
        }
    }

    fn refresh_key(&mut self) {
        self.derived_key = if self.master_password.is_empty() {
            None
        } else {
            self.data
                .decode_salt()
                .and_then(|salt| crypto::derive_key(&self.master_password, &salt).ok())
        };
    }

    fn card(palette: Palette) -> egui::Frame {
        egui::Frame::new()
            .fill(palette.card)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(CornerRadius::same(18))
            .inner_margin(22)
    }

    fn paint_theme_icon(ui: &egui::Ui, response: &egui::Response, dark_mode: bool, color: Color32) {
        let painter = ui.painter();
        let center = egui::pos2(response.rect.left() + 19.0, response.rect.center().y);
        if dark_mode {
            painter.circle_filled(center, 6.0, color);
            painter.circle_filled(
                center + egui::vec2(3.0, -2.0),
                5.0,
                ui.style().interact(response).bg_fill,
            );
        } else {
            painter.circle_stroke(center, 4.0, Stroke::new(1.5, color));
            for index in 0..8 {
                let angle = index as f32 * std::f32::consts::TAU / 8.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + direction * 6.0, center + direction * 8.0],
                    Stroke::new(1.5, color),
                );
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, palette: Palette) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&self.icon_texture).fit_to_exact_size(egui::vec2(54.0, 54.0)));
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("HELM")
                        .size(11.0)
                        .strong()
                        .color(palette.accent),
                );
                ui.label(
                    RichText::new("密码保险箱")
                        .size(26.0)
                        .strong()
                        .color(palette.text),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.dark_mode {
                    "深色模式"
                } else {
                    "浅色模式"
                };
                let response = ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("    {label}")).color(palette.text),
                        )
                        .fill(palette.card)
                        .stroke(Stroke::new(1.0, palette.border))
                        .corner_radius(CornerRadius::same(12))
                        .min_size(egui::vec2(120.0, 42.0)),
                    )
                    .on_hover_text("切换界面主题");
                Self::paint_theme_icon(ui, &response, self.dark_mode, palette.text);
                if response.clicked() {
                    self.dark_mode = !self.dark_mode;
                    if let Err(error) = save_settings(&self.settings_path, self.dark_mode) {
                        self.status = Some((format!("无法保存色彩模式: {error}"), true));
                    }
                }
            });
        });
    }

    fn master_card(&mut self, ui: &mut egui::Ui, palette: Palette) {
        Self::card(palette).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("统一密码")
                            .size(19.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.label(
                        RichText::new("只保留在当前进程内存中, 输入后立即更新所有结果")
                            .size(13.0)
                            .color(palette.muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (label, color) = if self.master_password.is_empty() {
                        ("密文模式", palette.muted)
                    } else {
                        ("实时解密", palette.success)
                    };
                    egui::Frame::new()
                        .fill(palette.raised)
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(11, 7))
                        .show(ui, |ui| {
                            ui.label(RichText::new(label).size(12.0).strong().color(color));
                        });
                });
            });
            ui.add_space(15.0);

            let mut asterisk_layouter =
                |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                    let masked = text.as_str().replace('•', "*");
                    ui.fonts_mut(|fonts| {
                        fonts.layout(
                            masked,
                            FontId::proportional(16.0),
                            ui.visuals().text_color(),
                            wrap_width,
                        )
                    })
                };
            let available = ui.available_width();
            let mut changed = false;
            ui.horizontal(|ui| {
                let input_width = if self.master_password.is_empty() {
                    available
                } else {
                    available - 82.0
                };
                let response = ui.add_sized(
                    [input_width, 46.0],
                    egui::TextEdit::singleline(&mut self.master_password)
                        .password(true)
                        .layouter(&mut asterisk_layouter)
                        .background_color(palette.raised)
                        .margin(12)
                        .hint_text("输入统一密码"),
                );
                changed |= response.changed();
                response.on_hover_text("输入内容不会写入磁盘");
                if !self.master_password.is_empty()
                    && ui
                        .add(
                            egui::Button::new(RichText::new("清空").color(palette.muted))
                                .fill(palette.raised)
                                .corner_radius(CornerRadius::same(11))
                                .min_size(egui::vec2(70.0, 46.0)),
                        )
                        .clicked()
                {
                    self.master_password.zeroize();
                    changed = true;
                }
            });
            if changed {
                self.refresh_key();
                self.pending_delete = None;
            }
        });
    }

    fn add_card(&mut self, ui: &mut egui::Ui, palette: Palette) {
        Self::card(palette).show(ui, |ui| {
            ui.label(
                RichText::new("添加密码")
                    .size(19.0)
                    .strong()
                    .color(palette.text),
            );
            ui.label(
                RichText::new("填写名称和密码, 使用当前统一密码加密保存")
                    .size(13.0)
                    .color(palette.muted),
            );
            ui.add_space(15.0);

            let width = ui.available_width();
            let mut submit_by_enter = false;
            ui.horizontal(|ui| {
                ui.add_sized(
                    [width * 0.34, 44.0],
                    egui::TextEdit::singleline(&mut self.new_name)
                        .background_color(palette.raised)
                        .margin(11)
                        .hint_text("名称, 如 GitHub"),
                );
                let response = ui.add_sized(
                    [width * 0.66 - 10.0, 44.0],
                    egui::TextEdit::singleline(&mut self.new_password)
                        .password(true)
                        .background_color(palette.raised)
                        .margin(11)
                        .hint_text("要保存的密码"),
                );
                submit_by_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let enabled = !self.master_password.is_empty()
                    && !self.new_name.trim().is_empty()
                    && !self.new_password.is_empty();
                let button =
                    egui::Button::new(RichText::new("加密并保存").strong().color(Color32::WHITE))
                        .fill(if enabled {
                            palette.accent
                        } else {
                            palette.muted
                        })
                        .corner_radius(CornerRadius::same(11))
                        .min_size(egui::vec2(132.0, 42.0));
                if ui.add_enabled(enabled, button).clicked() || (enabled && submit_by_enter) {
                    self.add_entry();
                }
            });
        });
    }

    fn status_card(&self, ui: &mut egui::Ui, palette: Palette) {
        let Some((message, is_error)) = &self.status else {
            return;
        };
        let color = if *is_error {
            palette.danger
        } else {
            palette.success
        };
        egui::Frame::new()
            .fill(palette.card)
            .stroke(Stroke::new(1.0, color))
            .corner_radius(CornerRadius::same(13))
            .inner_margin(egui::Margin::symmetric(15, 11))
            .show(ui, |ui| {
                ui.label(RichText::new(message).size(13.0).color(color));
            });
    }

    fn entries_section(&mut self, ui: &mut egui::Ui, palette: Palette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("已保存密码")
                    .size(21.0)
                    .strong()
                    .color(palette.text),
            );
            egui::Frame::new()
                .fill(palette.raised)
                .corner_radius(CornerRadius::same(9))
                .inner_margin(egui::Margin::symmetric(9, 5))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(self.data.entries.len().to_string())
                            .size(12.0)
                            .strong()
                            .color(palette.muted),
                    );
                });
        });
        ui.add_space(9.0);

        if self.data.entries.is_empty() {
            Self::card(palette).show(ui, |ui| {
                ui.add_space(22.0);
                ui.vertical_centered(|ui| {
                    egui::Frame::new()
                        .fill(palette.raised)
                        .corner_radius(CornerRadius::same(16))
                        .inner_margin(15)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("＋")
                                    .size(24.0)
                                    .strong()
                                    .color(palette.accent),
                            );
                        });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("保险箱还是空的")
                            .size(17.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.label(
                        RichText::new("输入统一密码, 然后添加第一条记录")
                            .size(13.0)
                            .color(palette.muted),
                    );
                });
                ui.add_space(22.0);
            });
            return;
        }

        let rows: Vec<_> = self
            .data
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (index, entry.name.clone(), self.value_for(entry)))
            .collect();
        let mut confirmed_delete = None;

        for (index, name, value) in rows {
            Self::card(palette).inner_margin(18).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let initial = name
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string();
                    egui::Frame::new()
                        .fill(palette.raised)
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(11)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(initial)
                                    .size(16.0)
                                    .strong()
                                    .color(palette.accent),
                            );
                        });
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&name).size(16.0).strong().color(palette.text));
                        ui.label(
                            RichText::new(if self.master_password.is_empty() {
                                "当前显示加密结果"
                            } else {
                                "当前显示解密结果"
                            })
                            .size(12.0)
                            .color(palette.muted),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.pending_delete == Some(index) {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("确认删除").strong().color(Color32::WHITE),
                                    )
                                    .fill(palette.danger)
                                    .corner_radius(CornerRadius::same(10)),
                                )
                                .clicked()
                            {
                                confirmed_delete = Some(index);
                            }
                            if ui.button("取消").clicked() {
                                self.pending_delete = None;
                            }
                        } else {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("删除").color(palette.danger))
                                        .fill(palette.raised)
                                        .corner_radius(CornerRadius::same(10)),
                                )
                                .on_hover_text("删除此条记录")
                                .clicked()
                            {
                                self.pending_delete = Some(index);
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("复制").strong().color(Color32::WHITE),
                                    )
                                    .fill(palette.accent)
                                    .corner_radius(CornerRadius::same(10)),
                                )
                                .on_hover_text("复制当前显示的结果")
                                .clicked()
                            {
                                ui.ctx().copy_text(value.clone());
                                self.status = Some((format!("已复制 {name}"), false));
                            }
                        }
                    });
                });
                ui.add_space(11.0);
                egui::Frame::new()
                    .fill(palette.raised)
                    .corner_radius(CornerRadius::same(11))
                    .inner_margin(egui::Margin::symmetric(14, 11))
                    .show(ui, |ui| {
                        let response = ui.add_sized(
                            [ui.available_width(), 22.0],
                            egui::Label::new(
                                RichText::new(&value)
                                    .monospace()
                                    .size(13.0)
                                    .color(palette.text),
                            )
                            .truncate(),
                        );
                        response.on_hover_text(&value);
                    });
            });
            ui.add_space(10.0);
        }

        if let Some(index) = confirmed_delete {
            self.data.entries.remove(index);
            self.pending_delete = None;
            self.save();
        }
    }

    fn apply_style(ctx: &egui::Context, dark: bool, palette: Palette) {
        let theme = if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        ctx.set_theme(theme);
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.button_padding = egui::vec2(13.0, 9.0);
            style.visuals.panel_fill = palette.background;
            style.visuals.window_fill = palette.card;
            style.visuals.extreme_bg_color = palette.raised;
            style.visuals.selection.bg_fill = palette.accent;
            style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
            style.visuals.window_corner_radius = CornerRadius::same(16);
            style.visuals.menu_corner_radius = CornerRadius::same(12);

            style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
            style.visuals.widgets.noninteractive.bg_fill = palette.card;
            style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
            style.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);

            style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
            style.visuals.widgets.inactive.bg_fill = palette.raised;
            style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
            style.visuals.widgets.inactive.corner_radius = CornerRadius::same(10);

            style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
            style.visuals.widgets.hovered.bg_fill = palette.accent_hover;
            style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent_hover);
            style.visuals.widgets.hovered.corner_radius = CornerRadius::same(10);

            style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
            style.visuals.widgets.active.bg_fill = palette.accent;
            style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
            style.visuals.widgets.active.corner_radius = CornerRadius::same(10);
        });
    }
}

fn app_data_dir() -> PathBuf {
    #[cfg(windows)]
    let base = env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    base.unwrap_or_else(|| PathBuf::from(".")).join(APP_ID)
}

fn vault_data_path() -> PathBuf {
    app_data_dir().join("vault.json")
}

fn load_settings(path: &Path) -> AppSettings {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_settings(path: &Path, dark_mode: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let settings = AppSettings { dark_mode };
    let bytes = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
    fs::write(path, bytes)
}

impl eframe::App for VaultApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let palette = Palette::for_mode(self.dark_mode);
        Self::apply_style(ui.ctx(), self.dark_mode, palette);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.background).inner_margin(0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width().min(CONTENT_WIDTH);
                        let side_margin = ((ui.available_width() - width) * 0.5).max(20.0);
                        ui.horizontal(|ui| {
                            ui.add_space(side_margin);
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - side_margin).min(width));
                                ui.add_space(24.0);
                                self.header(ui, palette);
                                ui.add_space(24.0);
                                self.master_card(ui, palette);
                                ui.add_space(14.0);
                                self.add_card(ui, palette);
                                if self.status.is_some() {
                                    ui.add_space(12.0);
                                    self.status_card(ui, palette);
                                }
                                ui.add_space(25.0);
                                self.entries_section(ui, palette);
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new(
                                        "Helm 不验证统一密码. 错误密码会产生错误结果, 请自行辨认.",
                                    )
                                    .size(12.0)
                                    .color(palette.muted),
                                );
                                ui.add_space(28.0);
                            });
                        });
                    });
            });
    }
}

impl Drop for VaultApp {
    fn drop(&mut self) {
        self.master_password.zeroize();
        self.new_password.zeroize();
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn persists_color_mode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");

        save_settings(&path, false).unwrap();

        assert!(!load_settings(&path).dark_mode);
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "{\n  \"dark_mode\": false\n}"
        );
    }
}
