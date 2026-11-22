//! Stage: climate. Mean temperature, prevailing winds and annual precipitation.
//!
//! The map spans the whole globe: the top row is the north pole and the
//! bottom row the south pole. Temperature falls with latitude and altitude.
//! Winds follow the three-cell pattern (easterly trades 0-30°, westerlies
//! 30-60°, polar easterlies 60-90°). Precipitation comes from air parcels
//! marched downwind: they pick up moisture over ocean and rain it out,
//! heavily where terrain forces them upwards, so the far side of a range is
//! left in a rain shadow.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand_chacha::rand_core::RngCore;

use crate::config::{ClimateConfig, TerrainConfig, WorldConfig};
use crate::field::gaussian_blur;
use crate::heightmap::Heightmap;
use crate::rng::{stage, stage_rng, unit_f64};

/// Converts the model's rain units into millimetres per year.
const RAIN_TO_MM: f32 = 200_000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Climate {
    pub width: usize,
    pub height: usize,
    /// Mean annual temperature, °C.
    pub temperature: Vec<f32>,
    /// Annual precipitation, mm.
    pub precipitation: Vec<f32>,
    /// Prevailing wind in map axes: x points east, y points south (down the
    /// map). Components are in `-1.0..=1.0`.
    pub wind: Vec<[f32; 2]>,
}

/// Latitude in degrees (north positive) at the centre of row `y`, for a map
/// spanning the whole globe.
pub fn latitude(y: usize, height: usize) -> f32 {
    band_latitude(y, height, 90.0, -90.0)
}

/// Latitude at the centre of row `y` for a map whose top row is at `north`
/// and bottom row at `south`.
pub fn band_latitude(y: usize, height: usize, north: f32, south: f32) -> f32 {
    north + (south - north) * (y as f32 + 0.5) / height.max(1) as f32
}

/// Height above sea level in metres. The sea surface (and anything below it)
/// is 0. Quadratic, so most land is lowland and only the highest ground
/// reaches mountain heights.
pub fn elevation_m(height: f32, terrain: &TerrainConfig) -> f32 {
    let sea = terrain.sea_level;
    if height <= sea {
        return 0.0;
    }
    let above = (height - sea) / (1.0 - sea).max(1e-6);
    above * above * terrain.max_elevation_m
}

/// Mean temperature (°C) at a latitude and altitude, before random variation.
pub fn temperature_at(latitude: f32, elevation_m: f32, config: &ClimateConfig) -> f32 {
    let sea_level = config.pole_temperature
        + (config.equator_temperature - config.pole_temperature) * latitude.to_radians().cos();
    sea_level - config.lapse_rate * elevation_m / 1000.0 + config.global_temperature_offset
}

/// Prevailing wind at a latitude as `[east, north]`, each in `-1.0..=1.0`.
///
/// Trades and polar easterlies blow west and towards the equator; westerlies
/// blow east and towards the pole. Directions blend over a few degrees at the
/// 30° and 60° band edges, where real winds go calm.
pub fn prevailing_wind(latitude: f32) -> [f32; 2] {
    const BLEND: f32 = 5.0;
    let a = latitude.abs();
    let edge = |at: f32| smoothstep(((a - at) / BLEND * 0.5 + 0.5).clamp(0.0, 1.0));
    // -1 in the trades, +1 in the westerlies, -1 in the polar easterlies.
    let east = -1.0 + 2.0 * edge(30.0) - 2.0 * edge(60.0);
    let hemisphere = if latitude >= 0.0 { 1.0 } else { -1.0 };
    // Easterlies drift equatorwards, westerlies polewards.
    let north = 0.4 * hemisphere * east;
    [east, north]
}

/// Relative amount of water vapour air can hold at a temperature (1.0 at
/// 25 °C, roughly doubling every 10 °C).
fn moisture_capacity(temperature: f32) -> f32 {
    (0.07 * (temperature - 25.0)).exp()
}

/// Large-scale rain tendency by latitude: wet along the equator where the
/// trades converge, dry around 30° under the subtropical highs.
fn rain_tendency(latitude: f32) -> f32 {
    let equatorial = 2.0 * (-(latitude / 8.0).powi(2)).exp();
    let subtropical = 0.7 * (-((latitude.abs() - 28.0) / 8.0).powi(2)).exp();
    (1.0 + equatorial) * (1.0 - subtropical)
}

