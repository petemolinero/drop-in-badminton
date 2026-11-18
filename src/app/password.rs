//! Settings → Password: protect the app (and encrypt its data) with a
//! password, change it, or remove it. There's no recovery, so setting one
//! comes with a plain warning, shown in full each time.

use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Set,
    Change,
    Remove,
}

pub(super) struct PasswordDialog {
    mode: Mode,
    current: String,
    new: String,
    again: String,
    understood: bool,
    error: Option<String>,
}

const MIN_LEN: usize = 6;

impl PasswordDialog {
    pub fn new(mode: Mode) -> Self {
        Self { mode, current: String::new(), new: String::new(), again: String::new(), understood: false, error: None }
    }
}

impl PasswordDialog {
    #[cfg(debug_assertions)]
    pub fn debug_fill(&mut self, new: &str, again: &str) {
        self.new = new.into();
        self.again = again.into();
    }
}

impl DropInApp {
    /// The Password row in Settings.
    pub(super) fn password_setting(&mut self, ui: &mut Ui, pal: &Palette) {
        let on = self.store.has_password();
        ui.horizontal(|ui| {
            ui.set_min_height(46.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add_space(6.0);
                ui.label(RichText::new("Password").font(regular(15.0)).color(pal.fg1));
                let note = if on {
                    "On: asked for when the app opens. The data is encrypted."
                } else {
                    "Off: anyone who opens the app can see and change everything."
                };
                ui.label(RichText::new(note).font(regular(13.0)).color(pal.fg3));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if on {
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, "Remove…", 32.0).clicked() {
                        self.password_dialog = Some(PasswordDialog::new(Mode::Remove));
                    }
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, "Change…", 32.0).clicked() {
                        self.password_dialog = Some(PasswordDialog::new(Mode::Change));
                    }
                } else if widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::LOCK), "Set a password…", 32.0).clicked() {
                    self.password_dialog = Some(PasswordDialog::new(Mode::Set));
                }
            });
        });
        if self.password_dialog.is_some() {
            self.settings_open = false;
        }
    }

    pub(super) fn password_dialog_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut d) = self.password_dialog.take() else { return };
        let (mut go, mut cancel) = (false, false);
        let title = match d.mode {
            Mode::Set => "Set a password",
            Mode::Change => "Change the password",
            Mode::Remove => "Remove the password",
        };
        let field = |ui: &mut Ui, label: &str| {
            ui.add_space(10.0);
            ui.label(RichText::new(label).font(semibold(14.0)).color(pal.fg2));
            ui.add_space(2.0);
        };
        fn text_box<'a>(text: &'a mut String, hidden: bool, hint: &str) -> egui::TextEdit<'a> {
            egui::TextEdit::singleline(text)
                .password(hidden)
                .hint_text(hint.to_owned())
                .font(regular(17.0))
                .margin(Margin::symmetric(10, 8))
                .desired_width(f32::INFINITY)
        }
        let modal = Modal::new(unique("password-dialog")).frame(Self::dialog_frame(pal)).backdrop_color(pal.scrim).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::LOCK).font(icon_font(22.0)).color(pal.brand_fg));
                ui.label(RichText::new(title).font(semibold(20.0)));
            });
            if d.mode != Mode::Remove {
                // The warning, every time: there is no way back from a lost password.
                ui.add_space(8.0);
                Frame::NONE
                    .fill(pal.warning_subtle)
                    .stroke(Stroke::new(1.0, pal.warning_fg.gamma_multiply(0.5)))
                    .corner_radius(8)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon::WARNING_F).font(icon_font(20.0)).color(pal.warning_fg));
                            ui.label(RichText::new("Don't lose this password").font(semibold(16.0)).color(pal.fg1));
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "All of the app's data (every player, session and game) will be encrypted with it. \
                                 There's no reset and no recovery: if the password is forgotten, the data is lost for \
                                 good, and nobody can get it back.",
                            )
                            .font(regular(14.0))
                            .color(pal.fg1),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new("Write it down and keep it somewhere safe.").font(semibold(14.0)).color(pal.fg1));
                    });
            } else {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("The data will be saved unencrypted, and the app will open without asking for a password.")
                        .font(regular(14.0))
                        .color(pal.fg2),
                );
            }
            if d.mode != Mode::Set {
                field(ui, "Current password");
                let r = ui.add(text_box(&mut d.current, true, ""));
                if r.changed() {
                    d.error = None;
                }
            }
            if d.mode != Mode::Remove {
                // Shown as typed, so a typo can't slip through unseen.
                field(ui, "New password");
                ui.add(text_box(&mut d.new, false, &format!("At least {MIN_LEN} characters")));
                field(ui, "Type it again");
                ui.add(text_box(&mut d.again, false, ""));
                let (glyph, text, color) = if d.new.is_empty() && d.again.is_empty() {
                    ("", String::new(), pal.fg3)
                } else if d.new.chars().count() < MIN_LEN {
                    (icon::INFO, format!("At least {MIN_LEN} characters"), pal.fg3)
                } else if d.new != d.again {
                    (icon::CLOSE, "The two don't match yet".to_owned(), pal.danger_fg)
                } else {
                    (icon::CHECK, "They match".to_owned(), pal.success_fg)
                };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(glyph).font(icon_font(15.0)).color(color));
                    ui.label(RichText::new(text).font(regular(13.0)).color(color));
                });
                ui.add_space(8.0);
                ui.checkbox(
                    &mut d.understood,
                    RichText::new("I understand: if this password is lost, all the data is lost.").font(regular(14.0)).color(pal.fg1),
                );
            }
            if let Some(err) = &d.error {
                ui.add_space(6.0);
                ui.label(RichText::new(err).font(regular(14.0)).color(pal.danger_fg));
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ready = match d.mode {
                    Mode::Remove => !d.current.is_empty(),
                    _ => d.new.chars().count() >= MIN_LEN && d.new == d.again && d.understood && (d.mode == Mode::Set || !d.current.is_empty()),
                };
                let (kind, label) = match d.mode {
                    Mode::Set => (ButtonKind::Primary, "Set password"),
                    Mode::Change => (ButtonKind::Primary, "Change password"),
                    Mode::Remove => (ButtonKind::Danger, "Remove password"),
                };
                go = ui.add_enabled_ui(ready, |ui| widgets::button(ui, pal, kind, None, label, 34.0)).inner.clicked();
                cancel |= widgets::button(ui, pal, ButtonKind::Secondary, None, "Cancel", 34.0).clicked();
            });
        });
        if go {
            if d.mode != Mode::Set && !self.store.password_matches(&d.current) {
                d.error = Some("The current password isn't right.".into());
                d.current.clear();
                self.password_dialog = Some(d);
                return;
            }
            // Everything on screen goes into the file being protected.
            self.save();
            self.dirty = false;
            let result = match d.mode {
                Mode::Remove => self.store.remove_password(),
                _ => self.store.set_password(&d.new),
            };
            let text = match (&result, d.mode) {
                (Err(err), _) => format!("Couldn't change the password: {err}"),
                (Ok(()), Mode::Set) => "Password set. The data is now encrypted, and the app will ask for it when it opens.".into(),
                (Ok(()), Mode::Change) => "Password changed".into(),
                (Ok(()), Mode::Remove) => "Password removed. The app will open without asking.".into(),
            };
            self.toast(ctx, text);
        } else if !cancel && !modal.should_close() {
            self.password_dialog = Some(d);
        }
    }
}
