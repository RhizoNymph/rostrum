//! Reading a GitHub Actions job log.
//!
//! The raw log is the whole job concatenated: every line prefixed with an
//! RFC 3339 timestamp, workflow commands (`##[group]`, `##[endgroup]`,
//! `##[error]`, …) marking structure, and the tools' own ANSI colours. This
//! turns it into lines a viewer can draw — timestamps, markers and escape
//! codes removed, each line classified — plus the collapsible groups, the
//! steps, and which step failed.
//!
//! Steps are not marked in the text. Each step's output starts with a group
//! headed by its command (`##[group]Run cargo test`, closed straight after
//! the command echo), and its output follows the group's end, so a step runs
//! from one `Run …` (or `Post …`) group header to the next. Everything before
//! the first is the job's set-up.

use std::collections::BTreeSet;

/// What a line is, for colouring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Plain,
    /// A `##[group]` header: the title of a collapsible section.
    GroupHeader,
    Error,
    Warning,
    Notice,
    Debug,
    /// A `[command]` echo of what the runner executed.
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    /// Line number in the full log, from 1, as GitHub's own viewer counts.
    pub number: usize,
    pub text: String,
    pub kind: LineKind,
}

/// A collapsible section: its header line and every line up to its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogGroup {
    pub title: String,
    /// Index into [`ParsedLog::lines`] of the header.
    pub header: usize,
    /// One past the last line of the group.
    pub end: usize,
}

impl LogGroup {
    /// Lines hidden when the group is collapsed: everything but the header.
    fn body(&self) -> std::ops::Range<usize> {
        self.header + 1..self.end
    }
}

/// One step of the job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogStep {
    pub title: String,
    pub start: usize,
    pub end: usize,
}

/// How much of a log to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineLimit {
    Full,
    /// The last `n` lines: the end of a failed job is where its failure is.
    Last(usize),
}

/// The default cap on lines kept before "load full".
pub const DEFAULT_LOG_LINES: usize = 20_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedLog {
    pub lines: Vec<LogLine>,
    pub groups: Vec<LogGroup>,
    pub steps: Vec<LogStep>,
    /// Index of the first error line.
    pub first_error: Option<usize>,
    /// Index into `steps` of the step holding the first error.
    pub failing_step: Option<usize>,
    /// Lines dropped from the top by a [`LineLimit::Last`].
    pub dropped: usize,
}

/// Remove ANSI escape sequences: CSI (`ESC [ … final`), OSC (`ESC ] … BEL`
/// or `ESC \`), and two-byte escapes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // Parameters and intermediates, then one final byte @..~.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Drop the leading `2026-10-02T00:06:31.5741691Z ` the runner prefixes to
/// every line. A line without one is left alone.
fn strip_timestamp(line: &str) -> &str {
    let bytes = line.as_bytes();
    let looks_dated = bytes.len() > 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[..4].iter().all(u8::is_ascii_digit);
    if !looks_dated {
        return line;
    }
    match line.find("Z ") {
        Some(end) if end < 40 => &line[end + 2..],
        _ => line.strip_suffix('Z').map_or(line, |_| ""),
    }
}

fn is_step_start(title: &str) -> bool {
    title.starts_with("Run ") || title.starts_with("Post ")
}

