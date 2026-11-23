//! Time-lapse export: renders the history frame by frame in the antique
//! style (borders sweeping between sampled years, see
//! `worldgen_core::timelapse`) and encodes it as a GIF, or on native as an
//! MP4 through ffmpeg.
//!
//! A [`Job`] renders and encodes in order, a few frames per
//! [`Job::step`]. Native builds run it on a worker thread and render each
//! batch in parallel with rayon (frames come back in index order, so the
//! output is the same as rendering one by one). The web build has no
//! threads, so the app steps the job a frame or two per UI update.

#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;

use worldgen_core::history::{EventKind, HistoryState};
use worldgen_core::timelapse::{frame_state, schedule, FrameSpec, Sweep, TimelapseOptions};
use worldgen_core::World;

use crate::style::{AntiqueStyle, Canvas, MapStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Gif,
    /// Native only, through ffmpeg.
    Mp4,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub options: TimelapseOptions,
    /// Canvas pixels per world cell.
    pub scale: usize,
    pub subtitles: bool,
    pub format: Format,
    /// World name and era, for the cartouche.
    pub title: String,
    pub era: String,
}

/// The GIF palette index meaning "as in the previous frame".
const TRANSPARENT: u8 = 255;

/// Where encoded frames go.
enum Sink {
    Gif {
        encoder: gif::Encoder<Vec<u8>>,
        /// 15-bit RGB → palette index, filled once from the shared palette.
        lookup: Vec<u8>,
        /// The previous frame's palette indices: only what changed is
        /// stored, the rest left transparent over it.
        previous: Option<Vec<u8>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Ffmpeg(std::process::Child),
}

pub struct Job {
    settings: Settings,
    frames: Vec<FrameSpec>,
    next: usize,
    landscape: Canvas,
    style: Box<dyn MapStyle + Send + Sync>,
    /// The sweep for the current tick pair.
    sweep: Option<(u32, u32, Sweep)>,
    /// Names for subtitles: the state at the end of history.
    names: HistoryState,
    sink: Sink,
}

/// Whether ffmpeg can be run (MP4 export needs it).
#[cfg(not(target_arch = "wasm32"))]
pub fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(target_arch = "wasm32")]
pub fn ffmpeg_available() -> bool {
    false
}

