//! Naming languages. Each people gets its own small phonology (which sounds
//! it uses, how it builds syllables, how its words tend to end), so names
//! from one people sound related and two peoples sound clearly different.
//!
//! Everything is derived from the world seed and the language id, so the
//! same people always gets the same language and the same names.

use rand_chacha::rand_core::RngCore;
use rand_chacha::ChaCha8Rng;

use crate::rng::{entity_rng, stage, unit_f32};

/// Every onset (syllable-initial consonant or cluster) a language may use.
const ONSETS: &[&str] = &[
    "b", "c", "d", "f", "g", "h", "j", "k", "l", "m", "n", "p", "r", "s", "t", "v", "w", "z", "th",
    "sh", "kh", "gr", "br", "dr", "tr", "st", "sk", "vr", "kr", "ch", "gh", "y", "ph", "qu",
];
/// Plain vowels; every language gets at least two.
const SIMPLE_VOWELS: &[&str] = &["a", "e", "i", "o", "u", "y"];
/// Vowel pairs; a language uses a few, sparingly.
const DIPHTHONGS: &[&str] = &["ae", "ai", "ei", "ou", "ia", "ea", "au", "oe", "io"];
/// Every coda (syllable-final consonant or cluster) a language may use.
const CODAS: &[&str] = &[
    "n", "r", "l", "s", "th", "m", "k", "d", "sh", "nd", "rn", "st", "x", "g", "rd", "lk", "ng",
    "v", "z",
];

const VOWELS: &[u8] = b"aeiouy";

/// A generated naming language.
#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    pub id: u32,
    seed: u64,
    onsets: Vec<&'static str>,
    vowels: Vec<&'static str>,
    diphthongs: Vec<&'static str>,
    /// Chance a nucleus is a diphthong rather than a plain vowel.
    diphthong_chance: f32,
    codas: Vec<&'static str>,
    /// Chance that a syllable is closed by a coda.
    coda_chance: f32,
    /// Chance that a syllable has no onset (starts with a vowel).
    bare_chance: f32,
    min_syllables: u32,
    max_syllables: u32,
    /// Word endings this language favours, built from its own sounds.
    endings: Vec<String>,
    /// Endings used for place names ("-holm", "-ard"...).
    place_endings: Vec<String>,
    /// How the people refer to themselves ("-i", "-an"...).
    people_ending: String,
}

impl Language {
    /// The language of people `id` in the world with this seed.
    pub fn generate(seed: u64, id: u32) -> Self {
        let rng = &mut entity_rng(seed, stage::LANGUAGE, id as u64);
        let onsets = pick_subset(rng, ONSETS, 7, 12);
        let vowels = pick_subset(rng, SIMPLE_VOWELS, 2, 5);
        let diphthongs = pick_subset(rng, DIPHTHONGS, 1, 3);
        let diphthong_chance = 0.1 + 0.25 * unit_f32(rng);
        let codas = pick_subset(rng, CODAS, 2, 6);
        let coda_chance = 0.1 + 0.5 * unit_f32(rng);
        let bare_chance = 0.05 + 0.2 * unit_f32(rng);
        let min_syllables = 1 + (rng.next_u32() % 2);
        let max_syllables = min_syllables + 1 + (rng.next_u32() % 2);

        let mut language = Self {
            id,
            seed,
            onsets,
            vowels,
            diphthongs,
            diphthong_chance,
            codas,
            coda_chance,
            bare_chance,
            min_syllables,
            max_syllables,
            endings: Vec::new(),
            place_endings: Vec::new(),
            people_ending: String::new(),
        };
        // Endings are short fragments in the language's own sounds, so they
        // reinforce its character instead of sounding borrowed.
        for _ in 0..3 {
            let nucleus = language.nucleus(rng);
            let coda = if unit_f32(rng) < 0.6 {
                language.pick(rng, &language.codas)
            } else {
                ""
            };
            language.endings.push(format!("{nucleus}{coda}"));
        }
        for _ in 0..2 {
            let onset = language.pick(rng, &language.onsets);
            let nucleus = language.pick(rng, &language.vowels);
            let coda = language.pick(rng, &language.codas);
            language
                .place_endings
                .push(format!("{onset}{nucleus}{coda}"));
        }
        let nucleus = language.pick(rng, &language.vowels);
        language.people_ending = match rng.next_u32() % 3 {
            0 => nucleus.to_owned(),
            1 => format!("{nucleus}n"),
            _ => format!("{nucleus}r"),
        };
        language
    }

