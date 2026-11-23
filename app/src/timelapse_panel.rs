//! The Time-lapse window: options, a progress bar and a cancel button.
//! Natively the job runs on a worker thread; on the web it is stepped a
//! frame at a time from the UI loop so the page never freezes.

use eframe::egui;
use worldgen_core::timelapse::TimelapseOptions;
use worldgen_core::World;

use crate::timelapse::{ffmpeg_available, Format, Job, Settings};

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    progress: std::sync::Arc<std::sync::atomic::AtomicU32>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    result: std::sync::mpsc::Receiver<Result<String, String>>,
}

pub struct TimelapsePanel {
    pub open: bool,
    from: u32,
    to: u32,
    fps: u32,
    years_per_second: f32,
    scale: usize,
    hold: bool,
    subtitles: bool,
    format: Format,
    ffmpeg: Option<bool>,
    notice: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<Worker>,
    /// On the web: the job being stepped, and the frames it has.
    #[cfg(target_arch = "wasm32")]
    job: Option<Job>,
}

impl Default for TimelapsePanel {
    fn default() -> Self {
        Self {
            open: false,
            from: 0,
            to: u32::MAX,
            fps: 12,
            years_per_second: 25.0,
            scale: 1,
            hold: true,
            subtitles: true,
            format: Format::Gif,
            ffmpeg: None,
            notice: None,
            #[cfg(not(target_arch = "wasm32"))]
            worker: None,
            #[cfg(target_arch = "wasm32")]
            job: None,
        }
    }
}

