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
mod canvas;
mod lettering;
mod text;
mod util;

pub use antique::AntiqueStyle;
pub use borders::realm_colour;
pub use canvas::Canvas;

use worldgen_core::World;

pub trait MapStyle {
    /// Name shown in the view menu.
    fn name(&self) -> &'static str;

    /// Renders the whole map. The canvas may be larger than the world grid
    /// (for crisper linework); the app scales it to fit.
    fn render(&self, world: &World) -> Canvas;
}

/// Every available style, default first.
pub fn all_styles() -> Vec<Box<dyn MapStyle>> {
    vec![Box::new(AntiqueStyle::default())]
}
