//! The front door: when the data is password-protected, the app opens on a
//! sign-in screen and shows nothing else until the password is entered.

use super::*;
use crate::store;

pub struct Gate {
    app: Option<Box<DropInApp>>,
    password: String,
    error: Option<String>,
    /// "Forgot the password?" is open, and what's been typed to confirm.
    forgot: bool,
    confirm: String,
    themed: bool,
    frame: u64,
}

impl Gate {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_fonts(theme::fonts());
        widgets::load_skill_finishes(&cc.egui_ctx);
        let app = (!store::is_locked()).then(|| Box::new(DropInApp::new(&cc.egui_ctx)));
        Self { app, password: String::new(), error: None, forgot: false, confirm: String::new(), themed: false, frame: 0 }
    }

    fn unlock(&mut self, ctx: &egui::Context) {
        if self.password.is_empty() {
            return;
        }
        match Store::unlock(&self.password) {
            Ok(store) => {
                self.password.clear();
                self.app = Some(Box::new(DropInApp::with_store(ctx, Ok(store))));
            }
            Err(store::UnlockError::WrongPassword) => {
                self.error = Some("That's not the password. Check Caps Lock, and try again.".into());
                self.password.clear();
            }
            Err(store::UnlockError::Other(err)) => self.error = Some(format!("The data couldn't be opened: {err}")),
        }
    }

    /// The password is lost: set the locked data aside and open an empty app.
    fn start_over(&mut self, ctx: &egui::Context) {
        let dir = crate::model::data_dir().join("backups");
        let _ = std::fs::create_dir_all(&dir);
        let aside = dir.join(format!("locked-{}.db.enc", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        if let Err(err) = std::fs::rename(store::vault_path(), &aside) {
            self.error = Some(format!("Couldn't set the locked data aside: {err}"));
            return;
        }
        let mut app = DropInApp::new(ctx);
        app.toast(ctx, "Started over. The locked data was kept in the backups folder, in case the password turns up.");
        self.app = Some(Box::new(app));
    }

    fn sign_in(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let pal = Palette::LIGHT;
        if !self.themed {
            theme::apply(&ctx, &pal);
            self.themed = true;
        }
        let mut open = false;
        let mut start_over = false;
        egui::CentralPanel::default().frame(Frame::NONE.fill(pal.canvas)).show(ui, |ui| {
            let card_w = 420.0;
            let top = (ui.available_height() * 0.5 - if self.forgot { 300.0 } else { 200.0 }).max(24.0);
            ui.add_space(top);
            ui.vertical_centered(|ui| {
                Frame::NONE
                    .fill(pal.bg1)
                    .stroke(Stroke::new(1.0, pal.card_stroke))
                    .corner_radius(12)
                    .shadow(pal.shadow(16))
                    .inner_margin(Margin::same(32))
                    .show(ui, |ui| {
                        ui.set_width(card_w);
                        ui.vertical_centered(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
                            ui.painter().image(widgets::app_icon(&ctx), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            ui.add_space(10.0);
                            ui.label(RichText::new("Drop-in Badminton").font(semibold(24.0)).color(pal.fg1));
                            ui.label(RichText::new("Enter the password to open the app.").font(regular(15.0)).color(pal.fg2));
                        });
                        ui.add_space(18.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.password)
                                .password(true)
                                .hint_text("Password")
                                .font(regular(18.0))
                                .margin(Margin::symmetric(12, 9))
                                .desired_width(f32::INFINITY),
                        );
                        if self.frame < 3 || self.error.is_some() && !field.has_focus() && !self.forgot {
                            field.request_focus();
                        }
                        if field.changed() {
                            self.error = None;
                        }
                        open |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if let Some(err) = &self.error {
                            ui.add_space(6.0);
                            ui.label(RichText::new(err).font(regular(14.0)).color(pal.danger_fg));
                        }
                        ui.add_space(12.0);
                        ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                            open |= widgets::button(ui, &pal, ButtonKind::Primary, Some(icon::LOCK_OPEN), "Open", 40.0).clicked();
                        });
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            let link = ui.add(egui::Label::new(RichText::new("Forgot the password?").font(regular(14.0)).color(pal.brand_fg)).sense(Sense::click()));
                            if link.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                self.forgot = !self.forgot;
                                self.confirm.clear();
                            }
                        });
                        if self.forgot {
                            ui.add_space(12.0);
                            Frame::NONE
                                .fill(pal.danger_subtle)
                                .corner_radius(8)
                                .inner_margin(Margin::same(14))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new(
                                            "The data is encrypted with the password, so without it nobody can open it. \
                                             There's no reset and no way around it.",
                                        )
                                        .font(regular(14.0))
                                        .color(pal.fg1),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "If it's truly lost, you can start over with an empty app. The locked data is kept \
                                             in the backups folder, in case the password turns up later.",
                                        )
                                        .font(regular(14.0))
                                        .color(pal.fg2),
                                    );
                                    ui.add_space(10.0);
                                    ui.label(RichText::new("Type START OVER to confirm").font(semibold(13.0)).color(pal.fg2));
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.confirm)
                                            .font(regular(15.0))
                                            .margin(Margin::symmetric(10, 7))
                                            .desired_width(f32::INFINITY),
                                    );
                                    ui.add_space(8.0);
                                    let ready = self.confirm.trim() == "START OVER";
                                    ui.add_enabled_ui(ready, |ui| {
                                        start_over = widgets::button(ui, &pal, ButtonKind::Danger, None, "Start over with no data", 34.0).clicked();
                                    });
                                });
                        }
                    });
            });
        });
        if open {
            self.unlock(&ctx);
        }
        if start_over {
            self.start_over(&ctx);
        }
    }
}

impl eframe::App for Gate {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.frame += 1;
        if let Some(app) = &mut self.app {
            app.ui(ui, frame);
            return;
        }
        self.sign_in(ui);
        // Debug builds: a screenshot of the sign-in screen.
        #[cfg(debug_assertions)]
        if let Ok(path) = std::env::var("DROP_IN_BADMINTON_SCREENSHOT") {
            if self.frame == 3 && std::env::var("DROP_IN_BADMINTON_SHOW").is_ok_and(|s| s.contains("forgot")) {
                self.forgot = true;
            }
            if self.frame == 20 {
                capture_screenshot(ui.ctx(), path);
            }
            ui.ctx().request_repaint();
        }
    }

    fn on_exit(&mut self) {
        if let Some(app) = &mut self.app {
            app.on_exit();
        }
    }

    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(app) = &mut self.app {
            app.raw_input_hook(ctx, raw);
        }
    }
}