    /// What the people call themselves.
    /// The seed this language was generated from; with `id`,
    /// `Language::generate(seed, id)` rebuilds it.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn people_name(&self) -> String {
        let rng = &mut self.rng(stage::NAMES_PEOPLE, 0);
        let stem = self.word(rng, 1, 2, false);
        finish(
            &format!("{stem}{}", self.people_ending),
            &self.people_ending,
        )
    }

    /// Place name number `index` (settlements, regions, rivers...). Callers
    /// wanting unique names try successive indices.
    pub fn place_name(&self, index: u64) -> String {
        let rng = &mut self.rng(stage::NAMES_PLACE, index);
        let use_place_ending = unit_f32(rng) < 0.3;
        let (min, max) = if use_place_ending {
            (1, self.max_syllables.min(2))
        } else {
            (self.min_syllables, self.max_syllables)
        };
        let stem = self.word(rng, min, max, !use_place_ending);
        if use_place_ending {
            let ending = self.pick(rng, &self.place_endings).to_owned();
            finish(&format!("{stem}{ending}"), &ending)
        } else {
            stem
        }
    }

    /// Personal name number `index` (rulers, heroes...).
    pub fn personal_name(&self, index: u64) -> String {
        let rng = &mut self.rng(stage::NAMES_PERSON, index);
        let min = self.min_syllables.max(2);
        self.word(rng, min, self.max_syllables.max(min), true)
    }

    fn rng(&self, stage_name: &str, index: u64) -> ChaCha8Rng {
        entity_rng(self.seed, stage_name, (self.id as u64) << 40 | index)
    }

    /// A vowel nucleus: usually plain, sometimes one of the language's
    /// diphthongs.
    fn nucleus(&self, rng: &mut impl RngCore) -> &'static str {
        if unit_f32(rng) < self.diphthong_chance {
            choose(rng, &self.diphthongs)
        } else {
            choose(rng, &self.vowels)
        }
    }

    fn pick<'a>(&self, rng: &mut impl RngCore, from: &'a [impl AsRef<str>]) -> &'a str {
        from[(rng.next_u32() as usize) % from.len()].as_ref()
    }

    /// A pronounceable capitalised word of `min..=max` syllables, with the
    /// language's favourite ending some of the time.
    fn word(&self, rng: &mut impl RngCore, min: u32, max: u32, with_ending: bool) -> String {
        for _ in 0..24 {
            let syllables = min + rng.next_u32() % (max - min + 1);
            let mut word = String::new();
            for k in 0..syllables {
                if k > 0 || unit_f32(rng) >= self.bare_chance {
                    word.push_str(self.pick(rng, &self.onsets));
                }
                word.push_str(self.nucleus(rng));
                if unit_f32(rng) < self.coda_chance {
                    word.push_str(self.pick(rng, &self.codas));
                }
            }
            if with_ending && unit_f32(rng) < 0.35 {
                word.push_str(self.pick(rng, &self.endings));
            }
            if is_pronounceable(&word) {
                return capitalise(&word);
            }
        }
        // Practically unreachable; a plain CV-CV word keeps it total.
        let a = self.pick(rng, &self.onsets);
        let b = self.pick(rng, &self.vowels);
        capitalise(&format!("{a}{b}{a}{b}"))
    }
}