impl Job {
    /// Prepares a job. For MP4, `output` is the file ffmpeg writes.
    pub fn new(
        world: &World,
        settings: Settings,
        output: Option<&std::path::Path>,
    ) -> Result<Self, String> {
        let frames = schedule(&world.history, &settings.options);
        if frames.is_empty() {
            return Err("No frames to render.".into());
        }
        let style = AntiqueStyle {
            scale: settings.scale.max(1),
        };
        let landscape = style.render_landscape(world);
        let (w, h) = (landscape.width, landscape.height);
        let mut job = Self {
            frames,
            next: 0,
            style: Box::new(style),
            sweep: None,
            names: world.history.state_at(world.history.years),
            sink: Sink::Gif {
                encoder: gif::Encoder::new(Vec::new(), w as u16, h as u16, &[])
                    .map_err(|e| e.to_string())?,
                lookup: Vec::new(),
                previous: None,
            },
            landscape,
            settings,
        };
        job.sink = match job.settings.format {
            Format::Gif => {
                // One palette for every frame, quantised from a few frames
                // across the run, so the parchment neither bands nor flickers.
                let n = job.frames.len();
                let samples: Vec<Canvas> = [0, n / 3, 2 * n / 3, n - 1]
                    .iter()
                    .map(|&i| job.render(world, i))
                    .collect();
                let mut rgba = Vec::new();
                for canvas in &samples {
                    for (i, px) in canvas.to_rgb8().as_chunks::<3>().0.iter().enumerate() {
                        if i % 3 == 0 {
                            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                        }
                    }
                }
                // 255 colours: the last index is kept for "unchanged" (transparent).
                let quant = color_quant::NeuQuant::new(10, 255, &rgba);
                let palette = quant.color_map_rgb();
                let lookup = (0..32768u32)
                    .map(|k| {
                        let (r, g, b) = ((k >> 10) & 31, (k >> 5) & 31, k & 31);
                        let px = [
                            (r << 3 | r >> 2) as u8,
                            (g << 3 | g >> 2) as u8,
                            (b << 3 | b >> 2) as u8,
                            255,
                        ];
                        quant.index_of(&px) as u8
                    })
                    .collect();
                let mut encoder = gif::Encoder::new(Vec::new(), w as u16, h as u16, &palette)
                    .map_err(|e| e.to_string())?;
                encoder
                    .set_repeat(gif::Repeat::Infinite)
                    .map_err(|e| e.to_string())?;
                Sink::Gif {
                    encoder,
                    lookup,
                    previous: None,
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Format::Mp4 => {
                let output = output.ok_or("MP4 export needs an output file.")?;
                let child = std::process::Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-loglevel",
                        "error",
                        "-f",
                        "rawvideo",
                        "-pix_fmt",
                        "rgb24",
                        "-s",
                    ])
                    .arg(format!("{w}x{h}"))
                    .args(["-r", &job.settings.options.fps.to_string(), "-i", "-"])
                    .args([
                        "-c:v",
                        "libx264",
                        "-pix_fmt",
                        "yuv420p",
                        "-vf",
                        "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                    ])
                    .arg(output)
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Couldn't start ffmpeg: {e}"))?;
                Sink::Ffmpeg(child)
            }
            #[cfg(target_arch = "wasm32")]
            Format::Mp4 => return Err("MP4 export needs the desktop app.".into()),
        };
        let _ = output;
        Ok(job)
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn progress(&self) -> f32 {
        self.next as f32 / self.frames.len().max(1) as f32
    }

    pub fn is_done(&self) -> bool {
        self.next >= self.frames.len()
    }

    /// Makes sure the sweep for this frame's ticks is ready.
    fn prepare(&mut self, world: &World, index: usize) {
        let f = &self.frames[index];
        if self
            .sweep
            .as_ref()
            .is_none_or(|(a, b, _)| (*a, *b) != (f.tick_from, f.tick_to))
        {
            let sweep = Sweep::between(
                &world.history.state_at(f.tick_from),
                &world.history.state_at(f.tick_to),
            );
            self.sweep = Some((f.tick_from, f.tick_to, sweep));
        }
    }

    /// Renders frame `index` (sequentially).
    pub fn render(&mut self, world: &World, index: usize) -> Canvas {
        self.prepare(world, index);
        let sweep = &self.sweep.as_ref().expect("prepared").2;
        render_frame(
            world,
            &self.settings,
            &*self.style,
            &self.landscape,
            &self.names,
            &self.frames[index],
            index,
            sweep,
        )
    }

    /// Renders and encodes up to `count` more frames.
    pub fn step(&mut self, world: &World, count: usize) -> Result<(), String> {
        let end = (self.next + count.max(1)).min(self.frames.len());
        let canvases = self.render_batch(world, self.next, end);
        for canvas in canvases {
            self.encode(&canvas)?;
        }
        self.next = end;
        Ok(())
    }

    /// Frames `start..end`, in order. Natively, frames that share a tick
    /// pair render in parallel.
    fn render_batch(&mut self, world: &World, start: usize, end: usize) -> Vec<Canvas> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            let mut out = Vec::with_capacity(end - start);
            let mut i = start;
            while i < end {
                self.prepare(world, i);
                let ticks = (self.frames[i].tick_from, self.frames[i].tick_to);
                let mut j = i;
                while j < end && (self.frames[j].tick_from, self.frames[j].tick_to) == ticks {
                    j += 1;
                }
                let sweep = &self.sweep.as_ref().expect("prepared").2;
                let (settings, style, landscape, names, frames) = (
                    &self.settings,
                    &*self.style,
                    &self.landscape,
                    &self.names,
                    &self.frames,
                );
                let batch: Vec<Canvas> = (i..j)
                    .into_par_iter()
                    .map(|k| {
                        render_frame(
                            world, settings, style, landscape, names, &frames[k], k, sweep,
                        )
                    })
                    .collect();
                out.extend(batch);
                i = j;
            }
            out
        }
        #[cfg(target_arch = "wasm32")]
        {
            (start..end).map(|k| self.render(world, k)).collect()
        }
    }

    fn encode(&mut self, canvas: &Canvas) -> Result<(), String> {
        let rgb = canvas.to_rgb8();
        let delay = (100.0 / self.settings.options.fps.max(1) as f32).round() as u16;
        match &mut self.sink {
            Sink::Gif {
                encoder,
                lookup,
                previous,
            } => {
                let indices: Vec<u8> = rgb
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| {
                        let k = ((p[0] as usize >> 3) << 10)
                            | ((p[1] as usize >> 3) << 5)
                            | (p[2] as usize >> 3);
                        lookup[k]
                    })
                    .collect();
                let (w, h) = (canvas.width, canvas.height);
                let frame = match previous.as_ref() {
                    None => gif::Frame {
                        width: w as u16,
                        height: h as u16,
                        buffer: std::borrow::Cow::Owned(indices.clone()),
                        delay,
                        dispose: gif::DisposalMethod::Keep,
                        ..Default::default()
                    },
                    Some(before) => {
                        // The box around everything that changed; inside it,
                        // unchanged pixels are transparent.
                        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
                        for (i, (a, b)) in indices.iter().zip(before).enumerate() {
                            if a != b {
                                let (x, y) = (i % w, i / w);
                                (x0, y0, x1, y1) =
                                    (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                            }
                        }
                        if x0 >= x1 {
                            (x0, y0, x1, y1) = (0, 0, 1, 1);
                        }
                        let mut buffer = Vec::with_capacity((x1 - x0) * (y1 - y0));
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let i = y * w + x;
                                buffer.push(if indices[i] == before[i] {
                                    TRANSPARENT
                                } else {
                                    indices[i]
                                });
                            }
                        }
                        gif::Frame {
                            left: x0 as u16,
                            top: y0 as u16,
                            width: (x1 - x0) as u16,
                            height: (y1 - y0) as u16,
                            buffer: std::borrow::Cow::Owned(buffer),
                            delay,
                            dispose: gif::DisposalMethod::Keep,
                            transparent: Some(TRANSPARENT),
                            ..Default::default()
                        }
                    }
                };
                *previous = Some(indices);
                encoder.write_frame(&frame).map_err(|e| e.to_string())
            }
            #[cfg(not(target_arch = "wasm32"))]
            Sink::Ffmpeg(child) => child
                .stdin
                .as_mut()
                .ok_or("ffmpeg closed its input")?
                .write_all(&rgb)
                .map_err(|e| format!("ffmpeg stopped: {e}")),
        }
    }

    /// Finishes the file: the GIF's bytes, or `None` once ffmpeg has
    /// written the MP4.
    pub fn finish(self) -> Result<Option<Vec<u8>>, String> {
        match self.sink {
            Sink::Gif { encoder, .. } => encoder.into_inner().map(Some).map_err(|e| e.to_string()),
            #[cfg(not(target_arch = "wasm32"))]
            Sink::Ffmpeg(mut child) => {
                drop(child.stdin.take());
                let status = child.wait().map_err(|e| e.to_string())?;
                if status.success() {
                    Ok(None)
                } else {
                    Err(format!("ffmpeg failed ({status})."))
                }
            }
        }
    }
}

