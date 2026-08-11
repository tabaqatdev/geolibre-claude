//! Shared contracts + pure logic for GeoLibre-Claude.
//!
//! One source of truth for tool inputs/outputs so the MCP server (`geolibre-claude`)
//! and the GeoLibre TS plugin agree on shapes, plus the dependency-free pieces that
//! are worth unit-testing on their own: locale-aware text [`normalize`]-ation and the
//! BM25 catalog [`index`].

pub mod index;
pub mod normalize;

/// The GeoLibre UI locales we mirror. Missing keys fall back to `en`,
/// exactly like GeoLibre's own react-i18next catalogs.
pub const SUPPORTED_LOCALES: [&str; 16] = [
    "en", "zh", "es", "fr", "de", "pt", "it", "nl", "ja", "ko", "ru", "tr", "id", "hi", "th", "ar",
];

/// Returns `true` if `locale` is one we carry a catalog for.
pub fn is_supported_locale(locale: &str) -> bool {
    SUPPORTED_LOCALES.contains(&locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_and_english_are_supported() {
        assert!(is_supported_locale("ar"));
        assert!(is_supported_locale("en"));
        assert!(!is_supported_locale("xx"));
    }
}
