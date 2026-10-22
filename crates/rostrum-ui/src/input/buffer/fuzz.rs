//! Deterministic fuzzing of [`TextBuffer`]: thousands of random operation
//! sequences with fixed seeds, asserting after every step that nothing
//! panicked and the invariant holds.

use super::*;

/// xorshift64*: tiny, deterministic, and good enough to shake out edge cases.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// Fragments that exercise every width of UTF-8 and UTF-16, grapheme clusters
/// longer than one `char`, and line endings.
const FRAGMENTS: &[&str] = &[
    "a",
    "Z",
    " ",
    "\n",
    "\r\n",
    "\r",
    "—",
    "“",
    "”",
    "é",
    "e\u{301}",
    "\u{301}",
    "ß",
    "你",
    "日本",
    "😀",
    "👍🏽",
    "👨‍👩‍👧",
    "🇬🇧",
    "\t",
    "",
];

fn random_text(rng: &mut Rng) -> String {
    (0..rng.below(5)).map(|_| rng.pick(FRAGMENTS)).collect()
}

/// An untrusted offset: usually in range, sometimes well past the end.
fn random_offset(rng: &mut Rng, len: usize) -> usize {
    match rng.below(8) {
        0 => usize::MAX - rng.below(3),
        1 => len + rng.below(16),
        _ => rng.below(len + 1),
    }
}

fn random_range(rng: &mut Rng, len: usize) -> Range<usize> {
    // Deliberately allowed to be reversed.
    random_offset(rng, len)..random_offset(rng, len)
}

fn random_utf16_range(rng: &mut Rng, buffer: &TextBuffer) -> Option<Range<usize>> {
    let len = buffer.offset_to_utf16(buffer.len());
    (rng.below(3) != 0).then(|| random_range(rng, len))
}

fn step(rng: &mut Rng, buffer: &mut TextBuffer) -> String {
    let len = buffer.len();
    match rng.below(20) {
        0 => {
            let text = random_text(rng);
            let _ = buffer.insert(&text);
            format!("insert {text:?}")
        }
        1 => {
            let _ = buffer.backspace();
            "backspace".into()
        }
        2 => {
            let _ = buffer.delete();
            "delete".into()
        }
        3 => {
            buffer.move_left();
            "left".into()
        }
        4 => {
            buffer.move_right();
            "right".into()
        }
        5 => {
            buffer.select_left();
            "select_left".into()
        }
        6 => {
            buffer.select_right();
            "select_right".into()
        }
        7 => {
            buffer.home();
            "home".into()
        }
        8 => {
            buffer.end();
            "end".into()
        }
        9 => {
            buffer.select_all();
            "select_all".into()
        }
        10 => {
            let offset = random_offset(rng, len);
            buffer.move_to(offset);
            format!("move_to {offset}")
        }
        11 => {
            let offset = random_offset(rng, len);
            buffer.select_to(offset);
            format!("select_to {offset}")
        }
        12 => {
            let range = random_range(rng, len);
            let text = random_text(rng);
            let _ = buffer.replace_range(range.clone(), &text);
            format!("replace_range {range:?} {text:?}")
        }
        13 => {
            let range = random_utf16_range(rng, buffer);
            let text = random_text(rng);
            let _ = buffer.ime_replace(range.clone(), &text);
            format!("ime_replace {range:?} {text:?}")
        }
        14 | 15 => {
            let range = random_utf16_range(rng, buffer);
            let text = random_text(rng);
            let preedit_len = text.encode_utf16().count();
            let selected = match rng.below(4) {
                0 => None,
                1 => Some(preedit_len..preedit_len),
                _ => Some(random_range(rng, preedit_len)),
            };
            let _ = buffer.ime_replace_and_mark(range.clone(), &text, selected.clone());
            format!("ime_replace_and_mark {range:?} {text:?} {selected:?}")
        }
        16 => {
            buffer.unmark();
            "unmark".into()
        }
        17 => {
            let text = random_text(rng);
            let _ = buffer.set_text(text.clone());
            format!("set_text {text:?}")
        }
        18 => {
            let cut = buffer.cut();
            format!("cut -> {cut:?}")
        }
        _ => {
            // The read-only queries the platform makes between edits.
            let len16 = buffer.offset_to_utf16(buffer.len());
            let range = random_range(rng, len16);
            let _ = buffer.text_for_utf16_range(&range);
            let _ = buffer.selected_range_utf16();
            let _ = buffer.marked_range_utf16();
            let _ = buffer.selected_text();
            format!("queries {range:?}")
        }
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng::new(seed);
    let mut buffer = TextBuffer::new();
    let mut log = Vec::new();
    for _ in 0..steps {
        log.push(step(&mut rng, &mut buffer));
        if let Some(violation) = buffer.invariant_violation() {
            panic!("seed {seed}: {violation:?} after {log:#?}\nbuffer: {buffer:?}");
        }
        // Every range in the buffer must slice cleanly.
        let _ = &buffer.text()[buffer.selected_range()];
        if let Some(marked) = buffer.marked_range() {
            let _ = &buffer.text()[marked];
        }
    }
}

#[test]
fn random_operation_sequences_never_break_the_invariant() {
    for seed in 0..4_000 {
        run(seed, 60);
    }
}

#[test]
fn long_random_sessions_never_break_the_invariant() {
    for seed in 10_000..10_040 {
        run(seed, 2_000);
    }
}