/// Parse a raw job log.
pub fn parse_log(raw: &str, limit: LineLimit) -> ParsedLog {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut log = ParsedLog::default();
    let mut open: Vec<usize> = Vec::new();
    let mut step_title = "Set up job".to_string();
    let mut step_start = 0;

    for (ix, raw_line) in raw.lines().enumerate() {
        let line = strip_ansi(strip_timestamp(raw_line.trim_end_matches('\r')));
        let at = log.lines.len();
        let (kind, text) = if let Some(title) = line.strip_prefix("##[group]") {
            (LineKind::GroupHeader, title.to_string())
        } else if line.starts_with("##[endgroup]") {
            if let Some(header) = open.pop() {
                let title = log.lines[header].text.clone();
                log.groups.push(LogGroup {
                    title,
                    header,
                    end: at,
                });
            }
            continue;
        } else if let Some(rest) = line.strip_prefix("##[error]") {
            (LineKind::Error, rest.to_string())
        } else if let Some(rest) = line.strip_prefix("##[warning]") {
            (LineKind::Warning, rest.to_string())
        } else if let Some(rest) = line.strip_prefix("##[notice]") {
            (LineKind::Notice, rest.to_string())
        } else if let Some(rest) = line.strip_prefix("##[debug]") {
            (LineKind::Debug, rest.to_string())
        } else if let Some(rest) = line.strip_prefix("[command]") {
            (LineKind::Command, rest.to_string())
        } else {
            (LineKind::Plain, line)
        };

        if kind == LineKind::GroupHeader {
            if open.is_empty() && is_step_start(&text) {
                if at > step_start {
                    log.steps.push(LogStep {
                        title: std::mem::take(&mut step_title),
                        start: step_start,
                        end: at,
                    });
                }
                step_title = text.clone();
                step_start = at;
            }
            open.push(at);
        }
        if kind == LineKind::Error && log.first_error.is_none() {
            log.first_error = Some(at);
        }
        log.lines.push(LogLine {
            number: ix + 1,
            text,
            kind,
        });
    }
    // A group the log never closed runs to the end.
    let total = log.lines.len();
    while let Some(header) = open.pop() {
        let title = log.lines[header].text.clone();
        log.groups.push(LogGroup {
            title,
            header,
            end: total,
        });
    }
    log.groups.sort_by_key(|group| group.header);
    if total > step_start {
        log.steps.push(LogStep {
            title: step_title,
            start: step_start,
            end: total,
        });
    }
    log.failing_step = log.first_error.and_then(|error| {
        log.steps
            .iter()
            .position(|s| (s.start..s.end).contains(&error))
    });

    match limit {
        LineLimit::Last(keep) if total > keep => log.keep_last(keep),
        _ => log,
    }
}

impl ParsedLog {
    /// Drop all but the last `keep` lines, re-indexing everything that
    /// refers to a line and clipping what straddles the cut.
    fn keep_last(mut self, keep: usize) -> Self {
        let cut = self.lines.len() - keep;
        self.lines.drain(..cut);
        self.dropped = cut;
        let shift = |ix: usize| ix.checked_sub(cut);
        self.groups = self
            .groups
            .into_iter()
            .filter_map(|group| {
                let end = shift(group.end).filter(|end| *end > 0)?;
                // A group whose header was cut keeps its tail, headless: it
                // can no longer collapse, so it is not a group any more.
                let header = shift(group.header)?;
                Some(LogGroup {
                    header,
                    end,
                    ..group
                })
            })
            .collect();
        self.steps = self
            .steps
            .into_iter()
            .filter_map(|step| {
                let end = shift(step.end).filter(|end| *end > 0)?;
                Some(LogStep {
                    start: shift(step.start).unwrap_or(0),
                    end,
                    ..step
                })
            })
            .collect();
        self.first_error = self.first_error.and_then(shift);
        let steps = &self.steps;
        self.failing_step = self
            .first_error
            .and_then(|error| steps.iter().position(|s| (s.start..s.end).contains(&error)));
        self
    }

    /// The groups to show collapsed when the log opens: every group except
    /// one holding an error, so the failure is in view and set-up noise is
    /// not.
    pub fn default_collapsed(&self) -> BTreeSet<usize> {
        self.groups
            .iter()
            .enumerate()
            .filter(|(_, group)| {
                !self.lines[group.header..group.end]
                    .iter()
                    .any(|line| line.kind == LineKind::Error)
            })
            .map(|(ix, _)| ix)
            .collect()
    }

    /// Indices of the lines to draw with `collapsed` groups folded to their
    /// headers.
    pub fn visible(&self, collapsed: &BTreeSet<usize>) -> Vec<usize> {
        let mut hidden = vec![false; self.lines.len()];
        for ix in collapsed {
            if let Some(group) = self.groups.get(*ix) {
                for line in group.body() {
                    hidden[line] = true;
                }
            }
        }
        (0..self.lines.len()).filter(|ix| !hidden[*ix]).collect()
    }

    /// The group a header line opens, if `line` is one.
    pub fn group_at(&self, line: usize) -> Option<usize> {
        self.groups.iter().position(|group| group.header == line)
    }