/// One frame: the map at the frame's year with swept borders, the year
/// cartouche, a ring flashing on a held event, and a subtitle.
#[allow(clippy::too_many_arguments)]
fn render_frame(
    world: &World,
    settings: &Settings,
    style: &dyn MapStyle,
    landscape: &Canvas,
    names: &HistoryState,
    frame: &FrameSpec,
    index: usize,
    sweep: &Sweep,
) -> Canvas {
    let state = frame_state(&world.history, frame, sweep);
    let mut canvas = style.render_year(world, &state, landscape);
    let s = settings.scale.max(1) as f32;
    crate::style::decorate(
        &mut canvas,
        &crate::style::Decorations {
            title: &settings.title,
            subtitle: &format!("the year {} of the Age of {}", frame.year, settings.era),
            leagues_per_cell: 512.0 / world.config.width.max(1) as f32,
            cell_px: s,
        },
    );
    if let Some(event) = frame.hold {
        // Flash: the ring shows on every other held frame.
        if index % 4 < 2 {
            if let Some(cell) = event_cell(&world.history.events[event].kind, &state) {
                crate::style::ring(&mut canvas, cell, state.width, s);
            }
        }
    }
    if settings.subtitles {
        let latest = world.history.events[..world.history.first_event_at_or_after(frame.year + 1)]
            .iter()
            .rev()
            .take_while(|e| frame.year.saturating_sub(e.year) < 6)
            .find(|e| !e.kind.is_minor() && !matches!(e.kind, EventKind::BorderChanged { .. }));
        let line = match (frame.hold, latest) {
            (Some(i), _) => Some(crate::timeline::describe(&world.history.events[i], names)),
            (None, Some(e)) => Some(crate::timeline::describe(e, names)),
            _ => None,
        };
        if let Some(line) = line {
            crate::style::subtitle(&mut canvas, &line);
        }
    }
    canvas
}

