use super::*;

fn buffer(text: &str) -> TextBuffer {
    let mut buffer = TextBuffer::new();
    let _ = buffer.set_text(text);
    buffer
}

/// A reversed range, as the platform IME may send; spelled out so clippy
/// does not take it for a mistake.
fn reversed(start: usize, end: usize) -> Range<usize> {
    Range { start, end }
}

fn assert_valid(buffer: &TextBuffer) {
    assert_eq!(buffer.invariant_violation(), None, "{buffer:?}");
}

// --- regressions -------------------------------------------------------------
//
// Each of these panicked on the pre-fix `TextInput` logic (a verbatim gpui-free
// port of it was run to confirm; the message it produced is quoted).

/// The IME's new selection is relative to the pre-edit string, but it used to
/// be converted against the whole text and then offset by the insertion
/// point. "—abc" + mark "x" with the caret at 1 put the selection at
/// 6 + utf16→utf8(1 in "—abcx") = 6 + 3 = 9 in a 7-byte string. Once the
/// composition was unmarked, the next keystroke sliced it: "end byte index 9 is
/// out of bounds for string of length 7".
#[test]
fn ime_selection_is_relative_to_the_preedit_text() {
    let mut buffer = buffer("—abc");
    let _ = buffer.ime_replace_and_mark(None, "x", Some(1..1));
    assert_eq!(buffer.text(), "—abcx");
    assert_eq!(buffer.marked_range(), Some(6..7));
    assert_eq!(buffer.selected_range(), 7..7);
    buffer.unmark();
    let _ = buffer.ime_replace(None, "y");
    assert_eq!(buffer.text(), "—abcxy");
    assert_valid(&buffer);
}

/// The same defect landing inside a character: "—" + mark "éé" with the caret
/// after the first "é" gave 3 + 3 = 6, the middle of the second "é": "end byte
/// index 6 is not a char boundary; it is inside 'é' (bytes 5..7 of string)".
#[test]
fn ime_selection_inside_preedit_lands_on_a_char_boundary() {
    let mut buffer = buffer("—");
    let _ = buffer.ime_replace_and_mark(None, "éé", Some(1..1));
    assert_eq!(buffer.selected_range(), 5..5);
    buffer.unmark();
    let _ = buffer.ime_replace(None, "y");
    assert_eq!(buffer.text(), "—éyé");
    assert_valid(&buffer);
}

#[test]
fn backspace_after_ime_mark_with_a_caret_does_not_panic() {
    let mut buffer = buffer("—abc");
    let _ = buffer.ime_replace_and_mark(None, "x", Some(1..1));
    let _ = buffer.backspace();
    assert_eq!(buffer.text(), "—abc");
    assert_valid(&buffer);
}

/// Clicking an empty input hit-tested against the *placeholder's* layout and
/// put the caret up to the placeholder's length into an empty string; the
/// first keystroke then panicked: "end byte index 18 is out of bounds for
/// string of length 0".
#[test]
fn an_offset_from_the_placeholder_layout_is_clamped() {
    let mut buffer = buffer("");
    buffer.move_to("Leave a comment…".len());
    assert_eq!(buffer.selected_range(), 0..0);
    let _ = buffer.insert("a");
    assert_eq!(buffer.text(), "a");
}

/// A layout shaped before the last edit can map Up/Down or a mouse position to
/// an offset inside a character of the current text: "end byte index 2 is not
/// a char boundary; it is inside '—' (bytes 1..4 of string)".
#[test]
fn a_stale_offset_inside_a_character_is_snapped_back() {
    let mut buffer = buffer("a—b");
    buffer.move_to(2);
    assert_eq!(buffer.selected_range(), 1..1);
    let _ = buffer.insert("x");
    assert_eq!(buffer.text(), "ax—b");
}

/// ...or past the end, after which Home sliced it: "end byte index 7 is out of
/// bounds for string of length 2".
#[test]
fn a_stale_offset_past_the_end_is_clamped() {
    let mut buffer = buffer("ab");
    buffer.move_to(7);
    buffer.home();
    assert_eq!(buffer.selected_range(), 0..0);
    buffer.select_to(99);
    assert_eq!(buffer.selected_range(), 0..2);
    buffer.end();
    assert_eq!(buffer.selected_range(), 2..2);
}

