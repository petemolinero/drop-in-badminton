//! Playing in groups: players linked to always play together (across
//! sessions), and tonight's practice group, who only play each other.

use std::collections::BTreeSet;

use super::*;

/// "Who does … play with?": picking the people linked with a player.
pub(super) struct LinkDialog {
    pub id: PlayerId,
    chosen: Vec<PlayerId>,
    search: String,
}

/// Choosing tonight's practice group.
pub(super) struct PracticeDialog {
    chosen: BTreeSet<PlayerId>,
    search: String,
}

/// The practice group's colour (the second series colour, after brand blue).
pub(super) fn practice_color(pal: &Palette) -> Color32 {
    if pal.dark { theme::hex(0xD95926) } else { theme::hex(0xEB6834) }
}

/// "Ann", "Ann and Bo", "Ann, Bo and Cy".
pub(super) fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl DropInApp {
    pub(super) fn open_link_dialog(&mut self, id: PlayerId) {
        let chosen = self.state.linked(id);
        self.link_dialog = Some(LinkDialog { id, chosen, search: String::new() });
    }

    pub(super) fn open_practice_dialog(&mut self) {
        let chosen = self.state.session.practice.iter().copied().filter(|id| self.state.session.is_here(*id)).collect();
        self.practice_dialog = Some(PracticeDialog { chosen, search: String::new() });
    }

    /// Names of the people linked with `id`, for labels.
    pub(super) fn partner_names(&self, id: PlayerId) -> Vec<String> {
        let mut names: Vec<String> = self.state.linked(id).iter().map(|p| self.state.name(*p).to_owned()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// Small markers after a name: linked (with someone here tonight) and
    /// practising. Returns the width used.
    pub(super) fn paint_group_marks(&self, painter: &egui::Painter, pal: &Palette, left_center: Pos2, id: PlayerId, size: f32, alpha: f32) -> f32 {
        let mut x = left_center.x;
        let here = |p: &PlayerId| self.state.session.is_here(*p);
        if self.state.linked(id).iter().any(here) {
            let r = painter.text(pos2(x, left_center.y + 1.0), Align2::LEFT_CENTER, icon::LINK, icon_font(size), pal.brand_fg.gamma_multiply(alpha));
            x = r.right() + 4.0;
        }
        if self.state.in_practice(id) {
            let r = painter.text(pos2(x, left_center.y + 1.0), Align2::LEFT_CENTER, icon::TROPHY_F, icon_font(size), practice_color(pal).gamma_multiply(alpha));
            x = r.right() + 4.0;
        }
        x - left_center.x
    }

    /// Width `paint_group_marks` will take.
    pub(super) fn group_marks_width(&self, id: PlayerId, size: f32) -> f32 {
        let linked = self.state.linked(id).iter().any(|p| self.state.session.is_here(*p));
        let n = linked as u32 + self.state.in_practice(id) as u32;
        n as f32 * (size + 4.0)
    }

    /// A checklist row: tick box, skill badge, name, and a note on the right.
    fn pick_row(ui: &mut Ui, pal: &Palette, on: bool, enabled: bool, skill: Skill, name: &str, note: &str) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), if enabled { Sense::click() } else { Sense::hover() });
        let resp = if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp };
        let p = ui.painter();
        if resp.hovered() && enabled {
            p.rect_filled(rect, CornerRadius::same(4), pal.subtle_hover);
        }
        let ink = if enabled { pal.fg1 } else { pal.fg3 };
        let cy = rect.center().y;
        let boxr = Rect::from_center_size(pos2(rect.left() + 18.0, cy), Vec2::splat(18.0));
        if on {
            p.rect_filled(boxr, CornerRadius::same(4), pal.brand);
            p.text(boxr.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, icon::CHECK, icon_font(14.0), pal.on_brand);
        } else {
            p.rect_stroke(boxr, CornerRadius::same(4), Stroke::new(1.5, if enabled { pal.stroke_accessible } else { pal.stroke2 }), StrokeKind::Inside);
        }
        let badge = Rect::from_min_size(pos2(boxr.right() + 12.0, cy - 10.0), vec2(widgets::badge_width(20.0), 20.0));
        widgets::paint_skill_badge(p, pal, badge, skill);
        let note_g = p.layout_no_wrap(note.to_owned(), regular(13.0), pal.fg3);
        let name_w = rect.right() - 10.0 - note_g.size().x - 12.0 - (badge.right() + 10.0);
        let name_g = widgets::elided(p, name, semibold(15.0), ink, name_w.max(40.0));
        p.galley(pos2(badge.right() + 10.0, cy - name_g.size().y / 2.0), name_g, ink);
        p.galley(pos2(rect.right() - 10.0 - note_g.size().x, cy - note_g.size().y / 2.0), note_g, pal.fg3);
        enabled && resp.clicked()
    }

    fn search_box(ui: &mut Ui, text: &mut String, hint: &str) -> egui::Response {
        ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(format!("{}  {hint}", icon::SEARCH))
                .font(regular(15.0))
                .margin(Margin::symmetric(10, 7))
                .desired_width(f32::INFINITY),
        )
    }

    pub(super) fn link_dialog_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut d) = self.link_dialog.take() else { return };
        let Some(name) = self.state.players.get(&d.id).map(|p| p.name.clone()) else { return };
        let others = AppState::MAX_LINKED - 1;
        let (mut save, mut cancel, mut unlink) = (false, false, false);
        let modal = Modal::new(unique("link-dialog")).frame(Self::dialog_frame(pal)).backdrop_color(pal.scrim).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::LINK).font(icon_font(22.0)).color(pal.brand_fg));
                ui.label(RichText::new(format!("Who does {name} play with?")).font(semibold(20.0)));
            });
            ui.label(
                RichText::new(format!(
                    "Linked players always go on the same court, whenever they're here and free at the same time. \
                     Up to {others} others. It carries over to every session until you unlink them."
                ))
                .font(regular(14.0))
                .color(pal.fg2),
            );
            ui.add_space(10.0);
            Self::search_box(ui, &mut d.search, "Find a player");
            ui.add_space(6.0);
            let query = d.search.trim().to_lowercase();
            let mut list: Vec<&Player> = self
                .state
                .players
                .values()
                .filter(|p| p.id != d.id && (query.is_empty() || p.name.to_lowercase().contains(&query) || d.chosen.contains(&p.id)))
                .collect();
            // The chosen at the top, then everyone by name.
            list.sort_by_key(|p| (!d.chosen.contains(&p.id), p.name.to_lowercase()));
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for p in list {
                    let on = d.chosen.contains(&p.id);
                    let full = d.chosen.len() >= others;
                    // Someone in another group leaves it if picked here.
                    let theirs: Vec<String> = self.state.linked(p.id).iter().filter(|q| **q != d.id && !d.chosen.contains(q)).map(|q| self.state.name(*q).to_owned()).collect();
                    let note = if !theirs.is_empty() && !on {
                        format!("plays with {}", and_list(&theirs))
                    } else if full && !on {
                        format!("{others} already picked")
                    } else {
                        String::new()
                    };
                    if Self::pick_row(ui, pal, on, on || !full, p.skill, &p.name, &note) {
                        if on {
                            d.chosen.retain(|x| *x != p.id);
                        } else {
                            d.chosen.push(p.id);
                        }
                    }
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if !self.state.linked(d.id).is_empty() {
                    unlink = widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::CLOSE), "Unlink", 34.0)
                        .on_hover_text(format!("{name} plays on their own again"))
                        .clicked();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    save = widgets::button(ui, pal, ButtonKind::Primary, None, "Save", 34.0).clicked();
                    cancel |= widgets::button(ui, pal, ButtonKind::Secondary, None, "Cancel", 34.0).clicked();
                });
            });
        });
        let id = d.id;
        if unlink {
            let partners = and_list(&self.partner_names(id));
            self.act(ctx, format!("{name} no longer plays with {partners}"), |s| s.unlink(id));
        } else if save {
            let mut chosen = d.chosen.clone();
            chosen.sort_by_key(|p| self.state.name(*p).to_lowercase());
            let names: Vec<String> = chosen.iter().map(|p| self.state.name(*p).to_owned()).collect();
            let text = if chosen.is_empty() {
                format!("{name} plays on their own")
            } else {
                format!("{} will always play together", and_list(&std::iter::once(name.clone()).chain(names).collect::<Vec<_>>()))
            };
            let now = self.state.now();
            self.act(ctx, text, |s| s.link_players(id, &chosen, now));
        } else if !cancel && !modal.should_close() {
            self.link_dialog = Some(d);
        }
    }

    pub(super) fn practice_dialog_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut d) = self.practice_dialog.take() else { return };
        let (mut save, mut cancel) = (false, false);
        let modal = Modal::new(unique("practice-dialog")).frame(Self::dialog_frame(pal)).backdrop_color(pal.scrim).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::TROPHY_F).font(icon_font(22.0)).color(practice_color(pal)));
                ui.label(RichText::new("Practice group").font(semibold(20.0)));
            });
            ui.label(
                RichText::new(
                    "Players in the practice group only play each other — handy before a tournament. \
                     The courts are still shared fairly: whichever side has played less tonight gets the next court, \
                     so everyone gets the same time on court. It lasts for this session.",
                )
                .font(regular(14.0))
                .color(pal.fg2),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.scope(|ui| {
                    ui.set_width(300.0);
                    Self::search_box(ui, &mut d.search, "Find a player");
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !d.chosen.is_empty() && widgets::button(ui, pal, ButtonKind::Secondary, None, "Clear", 30.0).clicked() {
                        d.chosen.clear();
                    }
                    let n = d.chosen.len();
                    let label = if n == 1 { "1 player".to_owned() } else { format!("{n} players") };
                    ui.label(RichText::new(label).font(semibold(14.0)).color(if n > 0 { practice_color(pal) } else { pal.fg3 }));
                });
            });
            ui.add_space(6.0);
            let query = d.search.trim().to_lowercase();
            let mut here: Vec<&Player> = self
                .state
                .players
                .values()
                .filter(|p| self.state.session.is_here(p.id) && (query.is_empty() || p.name.to_lowercase().contains(&query)))
                .collect();
            here.sort_by_key(|p| p.name.to_lowercase());
            if here.is_empty() {
                ui.add_space(12.0);
                let text = if query.is_empty() { "Nobody is checked in yet." } else { "Nobody checked in matches." };
                ui.label(RichText::new(text).font(regular(14.0)).color(pal.fg3));
            }
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for p in here {
                    let on = d.chosen.contains(&p.id);
                    let partners: Vec<String> = self.state.linked(p.id).iter().filter(|q| self.state.session.is_here(**q)).map(|q| self.state.name(*q).to_owned()).collect();
                    let note = if partners.is_empty() { String::new() } else { format!("plays with {}", and_list(&partners)) };
                    if Self::pick_row(ui, pal, on, true, p.skill, &p.name, &note) {
                        if on {
                            d.chosen.remove(&p.id);
                        } else {
                            d.chosen.insert(p.id);
                        }
                    }
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            ui.add_space(14.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                save = widgets::button(ui, pal, ButtonKind::Primary, None, "Save", 34.0).clicked();
                cancel |= widgets::button(ui, pal, ButtonKind::Secondary, None, "Cancel", 34.0).clicked();
            });
        });
        if save {
            let chosen = std::mem::take(&mut d.chosen);
            let n = chosen.len();
            let text = match n {
                0 => "No practice group tonight".to_owned(),
                1 => "Practice group: 1 player (they'll need company to play)".to_owned(),
                n => format!("Practice group: {n} players"),
            };
            let now = self.state.now();
            self.act(ctx, text, |s| s.set_practice(chosen, now));
        } else if !cancel && !modal.should_close() {
            self.practice_dialog = Some(d);
        }
    }

    /// Put one player in or out of tonight's practice group.
    pub(super) fn toggle_practice(&mut self, ctx: &egui::Context, id: PlayerId) {
        let name = self.state.name(id).to_owned();
        let now = self.state.now();
        let mut set = self.state.session.practice.clone();
        let text = if set.remove(&id) {
            format!("{name} left the practice group")
        } else {
            set.insert(id);
            format!("{name} joined the practice group")
        };
        self.act(ctx, text, |s| s.set_practice(set, now));
    }
}
