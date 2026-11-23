//! Finished-map styles.
//!
//! A [`MapStyle`] turns a generated [`World`] into a picture. Styles only read
//! the world; they never feed back into generation, so new styles (political,
//! satellite, ...) can be added here without touching `core`.
//!
//! To add a style: implement [`MapStyle`] in a new module and list it in
//! [`all_styles`]. It appears in the app's view menu automatically.

mod antique;
mod borders;
mod calamity;
mod canvas;
mod decor;
mod lettering;
mod network;
mod text;
mod util;
mod wonders;

pub use antique::AntiqueStyle;
pub use borders::realm_colour;
pub use canvas::Canvas;
pub use decor::{decorate, Decorations};
pub use text::REGULAR as FELL_REGULAR;

use worldgen_core::history::HistoryState;
use worldgen_core::World;

pub trait MapStyle {
    /// Name shown in the view menu.
    fn name(&self) -> &'static str;

    /// The layers that don't change with the year: terrain, water,
    /// vegetation. The app caches this until the world changes. The canvas
    /// may be larger than the world grid (for crisper linework).
    fn render_landscape(&self, world: &World) -> Canvas;

    /// The layers that change with the year (realms, settlements,
    /// lettering), drawn onto a copy of the cached landscape.
    fn render_year(&self, world: &World, state: &HistoryState, landscape: &Canvas) -> Canvas;

    /// The same style drawn at `scale` canvas pixels per world cell (for
    /// export).
    fn scaled(&self, scale: usize) -> Box<dyn MapStyle>;

    /// The whole map for one year, without caching (export and tests).
    fn render(&self, world: &World, state: &HistoryState) -> Canvas {
        self.render_year(world, state, &self.render_landscape(world))
    }
}

/// Every available style, default first.
pub fn all_styles() -> Vec<Box<dyn MapStyle>> {
    vec![Box::new(AntiqueStyle::default())]
}