/// A reversed IME range did not panic but duplicated text: replacing 4..1 in
/// "abcdef" with "X" produced "abcdXbcdef".
#[test]
fn a_reversed_ime_range_is_normalised() {
    let mut buffer = buffer("abcdef");
    let _ = buffer.ime_replace(Some(reversed(4, 1)), "X");
    assert_eq!(buffer.text(), "aXef");
    assert_eq!(buffer.selected_range(), 2..2);
}

#[test]
fn a_reversed_ime_selection_is_normalised() {
    let mut buffer = buffer("");
    let _ = buffer.ime_replace_and_mark(None, "abc", Some(reversed(3, 1)));
    assert_eq!(buffer.selected_range(), 1..3);
    assert_valid(&buffer);
}

#[test]
fn an_ime_selection_past_the_preedit_is_clamped_to_it() {
    let mut buffer = buffer("ab");
    let _ = buffer.ime_replace_and_mark(Some(0..0), "é", Some(5..9));
    assert_eq!(buffer.text(), "éab");
    assert_eq!(buffer.selected_range(), 2..2);
}

#[test]
fn an_ime_range_past_the_end_is_clamped() {
    let mut buffer = buffer("ab");
    let _ = buffer.ime_replace(Some(10..20), "c");
    assert_eq!(buffer.text(), "abc");
    let _ = buffer.ime_replace_and_mark(Some(reversed(50, 1)), "z", None);
    assert_eq!(buffer.text(), "az");
    assert_eq!(buffer.marked_range(), Some(1..2));
}

// --- IME --------------------------------------------------------------------

#[test]
fn composition_marks_then_commits_over_the_mark() {
    let mut buffer = buffer("—");
    let _ = buffer.ime_replace_and_mark(None, "n", Some(1..1));
    let _ = buffer.ime_replace_and_mark(None, "ni", Some(2..2));
    assert_eq!(buffer.text(), "—ni");
    assert_eq!(buffer.marked_range(), Some(3..5));
    assert_eq!(buffer.marked_range_utf16(), Some(1..3));
    assert_eq!(buffer.selected_range_utf16(), 3..3);
    let _ = buffer.ime_replace(None, "你");
    assert_eq!(buffer.text(), "—你");
    assert_eq!(buffer.marked_range(), None);
    assert_eq!(buffer.selected_range(), 6..6);
}

#[test]
fn an_empty_preedit_clears_the_mark() {
    let mut buffer = buffer("a");
    let _ = buffer.ime_replace_and_mark(None, "b", None);
    let _ = buffer.ime_replace_and_mark(None, "", None);
    assert_eq!(buffer.text(), "a");
    assert_eq!(buffer.marked_range(), None);
    assert_eq!(buffer.selected_range(), 1..1);
}

#[test]
fn utf16_offsets_round_trip_across_surrogate_pairs() {
    let buffer = buffer("a😀b");
    assert_eq!(buffer.offset_from_utf16(1), 1);
    // Inside the surrogate pair rounds up past the emoji.
    assert_eq!(buffer.offset_from_utf16(2), 5);
    assert_eq!(buffer.offset_from_utf16(3), 5);
    assert_eq!(buffer.offset_from_utf16(4), 6);
    assert_eq!(buffer.offset_from_utf16(400), 6);
    assert_eq!(buffer.offset_to_utf16(5), 3);
    assert_eq!(buffer.offset_to_utf16(3), 3);
    assert_eq!(buffer.offset_to_utf16(600), 4);
}

#[test]
fn text_for_a_utf16_range_is_clamped_and_ordered() {
    let buffer = buffer("a😀b");
    assert_eq!(
        buffer.text_for_utf16_range(&(1..3)),
        ("😀".to_owned(), 1..3)
    );
    assert_eq!(
        buffer.text_for_utf16_range(&reversed(4, 0)),
        ("a😀b".to_owned(), 0..4)
    );
    assert_eq!(buffer.text_for_utf16_range(&(9..12)), (String::new(), 4..4));
}

// --- editing ----------------------------------------------------------------

#[test]
fn backspace_and_delete_remove_whole_graphemes() {
    let mut buffer = buffer("e\u{301}👍🏽x");
    buffer.move_to("e\u{301}👍🏽".len());
    let _ = buffer.backspace();
    assert_eq!(buffer.text(), "e\u{301}x");
    buffer.move_to(0);
    let _ = buffer.delete();
    assert_eq!(buffer.text(), "x");
}

