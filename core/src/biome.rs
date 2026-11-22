//! Stage: biomes. Classifies each cell from its climate and elevation using a
//! simplified Whittaker table.

use crate::climate::{elevation_m, Climate};
use crate::config::WorldConfig;
use crate::heightmap::Heightmap;
use crate::hydrology::{Hydrology, NO_LAKE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Biome {
    Ocean,
    Lake,
    /// Permanent ice sheet.
    Ice,
    Tundra,
    /// Bare rock and snow above the tree line.
    Alpine,
    BorealForest,
    TemperateForest,
    TemperateRainforest,
    /// Dry summer scrub (Mediterranean-type).
    Shrubland,
    Grassland,
    Desert,
    Savanna,
    TropicalForest,
    TropicalRainforest,
}

impl Biome {
    pub const ALL: [Biome; 14] = [
        Biome::Ocean,
        Biome::Lake,
        Biome::Ice,
        Biome::Tundra,
        Biome::Alpine,
        Biome::BorealForest,
        Biome::TemperateForest,
        Biome::TemperateRainforest,
        Biome::Shrubland,
        Biome::Grassland,
        Biome::Desert,
        Biome::Savanna,
        Biome::TropicalForest,
        Biome::TropicalRainforest,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "Ocean",
            Biome::Lake => "Lake",
            Biome::Ice => "Ice",
            Biome::Tundra => "Tundra",
            Biome::Alpine => "Alpine",
            Biome::BorealForest => "Boreal forest",
            Biome::TemperateForest => "Temperate forest",
            Biome::TemperateRainforest => "Temperate rainforest",
            Biome::Shrubland => "Shrubland",
            Biome::Grassland => "Grassland",
            Biome::Desert => "Desert",
            Biome::Savanna => "Savanna",
            Biome::TropicalForest => "Tropical forest",
            Biome::TropicalRainforest => "Tropical rainforest",
        }
    }

    pub fn is_forest(self) -> bool {
        matches!(
            self,
            Biome::BorealForest
                | Biome::TemperateForest
                | Biome::TemperateRainforest
                | Biome::TropicalForest
                | Biome::TropicalRainforest
        )
    }

    pub fn is_water(self) -> bool {
        matches!(self, Biome::Ocean | Biome::Lake)
    }
}

/// Above this elevation (metres) land is alpine whatever the climate.
pub const TREE_LINE_M: f32 = 3000.0;

/// Land biome for a mean temperature (°C), annual precipitation (mm) and
/// elevation (m).
pub fn classify(temperature: f32, precipitation: f32, elevation_m: f32) -> Biome {
    if temperature < -10.0 {
        return Biome::Ice;
    }
    if elevation_m > TREE_LINE_M {
        return Biome::Alpine;
    }
    if temperature < -2.0 {
        return Biome::Tundra;
    }
    if temperature < 5.0 {
        return if precipitation < 250.0 {
            Biome::Tundra
        } else {
            Biome::BorealForest
        };
    }
    if temperature < 18.0 {
        return match precipitation {
            p if p < 250.0 => Biome::Desert,
            p if p < 500.0 && temperature > 10.0 => Biome::Shrubland,
            p if p < 600.0 => Biome::Grassland,
            p if p < 1800.0 => Biome::TemperateForest,
            _ => Biome::TemperateRainforest,
        };
    }
    match precipitation {
        p if p < 300.0 => Biome::Desert,
        p if p < 900.0 => Biome::Savanna,
        p if p < 1800.0 => Biome::TropicalForest,
        _ => Biome::TropicalRainforest,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Biomes {
    pub width: usize,
    pub height: usize,
    pub biome: Vec<Biome>,
}

impl Biomes {
    pub fn at(&self, x: usize, y: usize) -> Biome {
        self.biome[y * self.width + x]
    }
}

/// Runs the biome stage over the final terrain, climate and hydrology.
pub fn generate_biomes(
    config: &WorldConfig,
    terrain: &Heightmap,
    climate: &Climate,
    hydrology: &Hydrology,
) -> Biomes {
    let sea = config.terrain.sea_level;
    let biome = (0..terrain.data.len())
        .map(|i| {
            let h = terrain.data[i];
            if h < sea {
                Biome::Ocean
            } else if hydrology.lake_id[i] != NO_LAKE {
                Biome::Lake
            } else {
                classify(
                    climate.temperature[i],
                    climate.precipitation[i],
                    elevation_m(h, &config.terrain),
                )
            }
        })
        .collect();
    Biomes {
        width: terrain.width,
        height: terrain.height,
        biome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whittaker_corners() {
        assert_eq!(classify(-20.0, 500.0, 0.0), Biome::Ice);
        assert_eq!(classify(-5.0, 300.0, 0.0), Biome::Tundra);
        assert_eq!(classify(2.0, 600.0, 0.0), Biome::BorealForest);
        assert_eq!(classify(12.0, 1000.0, 0.0), Biome::TemperateForest);
        assert_eq!(classify(12.0, 2500.0, 0.0), Biome::TemperateRainforest);
        assert_eq!(classify(12.0, 100.0, 0.0), Biome::Desert);
        assert_eq!(classify(8.0, 400.0, 0.0), Biome::Grassland);
        assert_eq!(classify(15.0, 400.0, 0.0), Biome::Shrubland);
        assert_eq!(classify(25.0, 100.0, 0.0), Biome::Desert);
        assert_eq!(classify(25.0, 600.0, 0.0), Biome::Savanna);
        assert_eq!(classify(25.0, 1200.0, 0.0), Biome::TropicalForest);
        assert_eq!(classify(25.0, 3000.0, 0.0), Biome::TropicalRainforest);
    }

    #[test]
    fn high_ground_is_alpine() {
        assert_eq!(classify(10.0, 1000.0, 3500.0), Biome::Alpine);
    }

    #[test]
    fn every_biome_has_a_name() {
        for b in Biome::ALL {
            assert!(!b.name().is_empty());
        }
    }
}
