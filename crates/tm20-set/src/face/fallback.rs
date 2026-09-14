//! Which stretches of a run go to the fallback face.
//!
//! The primary face shapes the whole run first. Every cluster it returned as
//! `.notdef` is missing. Emoji glue (joiners, variation selectors, skin tone
//! modifiers, keycap and tag characters) has no glyph of its own: the shaper
//! hides it in the primary face, so it never shows up as missing, yet it must
//! travel with the emoji next to it or the sequence falls apart. The spans
//! below widen the missing clusters over adjacent glue so `family`, `flag`,
//! and `keycap` sequences reach the fallback face whole.

use std::ops::Range;

/// A character that attaches to the emoji before or after it.
fn is_glue(c: char) -> bool {
    matches!(
        c,
        '\u{200D}' | '\u{FE0E}' | '\u{FE0F}' | '\u{20E3}' | '\u{E0020}'..='\u{E007F}' | '\u{1F3FB}'..='\u{1F3FF}'
    )
}

/// The combining enclosing keycap. Its base (`1`, `#`, `*`) is in every text
/// face, so the base is pulled into the fallback span with it.
const KEYCAP: char = '\u{20E3}';

/// Variation selector 16 asks for emoji presentation. A text face may own
/// the base character (Source Sans has a heart), but the writer asked for the
/// emoji, so the base goes to the fallback face with the selector.
pub(super) const EMOJI_PRESENTATION: char = '\u{FE0F}';

/// One stretch of the run and which face draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Segment {
    pub(super) range: Range<usize>,
    pub(super) fallback: bool,
}

/// Byte ranges the primary face returned `.notdef` for, from the clusters of
/// its shaped buffer. `clusters` are the byte offsets of every glyph in
/// buffer order; `missing` are the offsets of the `.notdef` glyphs.
pub(super) fn missing_ranges(
    text_len: usize,
    clusters: &[u32],
    missing: &[u32],
) -> Vec<Range<usize>> {
    let mut starts: Vec<usize> = clusters.iter().map(|&c| c as usize).collect();
    starts.sort_unstable();
    starts.dedup();
    let mut ranges = Vec::new();
    for &offset in missing {
        let start = offset as usize;
        let end = starts
            .iter()
            .find(|&&s| s > start)
            .copied()
            .unwrap_or(text_len)
            .min(text_len);
        if start < end {
            ranges.push(start..end);
        }
    }
    ranges.sort_unstable_by_key(|r| r.start);
    ranges.dedup();
    ranges
}

/// Split `text` into segments for the primary and the fallback face. The
/// fallback segments cover `missing` widened over emoji glue, plus any
/// character the writer marked for emoji presentation; everything else stays
/// with the primary face. Segments are in order and cover the text.
pub(super) fn segments(text: &str, missing: &[Range<usize>]) -> Vec<Segment> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    if n == 0 {
        return Vec::new();
    }
    let mut marked = vec![false; n];
    for (i, (offset, _)) in chars.iter().enumerate() {
        if missing.iter().any(|r| r.contains(offset)) {
            marked[i] = true;
        }
        if i > 0 && chars[i].1 == EMOJI_PRESENTATION {
            marked[i] = true;
            marked[i - 1] = true;
        }
    }
    widen_over_glue(&chars, &mut marked);

    let mut out: Vec<Segment> = Vec::new();
    for (i, (offset, c)) in chars.iter().enumerate() {
        let end = offset + c.len_utf8();
        match out.last_mut() {
            Some(last) if last.fallback == marked[i] => last.range.end = end,
            _ => out.push(Segment {
                range: *offset..end,
                fallback: marked[i],
            }),
        }
    }
    out
}