#[test]
fn backspace_at_the_start_changes_nothing() {
    let mut buffer = buffer("ab");
    buffer.move_to(0);
    let revision = buffer.revision();
    assert_eq!(buffer.backspace(), Edit::Unchanged);
    assert_eq!(buffer.revision(), revision);
    buffer.move_to(2);
    assert_eq!(buffer.delete(), Edit::Unchanged);
}

#[test]
fn every_content_change_bumps_the_revision() {
    let mut buffer = buffer("ab");
    let before = buffer.revision();
    let _ = buffer.insert("c");
    assert_ne!(buffer.revision(), before);
    let before = buffer.revision();
    let _ = buffer.ime_replace_and_mark(None, "d", None);
    assert_ne!(buffer.revision(), before);
    let before = buffer.revision();
    buffer.move_to(0);
    buffer.select_all();
    assert_eq!(buffer.revision(), before);
}

#[test]
fn insert_replaces_the_selection() {
    let mut buffer = buffer("hello world");
    buffer.move_to(0);
    buffer.select_to(5);
    let _ = buffer.insert("bye");
    assert_eq!(buffer.text(), "bye world");
    assert_eq!(buffer.selected_range(), 3..3);
}

#[test]
fn cut_returns_and_removes_the_selection() {
    let mut buffer = buffer("a—b");
    assert_eq!(buffer.cut(), None);
    buffer.move_to(1);
    buffer.select_right();
    assert_eq!(buffer.cut().as_deref(), Some("—"));
    assert_eq!(buffer.text(), "ab");
}

// --- movement ---------------------------------------------------------------

#[test]
fn left_and_right_step_over_graphemes() {
    let mut buffer = buffer("a👍🏽b");
    buffer.move_left();
    assert_eq!(buffer.cursor(), "a👍🏽".len());
    buffer.move_left();
    assert_eq!(buffer.cursor(), 1);
    buffer.move_right();
    assert_eq!(buffer.cursor(), "a👍🏽".len());
}

#[test]
fn select_left_past_the_anchor_reverses_the_selection() {
    let mut buffer = buffer("abc");
    buffer.move_to(1);
    buffer.select_right();
    assert_eq!(
        (buffer.selected_range(), buffer.is_reversed()),
        (1..2, false)
    );
    buffer.select_left();
    buffer.select_left();
    assert_eq!(
        (buffer.selected_range(), buffer.is_reversed()),
        (0..1, true)
    );
    assert_eq!(buffer.cursor(), 0);
}

#[test]
fn left_and_right_collapse_a_selection_to_its_ends() {
    let mut buffer = buffer("abcd");
    buffer.move_to(1);
    buffer.select_to(3);
    buffer.move_left();
    assert_eq!(buffer.selected_range(), 1..1);
    buffer.select_to(3);
    buffer.move_right();
    assert_eq!(buffer.selected_range(), 3..3);
}

#[test]
fn home_and_end_stay_on_the_logical_line() {
    let mut buffer = buffer("one\ntwo—\nthree");
    buffer.move_to(5);
    buffer.home();
    assert_eq!(buffer.cursor(), 4);
    buffer.end();
    assert_eq!(buffer.cursor(), "one\ntwo—".len());
}

#[test]
fn end_stops_before_a_crlf() {
    let mut buffer = buffer("ab\r\ncd");
    buffer.move_to(0);
    buffer.end();
    assert_eq!(buffer.cursor(), 2);
    buffer.move_to(5);
    buffer.home();
    assert_eq!(buffer.cursor(), 4);
    buffer.end();
    assert_eq!(buffer.cursor(), 6);
}

#[test]
fn select_all_spans_the_text() {
    let mut buffer = buffer("a—");
    buffer.select_all();
    assert_eq!(buffer.selected_text(), "a—");
}

#[test]
fn set_text_resets_selection_and_mark() {
    let mut buffer = buffer("abc");
    let _ = buffer.ime_replace_and_mark(None, "x", None);
    let _ = buffer.set_text("é");
    assert_eq!(buffer.selected_range(), 2..2);
    assert_eq!(buffer.marked_range(), None);
}

#[test]
fn floor_char_boundary_clamps_and_snaps_back() {
    assert_eq!(floor_char_boundary("a—", 2), 1);
    assert_eq!(floor_char_boundary("a—", 3), 1);
    assert_eq!(floor_char_boundary("a—", 4), 4);
    assert_eq!(floor_char_boundary("a—", 40), 4);
    assert_eq!(floor_char_boundary("", 3), 0);
}
