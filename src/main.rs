mod crypto;
mod store;

use std::{env, path::PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use store::{VaultData, VaultEntry, VaultStore};
use zeroize::{Zeroize, Zeroizing};

const APP_ID: &str = "io.github.cipher-vault";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_title("Cipher Vault")
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([760.0, 520.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Cipher Vault",
        options,
        Box::new(|cc| Ok(Box::new(VaultApp::new(cc)))),
    )
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
}

impl VaultApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let path = vault_data_path();
        let store = VaultStore::new(path);
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
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        Self {
            store,
            data,
            master_password: String::new(),
            derived_key: None,
            new_name: String::new(),
            new_password: String::new(),
            dark_mode: true,
            status,
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
            self.status = Some(("名称、统一密码和待保存密码均不能为空".into(), true));
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

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("CIPHER VAULT")
                        .size(12.0)
                        .strong()
                        .color(Color32::from_rgb(109, 124, 255)),
                );
                ui.heading(RichText::new("密码保险箱").size(28.0));
                ui.label(RichText::new("统一密码只存在于当前进程内存中").weak());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.dark_mode {
                    "☾  深色"
                } else {
                    "☀  浅色"
                };
                if ui
                    .button(label)
                    .on_hover_text("切换浅色/深色模式")
                    .clicked()
                {
                    self.dark_mode = !self.dark_mode;
                    ui.ctx().set_theme(if self.dark_mode {
                        egui::Theme::Dark
                    } else {
                        egui::Theme::Light
                    });
                }
            });
        });
    }

    fn master_input(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style())
            .inner_margin(18)
            .show(ui, |ui| {
                ui.label(RichText::new("统一密码").strong());
                ui.add_space(6.0);
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
                let response = ui.add_sized(
                    [ui.available_width(), 42.0],
                    egui::TextEdit::singleline(&mut self.master_password)
                        .password(true)
                        .layouter(&mut asterisk_layouter)
                        .hint_text("输入后实时解密; 清空则显示密文"),
                );
                let changed = response.changed();
                response.on_hover_text("输入内容不会写入磁盘");
                if changed {
                    self.derived_key = if self.master_password.is_empty() {
                        None
                    } else {
                        self.data
                            .decode_salt()
                            .and_then(|salt| crypto::derive_key(&self.master_password, &salt).ok())
                    };
                }
            });
    }

    fn add_form(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("＋ 添加密码").strong())
            .default_open(self.data.entries.is_empty())
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [220.0, 36.0],
                        egui::TextEdit::singleline(&mut self.new_name).hint_text("名称, 如 GitHub"),
                    );
                    ui.add_sized(
                        [ui.available_width() - 112.0, 36.0],
                        egui::TextEdit::singleline(&mut self.new_password)
                            .password(true)
                            .hint_text("要保存的密码"),
                    );
                    if ui
                        .add_sized([96.0, 36.0], egui::Button::new("加密保存"))
                        .clicked()
                    {
                        self.add_entry();
                    }
                });
            });
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("已保存密码").size(20.0));
            ui.label(RichText::new(format!("{} 项", self.data.entries.len())).weak());
        });
        ui.add_space(8.0);

        let mut delete_index = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::Grid::new("vault-table")
                .num_columns(4)
                .striped(true)
                .spacing([18.0, 14.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("名称").strong());
                    ui.label(
                        RichText::new(if self.master_password.is_empty() {
                            "加密结果"
                        } else {
                            "解密结果"
                        })
                        .strong(),
                    );
                    ui.label(RichText::new("复制").strong());
                    ui.label(RichText::new("管理").strong());
                    ui.end_row();
                    for (index, entry) in self.data.entries.iter().enumerate() {
                        ui.label(RichText::new(&entry.name).strong());
                        let value = self.value_for(entry);
                        ui.add_sized(
                            [ui.available_width().max(300.0), 28.0],
                            egui::Label::new(RichText::new(&value).monospace()).truncate(),
                        );
                        if ui
                            .button("复制")
                            .on_hover_text("复制当前显示的结果")
                            .clicked()
                        {
                            ui.ctx().copy_text(value);
                        }
                        if ui.button("删除").on_hover_text("永久删除此条目").clicked() {
                            delete_index = Some(index);
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some(index) = delete_index {
            self.data.entries.remove(index);
            self.save();
        }
        if self.data.entries.is_empty() {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("保险箱还是空的").size(18.0).strong());
                ui.label(RichText::new("输入统一密码, 然后添加第一条密码").weak());
            });
        }
    }
}

fn vault_data_path() -> PathBuf {
    #[cfg(windows)]
    let base = env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    base.unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
        .join("vault.json")
}

impl eframe::App for VaultApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let visuals = if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        ctx.set_visuals(visuals);
        ui.style_mut().spacing.item_spacing = egui::vec2(10.0, 10.0);
        ctx.style_mut_of(
            if self.dark_mode {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            },
            |style| {
                style.spacing.item_spacing = egui::vec2(10.0, 10.0);
                style.visuals.widgets.active.bg_stroke =
                    Stroke::new(1.0, Color32::from_rgb(109, 124, 255));
                style.visuals.selection.bg_fill = Color32::from_rgb(82, 98, 230);
            },
        );
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_max_width(1100.0);
                self.header(ui);
                ui.add_space(18.0);
                self.master_input(ui);
                ui.add_space(12.0);
                self.add_form(ui);
                if let Some((message, is_error)) = &self.status {
                    let color = if *is_error {
                        Color32::from_rgb(225, 82, 82)
                    } else {
                        Color32::from_rgb(62, 174, 126)
                    };
                    ui.label(RichText::new(message).color(color));
                }
                ui.add_space(20.0);
                self.table(ui);
                ui.add_space(16.0);
                ui.label(
                    RichText::new(
                        "提示: 本设计不验证统一密码. 错误密码会产生错误结果, 请自行辨认.",
                    )
                    .weak()
                    .size(12.0),
                );
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