/// Runs the climate stage over a finished heightmap.
pub fn generate_climate(config: &WorldConfig, heightmap: &Heightmap) -> Climate {
    let (width, height) = (heightmap.width, heightmap.height);
    let cfg = &config.climate;

    let mut rng = stage_rng(config.seed, stage::CLIMATE);
    let fbm = Fbm::<Perlin>::new(rng.next_u32())
        .set_octaves(3)
        .set_frequency(3.0);
    let (ox, oy) = (unit_f64(&mut rng) * 10_000.0, unit_f64(&mut rng) * 10_000.0);
    let inv = 1.0 / width.max(1) as f64;
    // Smooth random variation in -1..1, two independent channels.
    let variation = |x: usize, y: usize, channel: f64| {
        let (u, v) = (ox + x as f64 * inv + channel * 97.3, oy + y as f64 * inv);
        fbm.get([u, v]) as f32
    };

    let elevation: Vec<f32> = heightmap
        .data
        .iter()
        .map(|&h| elevation_m(h, &config.terrain))
        .collect();
    let ocean: Vec<bool> = heightmap
        .data
        .iter()
        .map(|&h| h < config.terrain.sea_level)
        .collect();

    let mut temperature = Vec::with_capacity(width * height);
    let mut wind = Vec::with_capacity(width * height);
    let lat_of = |y: usize| band_latitude(y, height, cfg.latitude_north, cfg.latitude_south);
    for y in 0..height {
        let lat = lat_of(y);
        let [east, north] = prevailing_wind(lat);
        for x in 0..width {
            let i = y * width + x;
            let jitter = cfg.temperature_variability * variation(x, y, 0.0);
            temperature.push(temperature_at(lat, elevation[i], cfg) + jitter);
            wind.push([east, -north]);
        }
    }

    let mut precipitation = march_parcels(cfg, width, height, &elevation, &ocean, &temperature, &wind);
    gaussian_blur(&mut precipitation, width, height, 1.5, false);
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let factor = 1.0 + 0.2 * variation(x, y, 1.0);
            precipitation[i] = (precipitation[i] * factor * RAIN_TO_MM).max(0.0);
        }
    }

    Climate {
        width,
        height,
        temperature,
        precipitation,
        wind,
    }
}