    /// Lines containing `query`, case-insensitively, in order. Searches every
    /// line, collapsed or not; the viewer expands a group to show a match.
    pub fn search(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.text.to_lowercase().contains(&query))
            .map(|(ix, _)| ix)
            .collect()
    }

    /// The groups that hide `line`, so a search hit can be revealed.
    pub fn groups_hiding(&self, line: usize) -> Vec<usize> {
        self.groups
            .iter()
            .enumerate()
            .filter(|(_, group)| group.body().contains(&line))
            .map(|(ix, _)| ix)
            .collect()
    }

    /// Whether `line` belongs to the failing step, for highlighting.
    pub fn in_failing_step(&self, line: usize) -> bool {
        self.failing_step
            .and_then(|ix| self.steps.get(ix))
            .is_some_and(|step| (step.start..step.end).contains(&line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}2026-10-02T00:06:31.5741691Z Current runner version: '2.337.0'\n\
2026-10-02T00:06:31.5773702Z ##[group]Runner Image\n\
2026-10-02T00:06:31.5774855Z Image: windows-2022\n\
2026-10-02T00:06:31.5775000Z ##[endgroup]\n\
2026-10-02T00:06:42.2955101Z ##[group]Run cargo build\n\
2026-10-02T00:06:42.2955200Z [command]cargo build --locked\n\
2026-10-02T00:06:42.3556400Z ##[endgroup]\n\
2026-10-02T00:06:50.0000000Z \u{1b}[1;32m   Compiling\u{1b}[0m demo v0.1.0\n\
2026-10-02T00:07:00.0000000Z ##[group]Run cargo test\n\
2026-10-02T00:07:00.0000001Z ##[endgroup]\n\
2026-10-02T00:07:30.0000000Z test tests::it_works ... \u{1b}[31mFAILED\u{1b}[0m\n\
2026-10-02T00:07:31.0000000Z ##[warning]flaky?\n\
2026-10-02T00:07:31.0000000Z ##[error]Process completed with exit code 101.\n\
2026-10-02T00:07:32.0000000Z ##[group]Post job cleanup.\n\
2026-10-02T00:07:32.0000001Z Cleaning up orphan processes\n\
2026-10-02T00:07:32.0000002Z ##[endgroup]\n";

    fn texts(log: &ParsedLog, ixs: &[usize]) -> Vec<String> {
        ixs.iter().map(|ix| log.lines[*ix].text.clone()).collect()
    }

    #[test]
    fn timestamps_bom_markers_and_ansi_are_removed() {
        let log = parse_log(SAMPLE, LineLimit::Full);
        assert_eq!(log.lines[0].text, "Current runner version: '2.337.0'");
        assert_eq!(log.lines[0].number, 1);
        assert_eq!(log.lines[1].text, "Runner Image");
        assert_eq!(log.lines[1].kind, LineKind::GroupHeader);
        let compiling = log
            .lines
            .iter()
            .find(|l| l.text.contains("Compiling"))
            .expect("line");
        assert_eq!(compiling.text, "   Compiling demo v0.1.0");
        assert!(
            log.lines
                .iter()
                .all(|l| !l.text.contains('\u{1b}') && !l.text.contains("##["))
        );
        // `##[endgroup]` lines are structure, not content.
        assert_eq!(log.lines.len(), 12);
        // Numbers still count the raw lines, endgroups included.
        assert_eq!(log.lines.last().map(|l| l.number), Some(15));
    }

    #[test]
    fn lines_are_classified() {
        let log = parse_log(SAMPLE, LineLimit::Full);
        let kind = |text: &str| log.lines.iter().find(|l| l.text == text).map(|l| l.kind);
        assert_eq!(kind("cargo build --locked"), Some(LineKind::Command));
        assert_eq!(kind("flaky?"), Some(LineKind::Warning));
        assert_eq!(
            kind("Process completed with exit code 101."),
            Some(LineKind::Error)
        );
    }

    #[test]
    fn groups_span_from_header_to_their_end() {
        let log = parse_log(SAMPLE, LineLimit::Full);
        let titles: Vec<&str> = log.groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Runner Image",
                "Run cargo build",
                "Run cargo test",
                "Post job cleanup."
            ]
        );
        assert_eq!(
            texts(
                &log,
                &(log.groups[0].header..log.groups[0].end).collect::<Vec<_>>()
            ),
            ["Runner Image", "Image: windows-2022"]
        );
    }

    /// A step's output follows its group's end, so a step runs from one
    /// `Run …` header to the next; the failure is in the step that ran.
    #[test]
    fn steps_run_header_to_header_and_the_failing_one_is_found() {
        let log = parse_log(SAMPLE, LineLimit::Full);
        let steps: Vec<&str> = log.steps.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            steps,
            [
                "Set up job",
                "Run cargo build",
                "Run cargo test",
                "Post job cleanup."
            ]
        );
        assert_eq!(log.failing_step, Some(2));
        let error = log.first_error.expect("an error");
        assert!(log.in_failing_step(error));
        let failed_test = log
            .lines
            .iter()
            .position(|l| l.text.contains("FAILED"))
            .expect("line");
        assert!(log.in_failing_step(failed_test));
        assert!(!log.in_failing_step(0));
    }

    #[test]
    fn groups_open_collapsed_except_the_one_with_the_error() {
        let mut sample = SAMPLE.replace("2026-10-02T00:07:00.0000001Z ##[endgroup]\n", "");
        sample = sample.replace(
            "2026-10-02T00:07:32.0000000Z ##[group]Post",
            "2026-10-02T00:07:31.5000000Z ##[endgroup]\n2026-10-02T00:07:32.0000000Z ##[group]Post",
        );
        let log = parse_log(&sample, LineLimit::Full);
        let collapsed = log.default_collapsed();
        let test_group = log
            .groups
            .iter()
            .position(|g| g.title == "Run cargo test")
            .expect("group");
        assert!(!collapsed.contains(&test_group));
        assert_eq!(collapsed.len(), log.groups.len() - 1);
    }

    #[test]
    fn collapsing_hides_a_groups_body_but_not_its_header() {
        let log = parse_log(SAMPLE, LineLimit::Full);
        let all = log.visible(&BTreeSet::new());
        assert_eq!(all.len(), log.lines.len());
        let folded = log.visible(&BTreeSet::from([0]));
        assert_eq!(folded.len(), log.lines.len() - 1);
        assert!(folded.contains(&log.groups[0].header));
        assert_eq!(log.group_at(log.groups[0].header), Some(0));
        assert_eq!(log.group_at(0), None);
    }

    #[test]
    fn search_is_case_insensitive_and_reveals_hidden_matches() {
        let log = parse_log(SAMPLE, LineLimit::Full);
        let hits = log.search("WINDOWS");
        assert_eq!(texts(&log, &hits), ["Image: windows-2022"]);
        assert_eq!(log.groups_hiding(hits[0]), vec![0]);
        assert!(log.search("   ").is_empty());
        assert!(log.search("absent").is_empty());
    }

    /// A long log keeps its end, where a failure is, and says how much was
    /// dropped; the structure is re-indexed to what remains.
    #[test]
    fn a_line_limit_keeps_the_tail_and_reindexes() {
        let full = parse_log(SAMPLE, LineLimit::Full);
        let tail = parse_log(SAMPLE, LineLimit::Last(6));
        assert_eq!(tail.lines.len(), 6);
        assert_eq!(tail.dropped, full.lines.len() - 6);
        assert_eq!(
            tail.lines[0].number,
            full.lines[full.lines.len() - 6].number
        );
        let error = tail.first_error.expect("error kept");
        assert_eq!(tail.lines[error].kind, LineKind::Error);
        assert!(
            tail.groups
                .iter()
                .all(|g| g.header < g.end && g.end <= tail.lines.len())
        );
        assert_eq!(
            tail.steps.last().map(|s| s.title.as_str()),
            Some("Post job cleanup.")
        );
        assert!(tail.in_failing_step(error));
        assert_eq!(parse_log(SAMPLE, LineLimit::Last(1_000)), full);
    }

    #[test]
    fn ansi_sequences_of_every_shape_are_stripped() {
        assert_eq!(strip_ansi("\u{1b}[1;31mred\u{1b}[0m plain"), "red plain");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}after"), "after");
        assert_eq!(strip_ansi("\u{1b}]8;;http://x\u{1b}\\link"), "link");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
    }

    #[test]
    fn an_unclosed_group_runs_to_the_end() {
        let log = parse_log("##[group]Open\nline\n", LineLimit::Full);
        assert_eq!(
            log.groups,
            vec![LogGroup {
                title: "Open".into(),
                header: 0,
                end: 2
            }]
        );
    }
}
