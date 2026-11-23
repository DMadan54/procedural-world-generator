//! The time slider, the event feed and the year cartouche.

use eframe::egui::{self, Color32, FontFamily, FontId, Rect, Stroke};
use worldgen_core::history::{Cause, Event, EventKind, History, HistoryState};
use worldgen_core::realms::NO_REALM;

/// The egui font family registered from IM Fell English.
pub const FELL: &str = "fell";

/// Lets egui draw text in IM Fell English (used by the cartouche).
pub fn register_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        FELL.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::style::FELL_REGULAR)),
    );
    fonts
        .families
        .insert(FontFamily::Name(FELL.into()), vec![FELL.to_owned()]);
    ctx.set_fonts(fonts);
}

pub struct Timeline {
    pub year: u32,
    pub playing: bool,
    /// Years per second while playing.
    pub speed: f32,
    /// Fractional years carried between frames while playing.
    carry: f64,
    /// Index into the event log of the event the user picked.
    pub selected: Option<usize>,
    /// List growth and shrinkage events too.
    show_minor: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            year: 0,
            playing: false,
            speed: 20.0,
            carry: 0.0,
            selected: None,
            show_minor: false,
        }
    }
}

impl Timeline {
    /// Advances the year while playing; returns true if it changed.
    pub fn tick(&mut self, ctx: &egui::Context, years: u32) -> bool {
        if !self.playing {
            return false;
        }
        if self.year >= years {
            self.playing = false;
            return false;
        }
        self.carry += ctx.input(|i| i.stable_dt) as f64 * self.speed as f64;
        let step = self.carry.floor();
        self.carry -= step;
        ctx.request_repaint();
        if step >= 1.0 {
            self.year = (self.year + step as u32).min(years);
            true
        } else {
            false
        }
    }

    /// The bottom bar: transport buttons, speed and the year slider. Returns
    /// true if the year changed.
    pub fn show_bar(
        &mut self,
        ctx: &egui::Context,
        history: &History,
        progress: Option<f32>,
    ) -> bool {
        let before = self.year;
        let years = history.years;
        egui::TopBottomPanel::bottom("timeline").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let major = |e: &Event| {
                    !e.kind.is_minor() && !matches!(e.kind, EventKind::RealmFounded { .. })
                };
                if ui.button("◀◀").on_hover_text("Previous event").clicked() {
                    if let Some(e) = history
                        .events
                        .iter()
                        .rev()
                        .find(|e| e.year < self.year && major(e))
                    {
                        self.year = e.year;
                    }
                }
                if ui.button("◀").on_hover_text("One year back").clicked() {
                    self.year = self.year.saturating_sub(1);
                }
                let label = if self.playing { "Pause" } else { "Play" };
                if ui.button(label).clicked() {
                    self.playing = !self.playing;
                    if self.playing && self.year >= years {
                        self.year = 0;
                    }
                }
                if ui.button("▶").on_hover_text("One year forward").clicked() {
                    self.year = (self.year + 1).min(years);
                }
                if ui.button("▶▶").on_hover_text("Next event").clicked() {
                    if let Some(e) = history
                        .events
                        .iter()
                        .find(|e| e.year > self.year && major(e))
                    {
                        self.year = e.year;
                    }
                }
                ui.separator();
                ui.label("Speed");
                ui.add(
                    egui::Slider::new(&mut self.speed, 1.0..=400.0)
                        .logarithmic(true)
                        .suffix(" yr/s"),
                );
                ui.separator();
                ui.strong(format!("Year {}", self.year));
                if let Some(p) = progress {
                    ui.add(
                        egui::ProgressBar::new(p)
                            .desired_width(160.0)
                            .text(format!("History {:.0}%", p * 100.0)),
                    );
                }
            });
            ui.spacing_mut().slider_width = ui.available_width() - 12.0;
            ui.add(egui::Slider::new(&mut self.year, 0..=years).show_value(false));
            ui.add_space(2.0);
        });
        self.year != before
    }

    /// The event feed: events around the current year, newest last.
    /// Clicking one jumps to it. Returns true if the year changed.
    pub fn show_feed(
        &mut self,
        ctx: &egui::Context,
        history: &History,
        names: &HistoryState,
    ) -> bool {
        let before = self.year;
        egui::SidePanel::right("events")
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.heading("Events");
                ui.checkbox(&mut self.show_minor, "Include growth and decline");
                ui.separator();
                let from = history.first_event_at_or_after(self.year.saturating_sub(40));
                let to = history.first_event_at_or_after(self.year + 41);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, event) in history.events.iter().enumerate().take(to).skip(from) {
                        if event.kind.is_minor() && !self.show_minor {
                            continue;
                        }
                        let text = format!("{:>4}  {}", event.year, describe(event, names));
                        let current = event.year <= self.year;
                        let label = if current {
                            egui::RichText::new(text)
                        } else {
                            egui::RichText::new(text).weak()
                        };
                        if ui
                            .selectable_label(self.selected == Some(index), label)
                            .clicked()
                        {
                            self.year = event.year;
                            self.selected = Some(index);
                            self.playing = false;
                        }
                    }
                });
            });
        self.year != before
    }
}

fn realm_name(names: &HistoryState, realm: u16) -> String {
    if realm == NO_REALM {
        return "the wilds".into();
    }
    names
        .realms
        .get(realm as usize)
        .map_or_else(|| format!("realm {realm}"), |r| r.name.clone())
}