/// Where on the map an event happened, for the flash.
fn event_cell(kind: &EventKind, state: &HistoryState) -> Option<u32> {
    let capital = |r: u16| {
        state
            .realms
            .get(r as usize)
            .and_then(|r| state.settlements.get(r.capital as usize))
            .map(|s| s.cell)
    };
    match kind {
        EventKind::WarDeclared { defender, .. } => capital(*defender),
        EventKind::RealmFell { realm, .. } | EventKind::RealmAnnexed { realm, .. } => {
            capital(*realm)
        }
        EventKind::RealmSplit { capital: c, .. } => {
            state.settlements.get(*c as usize).map(|s| s.cell)
        }
        EventKind::VolcanoErupted { cell, .. } => Some(*cell),
        EventKind::PlagueOutbreak { settlement, .. } => {
            state.settlements.get(*settlement as usize).map(|s| s.cell)
        }
        EventKind::DragonSlain { landmark, .. } => {
            state.landmarks.get(*landmark as usize).map(|l| l.cell)
        }
        EventKind::RiverAvulsion { new_course, .. } => new_course
            .first()
            .map(|p| p[1] as u32 * state.width as u32 + p[0] as u32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen_core::WorldConfig;

    fn settings() -> Settings {
        Settings {
            options: TimelapseOptions {
                from: 0,
                to: 60,
                fps: 10,
                years_per_second: 30.0,
                hold_seconds: 0.3,
            },
            scale: 1,
            subtitles: true,
            format: Format::Gif,
            title: "Test".into(),
            era: "Kings".into(),
        }
    }

    fn world() -> World {
        let mut config = WorldConfig {
            seed: 42,
            width: 64,
            height: 64,
            ..Default::default()
        };
        config.erosion.droplets = 1500;
        config.history.years = 60;
        World::generate(&config)
    }

    fn fnv(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01B3)
        })
    }

    #[test]
    fn frames_are_deterministic_and_batches_match_one_by_one() {
        let world = world();
        let mut a = Job::new(&world, settings(), None).unwrap();
        let mut b = Job::new(&world, settings(), None).unwrap();
        let n = a.frame_count();
        let one_by_one: Vec<u64> = (0..n)
            .map(|i| fnv(&a.render(&world, i).to_rgb8()))
            .collect();
        let batched: Vec<u64> = b
            .render_batch(&world, 0, n)
            .iter()
            .map(|c| fnv(&c.to_rgb8()))
            .collect();
        assert_eq!(one_by_one, batched);
        let mut c = Job::new(&world, settings(), None).unwrap();
        assert_eq!(fnv(&c.render(&world, n / 2).to_rgb8()), one_by_one[n / 2]);
    }

    #[test]
    fn gif_has_one_frame_per_scheduled_frame() {
        let world = world();
        let mut job = Job::new(&world, settings(), None).unwrap();
        let n = job.frame_count();
        while !job.is_done() {
            job.step(&world, 7).unwrap();
        }
        let bytes = job.finish().unwrap().unwrap();
        let mut decoder = gif::DecodeOptions::new()
            .read_info(bytes.as_slice())
            .unwrap();
        let mut frames = 0;
        while decoder.read_next_frame().unwrap().is_some() {
            frames += 1;
        }
        assert_eq!(frames, n);
    }
}