impl TimelapsePanel {
    pub fn show(&mut self, ctx: &egui::Context, world: &World, title: &str, era: &str) {
        if !self.open {
            return;
        }
        let years = world.history.years;
        self.to = self.to.min(years);
        let mut open = true;
        let mut start = false;
        let mut cancel = false;
        let running = self.running();
        let progress = self.progress(world);
        egui::Window::new("Time-lapse")
            .id(egui::Id::new("timelapse_window"))
            .open(&mut open)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Years");
                        ui.add(egui::DragValue::new(&mut self.from).range(0..=years));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut self.to).range(0..=years));
                    });
                    ui.add(
                        egui::Slider::new(&mut self.years_per_second, 1.0..=200.0)
                            .logarithmic(true)
                            .text("years a second"),
                    );
                    ui.add(egui::Slider::new(&mut self.fps, 4..=30).text("frames a second"));
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        for scale in [1, 2] {
                            let px = world.config.width * scale;
                            ui.radio_value(&mut self.scale, scale, format!("{px} px"));
                        }
                    });
                    ui.checkbox(&mut self.hold, "Pause on great events");
                    ui.checkbox(&mut self.subtitles, "Event subtitles");
                    let ffmpeg = *self.ffmpeg.get_or_insert_with(ffmpeg_available);
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.format, Format::Gif, "GIF");
                        ui.add_enabled_ui(ffmpeg, |ui| {
                            ui.radio_value(&mut self.format, Format::Mp4, "MP4")
                                .on_disabled_hover_text(if cfg!(target_arch = "wasm32") {
                                    "MP4 needs the desktop app with ffmpeg installed."
                                } else {
                                    "MP4 needs ffmpeg on your PATH."
                                });
                        });
                    });
                    let options = self.options(years);
                    let frames = worldgen_core::timelapse::schedule(&world.history, &options).len();
                    ui.weak(format!(
                        "{frames} frames, about {:.0} seconds",
                        frames as f32 / self.fps.max(1) as f32
                    ));
                });
                ui.add_space(6.0);
                if running {
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    cancel = ui.button("Cancel").clicked();
                } else {
                    start = ui.button("Render…").clicked();
                }
                if let Some(notice) = &self.notice {
                    ui.weak(notice);
                }
            });
        if !open {
            self.open = false;
        }
        if cancel {
            self.cancel();
        }
        if start {
            self.notice = Some(match self.start(world, title, era) {
                Ok(true) => "Rendering…".into(),
                Ok(false) => "Export cancelled.".into(),
                Err(e) => e,
            });
        }
        self.poll(world);
        if self.running() {
            ctx.request_repaint();
        }
    }

    fn options(&self, years: u32) -> TimelapseOptions {
        TimelapseOptions {
            from: self.from.min(years),
            to: self.to.min(years).max(self.from.min(years)),
            fps: self.fps.max(1),
            years_per_second: self.years_per_second,
            hold_seconds: if self.hold { 0.5 } else { 0.0 },
        }
    }

    fn settings(&self, world: &World, title: &str, era: &str) -> Settings {
        Settings {
            options: self.options(world.history.years),
            scale: self.scale,
            subtitles: self.subtitles,
            format: self.format,
            title: title.to_owned(),
            era: era.to_owned(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn running(&self) -> bool {
        self.worker.is_some()
    }

    #[cfg(target_arch = "wasm32")]
    fn running(&self) -> bool {
        self.job.is_some()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn progress(&self, _world: &World) -> f32 {
        self.worker.as_ref().map_or(0.0, |w| {
            w.progress.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn progress(&self, _world: &World) -> f32 {
        self.job.as_ref().map_or(0.0, Job::progress)
    }

    /// Asks where to save, then starts rendering on a worker thread.
    #[cfg(not(target_arch = "wasm32"))]
    fn start(&mut self, world: &World, title: &str, era: &str) -> Result<bool, String> {
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        use std::sync::Arc;
        let (kind, ext) = match self.format {
            Format::Gif => ("GIF animation", "gif"),
            Format::Mp4 => ("MP4 video", "mp4"),
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("timelapse-{}.{ext}", world.config.seed))
            .add_filter(kind, &[ext])
            .save_file()
        else {
            return Ok(false);
        };
        let settings = self.settings(world, title, era);
        let world = world.clone();
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (send, result) = std::sync::mpsc::channel();
        let (p, c) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            let run = || -> Result<String, String> {
                let mut job = Job::new(&world, settings, Some(&path))?;
                while !job.is_done() {
                    if c.load(Ordering::Relaxed) {
                        return Err("Time-lapse cancelled.".into());
                    }
                    job.step(&world, 8)?;
                    p.store((job.progress() * 1000.0) as u32, Ordering::Relaxed);
                }
                let frames = job.frame_count();
                if let Some(bytes) = job.finish()? {
                    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                }
                Ok(format!("Saved {frames} frames to {}.", path.display()))
            };
            let _ = send.send(run());
        });
        self.worker = Some(Worker {
            progress,
            cancel,
            result,
        });
        Ok(true)
    }

    #[cfg(target_arch = "wasm32")]
    fn start(&mut self, world: &World, title: &str, era: &str) -> Result<bool, String> {
        self.job = Some(Job::new(world, self.settings(world, title, era), None)?);
        Ok(true)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn cancel(&mut self) {
        if let Some(worker) = &self.worker {
            worker
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn cancel(&mut self) {
        self.job = None;
        self.notice = Some("Time-lapse cancelled.".into());
    }

    /// Picks up a finished worker (native), or renders the next frame and
    /// downloads the result when done (web).
    #[cfg(not(target_arch = "wasm32"))]
    fn poll(&mut self, _world: &World) {
        let Some(worker) = &self.worker else {
            return;
        };
        if let Ok(result) = worker.result.try_recv() {
            self.notice = Some(result.unwrap_or_else(|e| e));
            self.worker = None;
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn poll(&mut self, world: &World) {
        let Some(job) = &mut self.job else {
            return;
        };
        if let Err(e) = job.step(world, 1) {
            self.notice = Some(e);
            self.job = None;
            return;
        }
        if job.is_done() {
            let job = self.job.take().expect("checked above");
            let frames = job.frame_count();
            self.notice = Some(match job.finish() {
                Ok(Some(bytes)) => {
                    let name = format!("timelapse-{}.gif", world.config.seed);
                    match crate::export::save(&name, "GIF animation", "gif", &bytes) {
                        Ok(_) => format!("Time-lapse of {frames} frames downloaded."),
                        Err(e) => e,
                    }
                }
                Ok(None) => "Done.".into(),
                Err(e) => e,
            });
        }
    }
}
