//! Locale-aware text normalization for catalog search and attribute matching.
//!
//! Pure and deterministic so it can be unit-tested without a catalog. Arabic gets
//! the most work — the same word appears with different alef/hamza forms, optional
//! tashkeel, and tatweel padding, so without normalization "الأحمدية" and "الاحمديه"
//! never match. Latin text just gets case/space folding.

/// The outcome of normalizing a string, plus which rule groups fired (useful for
/// explaining a match to a user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub text: String,
    pub applied: Vec<&'static str>,
}

/// Normalize `input`. Arabic rules run when `locale` is `ar` or the text contains
/// Arabic-block characters, so it works even when the caller didn't tag the locale.
pub fn normalize(input: &str, locale: Option<&str>) -> Normalized {
    let mut applied = vec!["lowercase"];
    let lowered = input.to_lowercase();

    let arabic = locale == Some("ar") || contains_arabic(&lowered);
    let mapped = if arabic {
        applied.push("arabic");
        normalize_arabic(&lowered)
    } else {
        lowered
    };

    let collapsed = mapped.split_whitespace().collect::<Vec<_>>().join(" ");
    applied.push("collapse_whitespace");

    Normalized { text: collapsed, applied }
}

fn contains_arabic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))
}

fn normalize_arabic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // Tatweel (kashida) — pure padding.
            '\u{0640}' => {}
            // Tashkeel (harakat) + superscript alef — drop diacritics.
            '\u{064B}'..='\u{0652}' | '\u{0670}' => {}
            // Alef variants → bare alef.
            '\u{0622}' | '\u{0623}' | '\u{0625}' | '\u{0671}' => out.push('\u{0627}'),
            // Alef maqsura → ya.
            '\u{0649}' => out.push('\u{064A}'),
            // Ta marbuta → ha.
            '\u{0629}' => out.push('\u{0647}'),
            // Hamza carriers → their base letter; bare hamza dropped.
            '\u{0624}' => out.push('\u{0648}'),
            '\u{0626}' => out.push('\u{064A}'),
            '\u{0621}' => {}
            // Arabic-Indic digits → ASCII.
            '\u{0660}'..='\u{0669}' => {
                out.push((b'0' + (c as u32 - 0x0660) as u8) as char)
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_alef_and_ta_marbuta() {
        // "الأحمدية" → alef-hamza→alef, ta-marbuta→ha
        assert_eq!(normalize("الأحمدية", Some("ar")).text, "الاحمديه");
    }

    #[test]
    fn arabic_strips_tashkeel_and_tatweel() {
        // "مَدْرَسَــة" (harakat + tatweel + ta-marbuta) → "مدرسه"
        assert_eq!(normalize("مَدْرَسَــة", Some("ar")).text, "مدرسه");
    }

    #[test]
    fn arabic_digits_and_autodetect() {
        // No locale passed, but Arabic detected; digits fold to ASCII.
        assert_eq!(normalize("٢٠٢٦", None).text, "2026");
    }

    #[test]
    fn latin_lowercase_and_whitespace() {
        assert_eq!(normalize("  Schools   By  District ", None).text, "schools by district");
    }
}