fn settlement_name(names: &HistoryState, settlement: u32) -> String {
    names
        .settlements
        .get(settlement as usize)
        .map_or_else(|| format!("settlement {settlement}"), |s| s.name.clone())
}

fn cause_text(cause: Cause) -> &'static str {
    match cause {
        Cause::OldAge => " on the death of the old ruler",
        Cause::UntimelyDeath => " after an untimely death",
        Cause::BorderDispute => " in a border dispute",
        Cause::Ambition => "",
        Cause::Conquest => " by conquest",
        Cause::Overextension => ", grown too large to hold together",
        _ => "",
    }
}

/// A one-line description of an event for the feed. The full chronicle
/// with causes chained into prose comes later.
pub fn describe(event: &Event, names: &HistoryState) -> String {
    match &event.kind {
        EventKind::RealmFounded { realm, ruler, .. } => format!(
            "{} stands under {} of House {}",
            realm_name(names, *realm),
            ruler.name,
            ruler.house
        ),
        EventKind::RulerSucceeded {
            realm,
            ruler,
            cause,
        } => format!(
            "{} of House {} takes the throne of {}{}",
            ruler.name,
            ruler.house,
            realm_name(names, *realm),
            cause_text(*cause)
        ),
        EventKind::BorderChanged {
            from,
            to,
            cells,
            cause,
        } => {
            if *cause == Cause::Conquest {
                format!(
                    "{} conquers {}",
                    realm_name(names, *to),
                    realm_name(names, *from)
                )
            } else if *from == NO_REALM {
                format!(
                    "{} settles {} leagues of the wilds",
                    realm_name(names, *to),
                    cells.len()
                )
            } else {
                format!(
                    "{} takes {} leagues from {}{}",
                    realm_name(names, *to),
                    cells.len(),
                    realm_name(names, *from),
                    cause_text(*cause)
                )
            }
        }
        EventKind::SettlementFounded { name, .. } => format!("{name} is founded"),
        EventKind::SettlementGrew {
            settlement,
            population,
            ..
        } => {
            format!(
                "{} grows to {population}",
                settlement_name(names, *settlement)
            )
        }
        EventKind::SettlementShrank {
            settlement,
            population,
            ..
        } => {
            format!(
                "{} dwindles to {population}",
                settlement_name(names, *settlement)
            )
        }
        EventKind::SettlementRenamed { name, .. } => format!("A conquered town is renamed {name}"),
        EventKind::RealmFell { realm, .. } => format!("{} falls", realm_name(names, *realm)),
        EventKind::RealmSplit { realm, name, .. } => {
            format!(
                "{name} breaks away from {}{}",
                realm_name(names, *realm),
                cause_text(Cause::Overextension)
            )
        }
    }
}

/// The year in an antique cartouche at the top right of the map.
pub fn draw_cartouche(painter: &egui::Painter, map: Rect, year: u32, era: &str) {
    let fell = |size: f32| FontId::new(size, FontFamily::Name(FELL.into()));
    let ink = Color32::from_rgb(70, 48, 30);
    let title = painter.layout_no_wrap(format!("Year {year}"), fell(22.0), ink);
    let subtitle = painter.layout_no_wrap(format!("of the Age of {era}"), fell(14.0), ink);
    let width = title.size().x.max(subtitle.size().x) + 36.0;
    let height = title.size().y + subtitle.size().y + 18.0;
    let rect = Rect::from_min_size(
        egui::pos2(map.right() - width - 16.0, map.top() + 16.0),
        egui::vec2(width, height),
    );
    painter.rect_filled(rect, 10.0_f32, Color32::from_rgb(238, 226, 196));
    painter.rect_stroke(
        rect,
        10.0_f32,
        Stroke::new(1.5_f32, ink),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        rect.shrink(4.0),
        7.0_f32,
        Stroke::new(0.8_f32, ink),
        egui::StrokeKind::Inside,
    );
    let centre_x = rect.center().x;
    painter.galley(
        egui::pos2(centre_x - title.size().x / 2.0, rect.top() + 8.0),
        title.clone(),
        ink,
    );
    painter.galley(
        egui::pos2(
            centre_x - subtitle.size().x / 2.0,
            rect.top() + 8.0 + title.size().y,
        ),
        subtitle,
        ink,
    );
}

/// Rings around what the selected event involves.
pub fn draw_highlight(painter: &egui::Painter, map: Rect, state: &HistoryState, event: &Event) {
    let cell = map.width() / state.width.max(1) as f32;
    let at = |c: u32| {
        let (x, y) = (
            (c as usize % state.width) as f32 + 0.5,
            (c as usize / state.width) as f32 + 0.5,
        );
        map.min + egui::vec2(x, y) * cell
    };
    let stroke = Stroke::new(2.5_f32, Color32::from_rgb(200, 30, 20));
    if let EventKind::BorderChanged { cells, .. } = &event.kind {
        for &c in cells.iter().step_by((cells.len() / 300).max(1)) {
            painter.circle_filled(
                at(c),
                (cell * 0.6).max(1.0),
                Color32::from_rgba_unmultiplied(200, 30, 20, 120),
            );
        }
    }
    for realm in event.kind.realms() {
        if let Some(r) = state.realms.get(realm as usize) {
            if let Some(capital) = state.settlements.get(r.capital as usize) {
                painter.circle_stroke(at(capital.cell), 14.0_f32, stroke);
            }
        }
    }
    for s in event.kind.settlements() {
        if let Some(settlement) = state.settlements.get(s as usize) {
            painter.circle_stroke(at(settlement.cell), 9.0_f32, stroke);
        }
    }
}