/// Glue next to a marked character is marked too, in both directions, until
/// nothing changes. A marked keycap marks the base character before it.
fn widen_over_glue(chars: &[(usize, char)], marked: &mut [bool]) {
    let n = chars.len();
    loop {
        let mut changed = false;
        for i in 0..n {
            let c = chars[i].1;
            if !marked[i] && is_glue(c) {
                let before = i > 0 && marked[i - 1];
                let after = i + 1 < n && marked[i + 1];
                if before || after {
                    marked[i] = true;
                    changed = true;
                }
            }
            if marked[i] && c == KEYCAP {
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    if !is_glue(chars[j].1) {
                        if !marked[j] {
                            marked[j] = true;
                            changed = true;
                        }
                        break;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(range: Range<usize>, fallback: bool) -> Segment {
        Segment { range, fallback }
    }

    fn fallback_texts<'t>(text: &'t str, missing: &[Range<usize>]) -> Vec<&'t str> {
        segments(text, missing)
            .iter()
            .filter(|s| s.fallback)
            .map(|s| &text[s.range.clone()])
            .collect()
    }

    #[test]
    fn missing_clusters_become_byte_ranges_up_to_the_next_cluster() {
        // "a" + grinning face (4 bytes) + "b": clusters at 0, 1, 5.
        let ranges = missing_ranges(6, &[0, 1, 5], &[1]);
        assert_eq!(ranges, vec![1..5]);
        let last = missing_ranges(6, &[0, 1, 5], &[5, 1]);
        assert_eq!(last, vec![1..5, 5..6]);
    }

    #[test]
    fn nothing_missing_is_one_primary_segment() {
        assert_eq!(segments("hello", &[]), vec![seg(0..5, false)]);
        assert!(segments("", &[]).is_empty());
    }

    #[test]
    fn an_emoji_between_words_is_its_own_segment() {
        let text = "hi \u{1F600} you";
        let start = text.find('\u{1F600}').unwrap();
        let got = segments(text, std::slice::from_ref(&(start..start + 4)));
        assert_eq!(
            got,
            vec![seg(0..3, false), seg(3..7, true), seg(7..11, false)]
        );
    }

    #[test]
    fn zwj_sequences_and_selectors_travel_with_the_emoji() {
        // man ZWJ woman ZWJ girl: the joiners are hidden by the primary shaper.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let text = format!("us: {family}!");
        let missing: Vec<Range<usize>> = text
            .char_indices()
            .filter(|(_, c)| matches!(c, '\u{1F468}' | '\u{1F469}' | '\u{1F467}'))
            .map(|(i, c)| i..i + c.len_utf8())
            .collect();
        assert_eq!(fallback_texts(&text, &missing), vec![family]);

        let heart = "\u{2764}\u{FE0F}";
        let text = format!("love {heart} you");
        let at = text.find('\u{2764}').unwrap();
        assert_eq!(
            fallback_texts(&text, std::slice::from_ref(&(at..at + 3))),
            vec![heart]
        );
    }

    #[test]
    fn emoji_presentation_takes_a_character_the_primary_face_owns() {
        let heart = "\u{2764}\u{FE0F}";
        let text = format!("love {heart} you");
        assert_eq!(fallback_texts(&text, &[]), vec![heart]);
        assert_eq!(
            fallback_texts("love \u{2764} you", &[]),
            Vec::<&str>::new(),
            "a bare heart stays with the text face"
        );
    }

    #[test]
    fn skin_tones_flags_and_keycaps_stay_whole() {
        let wave = "\u{1F44B}\u{1F3FD}";
        let at = 0;
        assert_eq!(
            fallback_texts(&format!("{wave} hi"), std::slice::from_ref(&(at..at + 4))),
            vec![wave]
        );

        let flag = "\u{1F1EB}\u{1F1F7}";
        let text = format!("from {flag}");
        let at = text.find('\u{1F1EB}').unwrap();
        assert_eq!(
            fallback_texts(&text, &[at..at + 4, at + 4..at + 8]),
            vec![flag]
        );

        let keycap = "1\u{FE0F}\u{20E3}";
        let text = format!("press {keycap} now");
        let at = text.find('\u{20E3}').unwrap();
        assert_eq!(
            fallback_texts(&text, std::slice::from_ref(&(at..at + 3))),
            vec![keycap]
        );
    }

    #[test]
    fn glue_far_from_any_emoji_stays_with_the_primary_face() {
        let text = "a\u{200D}b \u{1F600}";
        let at = text.find('\u{1F600}').unwrap();
        let got = segments(text, std::slice::from_ref(&(at..at + 4)));
        assert_eq!(got, vec![seg(0..at, false), seg(at..at + 4, true)]);
    }
}
