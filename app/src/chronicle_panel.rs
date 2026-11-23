//! The Chronicle window: the event log as prose, following the time
//! slider, with filters and Markdown/HTML export.

use eframe::egui;
use worldgen_core::chronicle::{chronicle, Chronicle, ChronicleOptions, Focus};
use worldgen_core::history::EventKind;
use worldgen_core::World;

#[derive(Default)]
pub struct ChroniclePanel {
    pub open: bool,
    options: Option<ChronicleOptions>,
    text: Option<Chronicle>,
    /// The slider year we last scrolled to.
    scrolled_to: Option<u32>,
    notice: Option<String>,
}

impl ChroniclePanel {
    /// Drops the cached text and filters (the world changed).
    pub fn reset(&mut self) {
        self.options = None;
        self.text = None;
        self.scrolled_to = None;
    }

    pub fn show(&mut self, ctx: &egui::Context, world: &World, year: u32) {
        if !self.open {
            return;
        }
        let options = self
            .options
            .get_or_insert_with(|| ChronicleOptions::whole(world))
            .clone();
        let mut edited = options.clone();
        let mut open = true;
        let mut export: Option<&str> = None;
        let text = self.text.get_or_insert_with(|| chronicle(world, &options));
        egui::Window::new("Chronicle")
            .id(egui::Id::new("chronicle_window"))
            .open(&mut open)
            .default_size([460.0, 560.0])
            .show(ctx, |ui| {
                filters(ui, world, &mut edited);
                ui.horizontal(|ui| {
                    if ui.button("Export Markdown…").clicked() {
                        export = Some("md");
                    }
                    if ui.button("Export HTML…").clicked() {
                        export = Some("html");
                    }
                });
                if let Some(notice) = &self.notice {
                    ui.weak(notice);
                }
                ui.separator();
                let follow = self.scrolled_to != Some(year);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.heading(&text.title);
                        // The entry for the slider year: the latest at or before it.
                        let current = text
                            .chapters
                            .iter()
                            .flat_map(|c| &c.years)
                            .map(|y| y.year)
                            .filter(|&y| y <= year)
                            .max();
                        for chapter in &text.chapters {
                            ui.add_space(10.0);
                            ui.strong(&chapter.title);
                            ui.label(egui::RichText::new(&chapter.summary).italics());
                            for entry in &chapter.years {
                                let now = Some(entry.year) == current;
                                let line =
                                    egui::RichText::new(format!("{}.  {}", entry.year, entry.text));
                                let response = if now {
                                    ui.label(line.strong())
                                } else if entry.year > year {
                                    ui.label(line.weak())
                                } else {
                                    ui.label(line)
                                };
                                if now && follow {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                            }
                        }
                        for section in &text.appendix {
                            ui.add_space(10.0);
                            ui.strong(&section.title);
                            for entry in &section.entries {
                                ui.label(entry.replace("**", ""));
                            }
                        }
                    });
                if follow {
                    self.scrolled_to = Some(year);
                }
            });
        if !open {
            self.open = false;
        }
        if edited != options {
            self.options = Some(edited);
            self.text = None;
            self.scrolled_to = None;
        }
        if let (Some(kind), Some(text)) = (export, &self.text) {
            let (bytes, label) = match kind {
                "md" => (text.to_markdown().into_bytes(), "Markdown"),
                _ => (text.to_html().into_bytes(), "HTML page"),
            };
            let name = format!("chronicle-{}.{kind}", world.config.seed);
            self.notice = Some(match crate::export::save(&name, label, kind, &bytes) {
                Ok(true) => "Chronicle saved.".into(),
                Ok(false) => "Export cancelled.".into(),
                Err(e) => e,
            });
        }
    }
}

/// Whose story, which years, and how much of it.
fn filters(ui: &mut egui::Ui, world: &World, options: &mut ChronicleOptions) {
    let end = world.history.state_at(world.history.years);
    let label = |focus: &Focus| match focus {
        Focus::World => "The whole world".to_owned(),
        Focus::Realm(r) => end
            .realms
            .get(*r as usize)
            .map_or("?".into(), |r| r.name.clone()),
        Focus::People(p) => format!(
            "The {}",
            world
                .peoples
                .peoples
                .get(*p as usize)
                .map_or("?", |p| p.name.as_str())
        ),
        Focus::House(h) => format!("House {h}"),
    };
    egui::ComboBox::from_label("Story of")
        .selected_text(label(&options.focus))
        .width(220.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut options.focus, Focus::World, "The whole world");
            ui.separator();
            for r in 0..end.realms.len() as u16 {
                let focus = Focus::Realm(r);
                let text = label(&focus);
                ui.selectable_value(&mut options.focus, focus, text);
            }
            ui.separator();
            for p in 0..world.peoples.peoples.len() as u16 {
                let focus = Focus::People(p);
                let text = label(&focus);
                ui.selectable_value(&mut options.focus, focus, text);
            }
            ui.separator();
            let mut houses: Vec<String> = world
                .history
                .events
                .iter()
                .filter_map(|e| match &e.kind {
                    EventKind::RealmFounded { ruler, .. }
                    | EventKind::RulerSucceeded { ruler, .. }
                    | EventKind::RealmSplit { ruler, .. } => Some(ruler.house.clone()),
                    _ => None,
                })
                .collect();
            houses.sort();
            houses.dedup();
            for house in houses {
                let focus = Focus::House(house);
                let text = label(&focus);
                ui.selectable_value(&mut options.focus, focus, text);
            }
        });
    let years = world.history.years;
    ui.horizontal(|ui| {
        ui.label("Years");
        ui.add(egui::DragValue::new(&mut options.from).range(0..=years));
        ui.label("to");
        ui.add(egui::DragValue::new(&mut options.to).range(0..=years));
    });
    if options.from > options.to {
        options.to = options.from;
    }
    ui.horizontal(|ui| {
        ui.checkbox(&mut options.everything, "Include everything");
        ui.checkbox(&mut options.for_writers, "For writers");
    });
}