/// One item from a list of static strings. Copies the `&'static str` out, so
/// the result outlives the list it came from (unlike `Language::pick`, whose
/// generic signature can only promise the list's own lifetime).
fn choose(rng: &mut impl RngCore, from: &[&'static str]) -> &'static str {
    from[(rng.next_u32() as usize) % from.len()]
}

/// Picks between `min` and `max` distinct items, keeping their master order.
fn pick_subset<T: Copy>(rng: &mut impl RngCore, all: &[T], min: usize, max: usize) -> Vec<T> {
    let count = min + (rng.next_u32() as usize) % (max - min + 1);
    let mut order: Vec<usize> = (0..all.len()).collect();
    for i in (1..order.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    let mut chosen: Vec<usize> = order[..count.min(all.len())].to_vec();
    chosen.sort_unstable();
    chosen.into_iter().map(|i| all[i]).collect()
}

/// Joins a suffix; if that breaks pronounceability, drops the suffix.
fn finish(candidate: &str, suffix: &str) -> String {
    if is_pronounceable(candidate) {
        capitalise(candidate)
    } else {
        capitalise(candidate.strip_suffix(suffix).unwrap_or(candidate))
    }
}

/// Lowercase ASCII letters, 3–10 long, no run of 4+ consonants or 3+
/// vowels, no letter three times in a row, no three-letter chunk repeated,
/// and at least one vowel.
pub fn is_pronounceable(word: &str) -> bool {
    let bytes = word.to_ascii_lowercase().into_bytes();
    if !(3..=10).contains(&bytes.len()) || !bytes.iter().all(u8::is_ascii_lowercase) {
        return false;
    }
    if !bytes.iter().any(|b| VOWELS.contains(b)) {
        return false;
    }
    // Spellings that read as typos rather than a foreign language.
    const AWKWARD: &[&[u8]] = &[b"yy", b"ii", b"uu", b"yi", b"iy", b"uy"];
    if bytes.windows(2).any(|w| AWKWARD.contains(&w)) {
        return false;
    }
    if bytes[0] == b'y' && !VOWELS.contains(&bytes[1]) {
        return false;
    }
    // A repeated chunk ("yugraungyung") reads as a stutter.
    let triples: Vec<&[u8]> = bytes.windows(3).collect();
    if triples
        .iter()
        .enumerate()
        .any(|(i, t)| triples[i + 1..].contains(t))
    {
        return false;
    }
    let (mut consonant_run, mut vowel_run) = (0, 0);
    for (i, b) in bytes.iter().enumerate() {
        if VOWELS.contains(b) {
            consonant_run = 0;
            vowel_run += 1;
        } else {
            consonant_run += 1;
            vowel_run = 0;
        }
        if consonant_run >= 4 || vowel_run >= 3 {
            return false;
        }
        if i >= 2 && bytes[i - 1] == *b && bytes[i - 2] == *b {
            return false;
        }
    }
    true
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn place_names(language: &Language, range: std::ops::Range<u64>) -> Vec<String> {
        range.map(|i| language.place_name(i)).collect()
    }

    #[test]
    fn same_id_same_language_and_names() {
        let a = Language::generate(42, 3);
        let b = Language::generate(42, 3);
        assert_eq!(a, b);
        assert_eq!(a.people_name(), b.people_name());
        assert_eq!(place_names(&a, 0..20), place_names(&b, 0..20));
        assert_eq!(a.personal_name(5), b.personal_name(5));
    }

    #[test]
    fn different_seeds_or_ids_give_different_languages() {
        assert_ne!(Language::generate(42, 3), Language::generate(42, 4));
        assert_ne!(Language::generate(42, 3), Language::generate(43, 3));
    }

    #[test]
    fn names_are_never_empty_or_unpronounceable() {
        for seed in 0..5 {
            for id in 0..12 {
                let language = Language::generate(seed, id);
                let mut names = vec![language.people_name()];
                names.extend(place_names(&language, 0..60));
                names.extend((0..30).map(|i| language.personal_name(i)));
                for name in names {
                    assert!(
                        is_pronounceable(&name),
                        "seed {seed} language {id}: {name:?}"
                    );
                    assert!(name.chars().next().unwrap().is_ascii_uppercase(), "{name}");
                }
            }
        }
    }

    #[test]
    fn pronounceability_rules() {
        assert!(is_pronounceable("velmar"));
        assert!(!is_pronounceable("velmstrka"), "4+ consonant run");
        assert!(!is_pronounceable("bbbak"), "tripled letter");
        assert!(!is_pronounceable("dreaaeng"), "3+ vowel run");
        assert!(!is_pronounceable("aegyyth"), "awkward pair");
        assert!(
            !is_pronounceable("yjubor"),
            "y before a consonant at the start"
        );
        assert!(!is_pronounceable("ungraungun"), "repeated chunk");
        assert!(!is_pronounceable("wulthouxbox"), "too long");
        assert!(!is_pronounceable("ab"), "too short");
        assert!(!is_pronounceable("rkst"), "no vowel");
    }

    /// Letter pairs (bigrams) used across a set of names.
    fn bigrams(names: &[String]) -> BTreeSet<[u8; 2]> {
        names
            .iter()
            .flat_map(|n| {
                let b = n.to_ascii_lowercase().into_bytes();
                b.windows(2).map(|w| [w[0], w[1]]).collect::<Vec<_>>()
            })
            .collect()
    }

    fn similarity(a: &[String], b: &[String]) -> f32 {
        let (a, b) = (bigrams(a), bigrams(b));
        a.intersection(&b).count() as f32 / a.union(&b).count().max(1) as f32
    }

    #[test]
    fn a_people_sounds_like_itself_and_unlike_others() {
        let languages: Vec<Language> = (0..8).map(|id| Language::generate(7, id)).collect();
        let samples: Vec<(Vec<String>, Vec<String>)> = languages
            .iter()
            .map(|l| (place_names(l, 0..40), place_names(l, 40..80)))
            .collect();
        let mut within = 0.0;
        let mut across = 0.0;
        let mut pairs = 0.0;
        for (i, (first, second)) in samples.iter().enumerate() {
            within += similarity(first, second);
            for (other, _) in samples.iter().skip(i + 1) {
                across += similarity(first, other);
                pairs += 1.0;
            }
        }
        let within = within / samples.len() as f32;
        let across = across / pairs;
        assert!(
            within > across + 0.15,
            "within {within:.2} vs across {across:.2}"
        );
    }
}
