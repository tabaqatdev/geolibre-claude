---
name: geolibre-i18n
description: >-
  Handle geospatial requests in any of GeoLibre's 16 languages — replying in the user's language with
  correct direction, and matching a non-English request (Arabic especially) to the right map layer and
  field values. Use whenever the user writes in a non-English language, when a query in one language
  must line up with data labeled in another, or when an attribute filter on a text field needs to be
  robust to Arabic spelling/diacritic variation.
---

# GeoLibre-Claude — languages

GeoLibre ships in 16 locales: **en, zh, es, fr, de, pt, it, nl, ja, ko, ru, tr, id, hi, th, ar**.
Claude converses in all of them natively — the work is at the **data boundary**, where the user's
words have to line up with layer names and field values that may be in another language.

## 1. Reply in the user's language

Answer in the language the user wrote in, not English. For **Arabic**, write right-to-left and use
Arabic numerals/units naturally. Field *machine names* stay as they are (`POP`, `capacity`) — you
don't translate a column name — but everything you say around them is in the user's language.

## 2. Match a request to a layer + values

Layer names and fields are frequently **English-coded even when the question isn't**. You are the
translation layer:

1. **Look at the map** (`get_map_state`) and translate the concept — "المدارس" → look for a schools
   layer — then confirm with `describe_layer`.
2. **Use the sample values** from `describe_layer` to see the real spelling of attribute values before
   writing a `where`. If the data stores Arabic values, match against those exact forms.

## 3. Arabic attribute matching

Arabic text varies — alef/hamza forms (آأإٱ vs ا), optional tashkeel, tatweel (ـ) padding,
ta-marbuta (ة) vs ha (ه), alef-maqsura (ى) vs ya (ي), and Arabic-Indic digits (٠-٩). When you filter a
text field on an Arabic value:

- **Check the sample values** to see which forms the data actually uses, and match those.
- **Fold both sides** in the `where` when in doubt — e.g. compare a `replace(...)`-normalized column to
  a normalized literal, or use a `LIKE`/`ilike` with the stem, rather than an exact `=` that a single
  diacritic would break.
- Prefer the layer's own values over transliteration.

(There is no separate normalization tool — it's your judgement, informed by the sample values.)

## Boundaries

Cross-language matching is strongest when the layer carries that language's text or you translate the
concept to the layer's language. For a monolingual catalog, that translation is yours to do; a curated
term→layer glossary is a possible future aid.