/// Carries moisture downwind and returns rain per pixel (model units).
///
/// One air parcel per row travels with the wind one column per step. Rows
/// blowing east are swept left to right and rows blowing west right to
/// left; each pixel takes its incoming air from the previous column,
/// interpolated between rows to follow the wind's north-south drift.
fn march_parcels(
    cfg: &ClimateConfig,
    width: usize,
    height: usize,
    elevation: &[f32],
    ocean: &[bool],
    temperature: &[f32],
    wind: &[[f32; 2]],
) -> Vec<f32> {
    let mut moisture = vec![0.0f32; width * height];
    let mut rain = vec![0.0f32; width * height];
    if width == 0 || height == 0 {
        return rain;
    }
    // Rates are tuned for 512 px across; keep the per-map totals the same at
    // other resolutions.
    let step = 512.0 / width as f32;
    let blows_east: Vec<bool> = (0..height).map(|y| wind[y * width][0] >= 0.0).collect();
    let tendency: Vec<f32> = (0..height)
        .map(|y| rain_tendency(band_latitude(y, height, cfg.latitude_north, cfg.latitude_south)))
        .collect();

    for eastward in [true, false] {
        let columns: Vec<usize> = if eastward {
            (0..width).collect()
        } else {
            (0..width).rev().collect()
        };
        for (n, &x) in columns.iter().enumerate() {
            let previous = if n == 0 { None } else { Some(columns[n - 1]) };
            for y in 0..height {
                if blows_east[y] != eastward {
                    continue;
                }
                let i = y * width + x;
                let capacity = moisture_capacity(temperature[i]);

                // Air arriving from upwind: (moisture, ground elevation).
                let (incoming, upwind_elevation) = match previous {
                    // Air entering from off the map.
                    None => (capacity * if ocean[i] { 0.7 } else { 0.3 }, elevation[i]),
                    Some(px) => {
                        let [u, v] = wind[i];
                        let dy = (v / u.abs().max(0.25)).clamp(-1.0, 1.0);
                        let sy = y as f32 - dy;
                        let y0 = sy.floor().clamp(0.0, (height - 1) as f32) as usize;
                        let y1 = (y0 + 1).min(height - 1);
                        let t = (sy - y0 as f32).clamp(0.0, 1.0);
                        // Only sample rows swept in this same pass.
                        let row = |r: usize| if blows_east[r] == eastward { r } else { y };
                        let (a, b) = (row(y0) * width + px, row(y1) * width + px);
                        (
                            moisture[a] + (moisture[b] - moisture[a]) * t,
                            elevation[a] + (elevation[b] - elevation[a]) * t,
                        )
                    }
                };

                let mut m = incoming;
                let pickup = if ocean[i] { cfg.evaporation } else { 0.2 * cfg.evaporation };
                m += (pickup * step).min(1.0) * (capacity - m).max(0.0);

                // Forced ascent: rain out a share per km climbed.
                let rise_km = (elevation[i] - upwind_elevation).max(0.0) / 1000.0;
                let mut r = m * (rise_km * cfg.orographic_rain).min(1.0);
                m -= r;
                // Background rain, shaped by latitude.
                let base = m * (cfg.base_rain * step * tendency[y]).min(1.0);
                r += base;
                m -= base;
                // Air cooled below its dew point drops the excess.
                if m > capacity {
                    r += m - capacity;
                    m = capacity;
                }
                moisture[i] = m;
                rain[i] = r / step;
            }
        }
    }
    rain
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_world(size: usize, height: f32) -> Heightmap {
        Heightmap {
            width: size,
            height: size,
            data: vec![height; size * size],
        }
    }

    #[test]
    fn deterministic() {
        let config = WorldConfig::default();
        let map = flat_world(64, 0.3);
        assert_eq!(generate_climate(&config, &map), generate_climate(&config, &map));
    }

    #[test]
    fn latitude_runs_pole_to_pole() {
        assert!((latitude(0, 180) - 89.5).abs() < 1e-4);
        assert!((latitude(179, 180) + 89.5).abs() < 1e-4);
        assert!(latitude(90, 180) < 0.0 && latitude(89, 180) > 0.0);
    }

    #[test]
    fn tropical_band_is_warmer_than_whole_globe() {
        let map = flat_world(64, 0.3);
        let mut tropics = WorldConfig::default();
        tropics.climate.latitude_north = 20.0;
        tropics.climate.latitude_south = -20.0;
        let mean = |c: &Climate| c.temperature.iter().sum::<f32>() / c.temperature.len() as f32;
        let globe = generate_climate(&WorldConfig::default(), &map);
        assert!(mean(&generate_climate(&tropics, &map)) > mean(&globe) + 5.0);
    }

    #[test]
    fn equator_is_warmer_than_poles() {
        let c = ClimateConfig::default();
        assert!(temperature_at(0.0, 0.0, &c) > temperature_at(45.0, 0.0, &c));
        assert!(temperature_at(45.0, 0.0, &c) > temperature_at(-85.0, 0.0, &c));
    }

    #[test]
    fn mountains_are_colder() {
        let c = ClimateConfig::default();
        let drop = temperature_at(10.0, 0.0, &c) - temperature_at(10.0, 2000.0, &c);
        assert!((drop - 13.0).abs() < 1e-4);
    }

    #[test]
    fn global_offset_shifts_every_temperature() {
        let base = ClimateConfig::default();
        let hot = ClimateConfig {
            global_temperature_offset: 10.0,
            ..base.clone()
        };
        for lat in [-80.0, 0.0, 37.0] {
            let d = temperature_at(lat, 500.0, &hot) - temperature_at(lat, 500.0, &base);
            assert!((d - 10.0).abs() < 1e-4);
        }
    }

    #[test]
    fn wind_bands() {
        for hemisphere in [1.0, -1.0] {
            let trades = prevailing_wind(15.0 * hemisphere);
            let westerlies = prevailing_wind(45.0 * hemisphere);
            let polar = prevailing_wind(75.0 * hemisphere);
            assert!(trades[0] < 0.0 && polar[0] < 0.0, "easterlies blow west");
            assert!(westerlies[0] > 0.0, "westerlies blow east");
            // north component: towards the equator for easterlies.
            assert!(trades[1] * hemisphere < 0.0);
            assert!(westerlies[1] * hemisphere > 0.0);
            assert!(polar[1] * hemisphere < 0.0);
        }
    }

    #[test]
    fn elevation_is_zero_at_and_below_sea_level() {
        let t = TerrainConfig::default();
        assert_eq!(elevation_m(0.1, &t), 0.0);
        assert_eq!(elevation_m(t.sea_level, &t), 0.0);
        assert!((elevation_m(1.0, &t) - t.max_elevation_m).abs() < 1e-3);
    }

    /// Ocean in the west, a north-south ridge in the middle of the land, in
    /// the westerlies: the windward (west) slope should be much wetter than
    /// the lee (east) side.
    #[test]
    fn ranges_cast_rain_shadows() {
        let size = 128;
        let mut map = flat_world(size, 0.55);
        for y in 0..size {
            for x in 0..size {
                let h = if x < 32 {
                    0.2
                } else if (60..70).contains(&x) {
                    0.55 + 0.4 * (1.0 - ((x as f32 - 64.5) / 5.0).abs())
                } else {
                    0.55
                };
                map.data[y * size + x] = h;
            }
        }
        let config = WorldConfig::default();
        let climate = generate_climate(&config, &map);
        // Row at about 45°N, in the westerlies.
        let y = 32;
        assert!(prevailing_wind(latitude(y, size))[0] > 0.0);
        let at = |x: usize| climate.precipitation[y * size + x];
        let windward = at(62);
        let lee = at(80);
        assert!(windward > 2.0 * lee, "windward {windward} vs lee {lee}");
    }

    #[test]
    fn coasts_are_wetter_than_deep_interiors() {
        // Ocean in the west, low flat land to the east, in the westerlies.
        let size = 128;
        let mut map = flat_world(size, 0.46);
        for y in 0..size {
            for x in 0..32 {
                map.data[y * size + x] = 0.2;
            }
        }
        let climate = generate_climate(&WorldConfig::default(), &map);
        let y = 32;
        let at = |x: usize| climate.precipitation[y * size + x];
        assert!(at(36) > 1.1 * at(120), "coast {} vs interior {}", at(36), at(120));
    }
}
