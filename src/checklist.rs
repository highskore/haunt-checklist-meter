//! Markdown checkbox counting and which file a tab's meter follows, free of host calls.

/// Ticked and total `- [ ]` / `- [x]` items (also `*`, `+` and numbered lists), outside code fences.
pub fn count(markdown: &str) -> (u32, u32) {
    let (mut done, mut total, mut fence) = (0u32, 0u32, None::<&str>);
    for line in markdown.lines() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(&t[..3]);
            continue;
        }
        let rest = if let Some(r) = t.strip_prefix(['-', '*', '+']) {
            r
        } else {
            let digits = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
            match t[digits..].strip_prefix(['.', ')']) {
                Some(r) if digits > 0 => r,
                _ => continue,
            }
        };
        let Some(rest) = rest.strip_prefix(' ') else { continue };
        let b = rest.trim_start().as_bytes();
        if b.len() >= 3 && b[0] == b'[' && b[2] == b']' && (b.len() == 3 || b[3] == b' ') {
            match b[1] {
                b' ' => total += 1,
                b'x' | b'X' => {
                    total += 1;
                    done += 1;
                }
                _ => {}
            }
        }
    }
    (done, total)
}

/// A file pattern relative to the tab's folder: one `*` within the last segment at most.
pub fn matches(pattern: &str, rel: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == rel,
        Some((pre, post)) => rel.len() >= pre.len() + post.len() && rel.starts_with(pre) && rel.ends_with(post) && !rel[pre.len()..rel.len() - post.len()].contains('/'),
    }
}

/// `path` relative to `cwd`, if inside it, with no `.` or `..` parts.
pub fn relative<'a>(cwd: &str, path: &'a str) -> Option<&'a str> {
    let rel = path.strip_prefix(cwd.trim_end_matches('/'))?.strip_prefix('/')?;
    (!rel.is_empty() && rel.split('/').all(|p| !p.is_empty() && p != "." && p != "..")).then_some(rel)
}

/// The row's label word: the first folder of the file, singular (`specs/plan.md` → `spec`), else its stem
/// (`TODO.md` → `todo`).
pub fn label_word(rel: &str) -> String {
    let word = match rel.split_once('/') {
        Some((dir, _)) => dir.strip_suffix('s').unwrap_or(dir),
        None => rel.rsplit_once('.').map_or(rel, |(s, _)| s),
    };
    word.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(12).collect::<String>().to_lowercase()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    pub rel: String,
    pub done: u32,
    pub total: u32,
}

impl Meter {
    pub fn complete(&self) -> bool {
        self.total > 0 && self.done == self.total
    }

    /// `spec 11/18`, and `spec 18/18 ✓` once every box is ticked.
    pub fn label(&self) -> String {
        let tick = if self.complete() { " ✓" } else { "" };
        format!("{} {}/{}{tick}", label_word(&self.rel), self.done, self.total)
    }

    /// The bar under the row; none once complete.
    pub fn bar(&self) -> Option<f32> {
        (self.total > 0 && !self.complete()).then(|| self.done as f32 / self.total as f32)
    }
}

/// Which file to show: the one the agent touched last if it has a checklist, else the first candidate
/// with unticked items, else the first with any.
pub fn pick(candidates: &[(String, (u32, u32))], last_touched: Option<&str>) -> Option<Meter> {
    let meter = |(rel, (done, total)): &(String, (u32, u32))| Meter { rel: rel.clone(), done: *done, total: *total };
    let has = |c: &&(String, (u32, u32))| c.1.1 > 0;
    last_touched
        .and_then(|t| candidates.iter().filter(has).find(|c| c.0 == t))
        .or_else(|| candidates.iter().filter(has).find(|c| c.1.0 < c.1.1))
        .or_else(|| candidates.iter().find(has))
        .map(meter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_checkboxes_in_any_list_style() {
        let md = "# Plan\n- [x] one\n- [ ] two\n  * [X] nested\n+ [ ] plus\n1. [x] numbered\n2) [ ] paren\n- [] not one\n- [y] not one\n-[ ] not one\n- plain\n";
        assert_eq!(count(md), (3, 6));
    }

    #[test]
    fn code_fences_are_skipped() {
        assert_eq!(count("```md\n- [ ] example\n```\n- [x] real\n~~~\n- [ ] x\n~~~"), (1, 1));
    }

    #[test]
    fn checkbox_needs_a_space_after_it_or_end_of_line() {
        assert_eq!(count("- [x]done\n- [x]\n- [ ] ok"), (1, 2));
    }

    #[test]
    fn patterns_match_one_segment() {
        assert!(matches("specs/*.md", "specs/plan.md"));
        assert!(!matches("specs/*.md", "specs/old/plan.md"));
        assert!(!matches("specs/*.md", "specs/plan.txt"));
        assert!(matches("TODO.md", "TODO.md") && !matches("TODO.md", "a/TODO.md"));
    }

    #[test]
    fn relative_paths_stay_inside_the_folder() {
        assert_eq!(relative("/w/haunt", "/w/haunt/specs/plan.md"), Some("specs/plan.md"));
        assert_eq!(relative("/w/haunt/", "/w/haunt/TODO.md"), Some("TODO.md"));
        assert_eq!(relative("/w/haunt", "/w/haunt2/TODO.md"), None);
        assert_eq!(relative("/w/haunt", "/w/haunt/../x.md"), None);
        assert_eq!(relative("/w/haunt", "/w/haunt"), None);
    }

    #[test]
    fn labels_read_like_the_mockup() {
        assert_eq!(Meter { rel: "specs/plan.md".into(), done: 11, total: 18 }.label(), "spec 11/18");
        assert_eq!(label_word("TODO.md"), "todo");
        assert_eq!(label_word("docs/x.md"), "doc");
        assert!((Meter { rel: "a".into(), done: 11, total: 18 }.bar().unwrap() - 0.611).abs() < 0.01);
        let done = Meter { rel: "specs/plan.md".into(), done: 18, total: 18 };
        assert_eq!((done.label().as_str(), done.bar()), ("spec 18/18 ✓", None));
    }

    #[test]
    fn picks_the_touched_file_then_unfinished_then_any() {
        let c = vec![("specs/a.md".to_owned(), (3, 3)), ("specs/b.md".to_owned(), (1, 4)), ("TODO.md".to_owned(), (0, 0))];
        assert_eq!(pick(&c, Some("specs/a.md")).unwrap().rel, "specs/a.md");
        assert_eq!(pick(&c, None).unwrap().rel, "specs/b.md");
        assert_eq!(pick(&c, Some("TODO.md")).unwrap().rel, "specs/b.md", "a touched file without boxes is skipped");
        assert_eq!(pick(&c[..1], None).unwrap().rel, "specs/a.md");
        assert_eq!(pick(&c[2..], None), None);
    }
}
