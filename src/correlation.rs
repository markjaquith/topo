use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{FileResult, Match, PathMatch, Report};

const VERSION: u8 = 1;
const SENTINEL: &str = "{CORRELATION}";
const MIN_FUZZY_PATH_SIMILARITY: f64 = 0.8;
const MIN_FUZZY_SOURCE_SIMILARITY: f64 = 0.55;

#[derive(Debug, Serialize)]
struct CorrelationReport {
    report_type: &'static str,
    format_version: u8,
    created_at_unix_seconds: u64,
    sentinel: &'static str,
    provenance: Provenance,
    compatibility: Compatibility,
    old_report: Report,
    new_report: Report,
    entries: Vec<Entry>,
}

#[derive(Debug, Serialize)]
struct Provenance {
    old_input: String,
    new_input: String,
    limitation: &'static str,
}

#[derive(Debug, Serialize)]
struct Compatibility {
    repository_root: String,
    scan_scope: String,
    same_revision: Option<bool>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "entry_type", rename_all = "snake_case")]
enum Entry {
    FilePair {
        path: String,
        classification: Classification,
        old: Vec<Side>,
        new: Vec<Side>,
        smart_diff: Option<SmartDiff>,
        warnings: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Classification {
    Paired,
    OldOnly,
    NewOnly,
    Ambiguous,
}

#[derive(Debug, Serialize)]
struct Side {
    path: String,
    path_match_ranges: Vec<PathMatch>,
    content: Option<String>,
    matches: Vec<Match>,
    content_normalization_ranges: Vec<ContentRange>,
}

#[derive(Clone, Debug, Serialize)]
struct ContentRange {
    start_byte: usize,
    end_byte: usize,
    line: u64,
    start_column: u64,
    end_column: u64,
}

#[derive(Debug, Serialize)]
struct SmartDiff {
    classification: DiffClassification,
    lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiffClassification {
    Unchanged,
    RenameEquivalent,
    Substantive,
    Unavailable,
}

#[derive(Debug, Serialize)]
struct DiffLine {
    kind: DiffKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    old_text: Option<String>,
    new_text: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiffKind {
    Unchanged,
    RenameEquivalent,
    Addition,
    Deletion,
    Modification,
}

pub fn run(old_path: &Path, new_path: &Path, output: &Path) -> Result<(), String> {
    let old = load(old_path)?;
    let new = load(new_path)?;
    let compatibility = validate(&old, &new)?;
    let report = CorrelationReport {
        report_type: "topo_correlation",
        format_version: VERSION,
        created_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs(),
        sentinel: SENTINEL,
        provenance: Provenance {
            old_input: old_path.display().to_string(),
            new_input: new_path.display().to_string(),
            limitation: "Embedded source is authoritative scan evidence. Reports without git_revision cannot establish that scans used the same repository state.",
        },
        entries: correlate(&old, &new),
        compatibility,
        old_report: old,
        new_report: new,
    };
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(output, bytes)
        .map_err(|error| format!("could not write {}: {error}", output.display()))
}

fn load(path: &Path) -> Result<Report, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let report: Report = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "{} is not a compatible topo map report: {error}",
            path.display()
        )
    })?;
    if report.report_type != "topo_map" {
        return Err(format!("{} is not a topo map report", path.display()));
    }
    if report.format_version > crate::FORMAT_VERSION {
        return Err(format!(
            "{} uses unsupported map format version {}",
            path.display(),
            report.format_version
        ));
    }
    Ok(report)
}

fn validate(old: &Report, new: &Report) -> Result<Compatibility, String> {
    if old.metadata.repository_root != new.metadata.repository_root {
        return Err(format!(
            "reports use different repository roots: `{}` and `{}`",
            old.metadata.repository_root, new.metadata.repository_root
        ));
    }
    let old_scope = scope(old)?;
    let new_scope = scope(new)?;
    if old_scope != new_scope {
        return Err(format!(
            "reports use different scan scopes: `{old_scope}` and `{new_scope}`"
        ));
    }
    let same_revision = match (&old.metadata.git_revision, &new.metadata.git_revision) {
        (Some(old), Some(new)) => Some(old == new),
        _ => None,
    };
    let mut warnings = Vec::new();
    if same_revision == Some(false) {
        warnings.push("Scans record different Git revisions; correlation may include unrelated source changes.".to_owned());
    } else if same_revision.is_none() {
        warnings.push("At least one input lacks Git revision metadata, so equal repository state cannot be verified.".to_owned());
    }
    if old.metadata.git_dirty == Some(true) || new.metadata.git_dirty == Some(true) {
        warnings.push("At least one scan used a dirty working tree; its revision does not fully identify the source state.".to_owned());
    }
    Ok(Compatibility {
        repository_root: old.metadata.repository_root.clone(),
        scan_scope: old_scope,
        same_revision,
        warnings,
    })
}

fn scope(report: &Report) -> Result<String, String> {
    Path::new(&report.metadata.scan_directory)
        .strip_prefix(&report.metadata.repository_root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "a scan directory is outside its recorded repository root".to_owned())
}

fn correlate(old: &Report, new: &Report) -> Vec<Entry> {
    let old_by_path = old
        .files
        .iter()
        .map(|file| (&file.path, file))
        .collect::<BTreeMap<_, _>>();
    let new_by_path = new
        .files
        .iter()
        .map(|file| (&file.path, file))
        .collect::<BTreeMap<_, _>>();
    let shared_paths = old_by_path
        .keys()
        .filter(|path| new_by_path.contains_key(*path))
        .map(|path| (*path).clone())
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::new();
    let old_groups = groups(&old.files, &shared_paths);
    let new_groups = groups(&new.files, &shared_paths);
    entries.extend(
        old_groups
            .keys()
            .chain(new_groups.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|path| {
                let old_files = old_groups.get(&path).cloned().unwrap_or_default();
                let new_files = new_groups.get(&path).cloned().unwrap_or_default();
                let classification = match (old_files.len(), new_files.len()) {
                    (1, 1) => Classification::Paired,
                    (1, 0) => Classification::OldOnly,
                    (0, 1) => Classification::NewOnly,
                    _ => Classification::Ambiguous,
                };
                let old_sides = old_files
                    .into_iter()
                    .map(|file| side(file, old))
                    .collect::<Vec<_>>();
                let new_sides = new_files
                    .into_iter()
                    .map(|file| side(file, new))
                    .collect::<Vec<_>>();
                let (smart_diff, warnings) = if classification == Classification::Paired {
                    let (diff, warnings) = smart_diff(&old_sides[0], &new_sides[0]);
                    (Some(diff), warnings)
                } else {
                    (None, Vec::new())
                };
                Entry::FilePair {
                    path,
                    classification,
                    old: old_sides,
                    new: new_sides,
                    smart_diff,
                    warnings,
                }
            }),
    );
    add_fuzzy_pairs(&mut entries, old, new);
    entries.sort_by(|left, right| entry_path(left).cmp(entry_path(right)));
    entries
}

fn add_fuzzy_pairs(entries: &mut Vec<Entry>, old: &Report, new: &Report) {
    let mut unavailable_old = BTreeSet::new();
    let mut unavailable_new = BTreeSet::new();
    for entry in entries.iter() {
        match entry {
            Entry::FilePair {
                classification: Classification::Paired | Classification::Ambiguous,
                old,
                new,
                ..
            } => {
                unavailable_old.extend(old.iter().map(|side| side.path.clone()));
                unavailable_new.extend(new.iter().map(|side| side.path.clone()));
            }
            _ => {}
        }
    }

    let old_candidates = old
        .files
        .iter()
        .filter(|file| file.is_target && !unavailable_old.contains(&file.path))
        .collect::<Vec<_>>();
    let new_candidates = new
        .files
        .iter()
        .filter(|file| file.is_target && !unavailable_new.contains(&file.path))
        .collect::<Vec<_>>();
    let mut scores = Vec::new();
    for (old_index, old_file) in old_candidates.iter().enumerate() {
        for (new_index, new_file) in new_candidates.iter().enumerate() {
            if old_file.path == new_file.path {
                continue;
            }
            if !fuzzy_paths_compatible(old_file, new_file) {
                continue;
            }
            let old_side = side(old_file, old);
            let new_side = side(new_file, new);
            let Some(score) = source_similarity(&old_side, &new_side)
                .filter(|score| *score >= MIN_FUZZY_SOURCE_SIMILARITY)
            else {
                continue;
            };
            scores.push((old_index, new_index, score));
        }
    }

    let mut pairs = scores
        .iter()
        .filter(|(old_index, new_index, score)| {
            unique_best(&scores, *old_index, true) == Some((*new_index, *score))
                && unique_best(&scores, *new_index, false) == Some((*old_index, *score))
        })
        .copied()
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| {
        old_candidates[left.0]
            .path
            .cmp(&old_candidates[right.0].path)
    });

    let paired_old = pairs
        .iter()
        .map(|(old_index, _, _)| old_candidates[*old_index].path.as_str())
        .collect::<BTreeSet<_>>();
    let paired_new = pairs
        .iter()
        .map(|(_, new_index, _)| new_candidates[*new_index].path.as_str())
        .collect::<BTreeSet<_>>();
    entries.retain(|entry| match entry {
        Entry::FilePair {
            classification: Classification::OldOnly,
            old,
            ..
        } => !old
            .iter()
            .any(|side| paired_old.contains(side.path.as_str())),
        Entry::FilePair {
            classification: Classification::NewOnly,
            new,
            ..
        } => !new
            .iter()
            .any(|side| paired_new.contains(side.path.as_str())),
        _ => true,
    });
    entries.extend(pairs.into_iter().map(|(old_index, new_index, score)| {
        let old_file = old_candidates[old_index];
        let new_file = new_candidates[new_index];
        let old_side = side(old_file, old);
        let new_side = side(new_file, new);
        let (smart_diff, mut warnings) = smart_diff(&old_side, &new_side);
        warnings.insert(
            0,
            format!(
                "Paired by source similarity ({:.0}%) despite differing normalized paths.",
                score * 100.0
            ),
        );
        Entry::FilePair {
            path: normalize_path(&old_file.path, &old_file.path_match_ranges),
            classification: Classification::Paired,
            old: vec![old_side],
            new: vec![new_side],
            smart_diff: Some(smart_diff),
            warnings,
        }
    }));
}

fn fuzzy_paths_compatible(old: &FileResult, new: &FileResult) -> bool {
    if Path::new(&old.path).extension() != Path::new(&new.path).extension() {
        return false;
    }
    let old = normalize_path(&old.path, &old.path_match_ranges).replace(SENTINEL, "");
    let new = normalize_path(&new.path, &new.path_match_ranges).replace(SENTINEL, "");
    string_similarity(&old, &new) >= MIN_FUZZY_PATH_SIMILARITY
}

fn string_similarity(old: &str, new: &str) -> f64 {
    let old = old.chars().collect::<Vec<_>>();
    let new = new.chars().collect::<Vec<_>>();
    let longest = old.len().max(new.len());
    if longest == 0 {
        return 1.0;
    }
    let mut previous = (0..=new.len()).collect::<Vec<_>>();
    for (old_index, old_character) in old.iter().enumerate() {
        let mut current = vec![old_index + 1; new.len() + 1];
        for (new_index, new_character) in new.iter().enumerate() {
            current[new_index + 1] = if old_character == new_character {
                previous[new_index]
            } else {
                (previous[new_index] + 1)
                    .min(previous[new_index + 1] + 1)
                    .min(current[new_index] + 1)
            };
        }
        previous = current;
    }
    1.0 - previous[new.len()] as f64 / longest as f64
}

fn source_similarity(old: &Side, new: &Side) -> Option<f64> {
    let old = lines(&normalized(old)?);
    let new = lines(&normalized(new)?);
    let total = old.len() + new.len();
    (total > 0).then(|| 2.0 * lcs_len(&old, &new) as f64 / total as f64)
}

fn lcs_len(old: &[String], new: &[String]) -> usize {
    let mut previous = vec![0; new.len() + 1];
    for old_line in old {
        let mut current = vec![0; new.len() + 1];
        for (new_index, new_line) in new.iter().enumerate() {
            current[new_index + 1] = if old_line == new_line {
                previous[new_index] + 1
            } else {
                previous[new_index + 1].max(current[new_index])
            };
        }
        previous = current;
    }
    previous[new.len()]
}

fn unique_best(
    scores: &[(usize, usize, f64)],
    candidate_index: usize,
    old_to_new: bool,
) -> Option<(usize, f64)> {
    let mut candidates = scores
        .iter()
        .filter(|(old_index, new_index, _)| {
            if old_to_new {
                *old_index == candidate_index
            } else {
                *new_index == candidate_index
            }
        })
        .map(|(old_index, new_index, score)| {
            (if old_to_new { *new_index } else { *old_index }, *score)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    let best = candidates.first().copied()?;
    (candidates.get(1).is_none_or(|second| best.1 > second.1)).then_some(best)
}

fn entry_path(entry: &Entry) -> &str {
    match entry {
        Entry::FilePair { path, .. } => path,
    }
}

fn groups<'a>(
    files: &'a [FileResult],
    excluded_paths: &BTreeSet<String>,
) -> BTreeMap<String, Vec<&'a FileResult>> {
    let mut result = BTreeMap::<String, Vec<&FileResult>>::new();
    for file in files
        .iter()
        .filter(|file| file.is_target && !excluded_paths.contains(&file.path))
    {
        result
            .entry(normalize_path(&file.path, &file.path_match_ranges))
            .or_default()
            .push(file);
    }
    result
}

fn normalize_path(path: &str, ranges: &[PathMatch]) -> String {
    let mut by_component = BTreeMap::<usize, Vec<(usize, usize)>>::new();
    for range in ranges {
        by_component
            .entry(range.component_index)
            .or_default()
            .push((range.start, range.end));
    }
    path.split('/')
        .enumerate()
        .map(|(index, component)| {
            let Some(ranges) = by_component.get(&index) else {
                return component.to_owned();
            };
            let chars = component.chars().collect::<Vec<_>>();
            let ranges = merge(
                ranges
                    .iter()
                    .map(|(start, end)| ((*start).min(chars.len()), (*end).min(chars.len())))
                    .filter(|(start, end)| start < end)
                    .collect(),
            );
            let mut result = String::new();
            let mut position = 0;
            for (start, end) in ranges.into_iter().filter(|(start, end)| start < end) {
                result.extend(&chars[position..start]);
                result.push_str(SENTINEL);
                position = end;
            }
            result.extend(&chars[position..]);
            result
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn side(file: &FileResult, report: &Report) -> Side {
    let matches = report
        .matches
        .iter()
        .filter(|item| item.file == file.path)
        .cloned()
        .collect::<Vec<_>>();
    let content_normalization_ranges = file
        .content
        .as_deref()
        .map(|content| content_ranges(content, &matches))
        .unwrap_or_default();
    Side {
        path: file.path.clone(),
        path_match_ranges: file.path_match_ranges.clone(),
        content: file.content.clone(),
        matches,
        content_normalization_ranges,
    }
}

fn content_ranges(content: &str, matches: &[Match]) -> Vec<ContentRange> {
    let starts = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(index, _)| index + 1))
        .collect::<Vec<_>>();
    let mut result = matches
        .iter()
        .filter_map(|item| {
            let line_start = *starts.get(item.line.saturating_sub(1) as usize)?;
            let line_end = content[line_start..]
                .find('\n')
                .map(|offset| line_start + offset)
                .unwrap_or(content.len());
            let start = (line_start + item.column.saturating_sub(1) as usize).min(line_end);
            let end = (line_start + item.end_column.saturating_sub(1) as usize).min(line_end);
            (start < end && content.is_char_boundary(start) && content.is_char_boundary(end))
                .then_some(ContentRange {
                    start_byte: start,
                    end_byte: end,
                    line: item.line,
                    start_column: item.column,
                    end_column: item.end_column,
                })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|range| (range.start_byte, range.end_byte));
    result
}

fn normalized(side: &Side) -> Option<String> {
    let content = side.content.as_deref()?;
    let ranges = merge(
        side.content_normalization_ranges
            .iter()
            .map(|range| (range.start_byte, range.end_byte))
            .collect(),
    );
    let mut result = String::new();
    let mut position = 0;
    for (start, end) in ranges {
        result.push_str(&content[position..start]);
        result.push_str(SENTINEL);
        position = end;
    }
    result.push_str(&content[position..]);
    Some(result)
}

fn merge(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut result: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = result.last_mut().filter(|last| start < last.1) {
            last.1 = last.1.max(end);
        } else {
            result.push((start, end));
        }
    }
    result
}

fn smart_diff(old: &Side, new: &Side) -> (SmartDiff, Vec<String>) {
    let (Some(old_normalized), Some(new_normalized), Some(old_content), Some(new_content)) = (
        normalized(old),
        normalized(new),
        old.content.as_deref(),
        new.content.as_deref(),
    ) else {
        return (
            SmartDiff {
                classification: DiffClassification::Unavailable,
                lines: Vec::new(),
            },
            vec!["Source text is unavailable on at least one side.".to_owned()],
        );
    };
    let old_lines = lines(old_content);
    let new_lines = lines(new_content);
    let pairs = lcs(&lines(&old_normalized), &lines(&new_normalized));
    let mut result = Vec::new();
    let (mut old_start, mut new_start) = (0, 0);
    for (old_end, new_end) in pairs
        .into_iter()
        .chain(std::iter::once((old_lines.len(), new_lines.len())))
    {
        changed(
            &mut result,
            &old_lines,
            &new_lines,
            old_start,
            old_end,
            new_start,
            new_end,
        );
        if old_end < old_lines.len() {
            result.push(DiffLine {
                kind: if old_lines[old_end] == new_lines[new_end] {
                    DiffKind::Unchanged
                } else {
                    DiffKind::RenameEquivalent
                },
                old_line: Some(old_end + 1),
                new_line: Some(new_end + 1),
                old_text: Some(old_lines[old_end].clone()),
                new_text: Some(new_lines[new_end].clone()),
            });
        }
        old_start = old_end + 1;
        new_start = new_end + 1;
    }
    let classification = if result.iter().any(|line| {
        matches!(
            line.kind,
            DiffKind::Addition | DiffKind::Deletion | DiffKind::Modification
        )
    }) {
        DiffClassification::Substantive
    } else if result
        .iter()
        .any(|line| line.kind == DiffKind::RenameEquivalent)
    {
        DiffClassification::RenameEquivalent
    } else {
        DiffClassification::Unchanged
    };
    let normalized_bytes = old
        .content_normalization_ranges
        .iter()
        .chain(&new.content_normalization_ranges)
        .map(|range| range.end_byte - range.start_byte)
        .sum::<usize>();
    let warnings = if normalized_bytes * 2 > old_content.len() + new_content.len() {
        vec!["Recorded matches cover more than half of the source; a broad regex may suppress meaningful differences.".to_owned()]
    } else {
        Vec::new()
    };
    (
        SmartDiff {
            classification,
            lines: result,
        },
        warnings,
    )
}

fn lines(content: &str) -> Vec<String> {
    let normalized = content.replace("\r\n", "\n");
    let mut result = normalized
        .split('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if normalized.ends_with('\n') {
        result.pop();
    }
    result
}

fn lcs(old: &[String], new: &[String]) -> Vec<(usize, usize)> {
    let mut lengths = vec![vec![0; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lengths[i][j] = if old[i] == new[j] {
                lengths[i + 1][j + 1] + 1
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut result = Vec::new();
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            result.push((i, j));
            i += 1;
            j += 1;
        } else if lengths[i + 1][j] >= lengths[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

fn changed(
    result: &mut Vec<DiffLine>,
    old: &[String],
    new: &[String],
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
) {
    let paired = (old_end - old_start).min(new_end - new_start);
    for offset in 0..paired {
        result.push(DiffLine {
            kind: DiffKind::Modification,
            old_line: Some(old_start + offset + 1),
            new_line: Some(new_start + offset + 1),
            old_text: Some(old[old_start + offset].clone()),
            new_text: Some(new[new_start + offset].clone()),
        });
    }
    for (index, text) in old
        .iter()
        .enumerate()
        .take(old_end)
        .skip(old_start + paired)
    {
        result.push(DiffLine {
            kind: DiffKind::Deletion,
            old_line: Some(index + 1),
            new_line: None,
            old_text: Some(text.clone()),
            new_text: None,
        });
    }
    for (index, text) in new
        .iter()
        .enumerate()
        .take(new_end)
        .skip(new_start + paired)
    {
        result.push(DiffLine {
            kind: DiffKind::Addition,
            old_line: None,
            new_line: Some(index + 1),
            old_text: None,
            new_text: Some(text.clone()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_utf8_paths_and_overlapping_matches() {
        let ranges = vec![
            PathMatch {
                component_index: 0,
                start: 2,
                end: 4,
            },
            PathMatch {
                component_index: 1,
                start: 0,
                end: 3,
            },
            PathMatch {
                component_index: 1,
                start: 2,
                end: 5,
            },
        ];
        assert_eq!(
            normalize_path("mañana/OldFeature.rb", &ranges),
            "ma{CORRELATION}na/{CORRELATION}ature.rb"
        );
    }

    fn test_side(path: &str, content: &str, name: &str) -> Side {
        let start = content.find(name).unwrap();
        Side {
            path: path.to_owned(),
            path_match_ranges: Vec::new(),
            content: Some(content.to_owned()),
            matches: vec![Match {
                file: path.to_owned(),
                line: 1,
                column: start as u64 + 1,
                end_column: (start + name.len()) as u64 + 1,
                text: content.lines().next().unwrap().to_owned(),
            }],
            content_normalization_ranges: vec![ContentRange {
                start_byte: start,
                end_byte: start + name.len(),
                line: 1,
                start_column: start as u64 + 1,
                end_column: (start + name.len()) as u64 + 1,
            }],
        }
    }

    fn test_file(path: &str, content: &str, name: &str, is_target: bool) -> (FileResult, Match) {
        let content_start = content.find(name).unwrap();
        let line_start = content[..content_start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let line = content[..content_start].matches('\n').count() as u64 + 1;
        let (component_index, path_start) = path
            .split('/')
            .enumerate()
            .find_map(|(index, component)| {
                component
                    .to_lowercase()
                    .find(&name.to_lowercase())
                    .map(|start| (index, start))
            })
            .unwrap_or((path.split('/').count() - 1, 0));
        (
            FileResult {
                path: path.to_owned(),
                match_count: 1,
                is_target,
                path_match_ranges: is_target
                    .then(|| PathMatch {
                        component_index,
                        start: path_start,
                        end: path_start + name.len(),
                    })
                    .into_iter()
                    .collect(),
                content: Some(content.to_owned()),
            },
            Match {
                file: path.to_owned(),
                line,
                column: (content_start - line_start + 1) as u64,
                end_column: (content_start - line_start + name.len() + 1) as u64,
                text: content.lines().nth(line as usize - 1).unwrap().to_owned(),
            },
        )
    }

    fn test_report(regex: &str, files: Vec<(FileResult, Match)>) -> Report {
        let (files, matches): (Vec<_>, Vec<_>) = files.into_iter().unzip();
        Report {
            report_type: "topo_map".to_owned(),
            format_version: crate::FORMAT_VERSION,
            metadata: crate::Metadata {
                workspace_directory: "/repo".to_owned(),
                repository_root: "/repo".to_owned(),
                scan_directory: "/repo".to_owned(),
                regex: regex.to_owned(),
                mode: crate::MapMode::All,
                searched_at_unix_seconds: 1,
                matcher: "ripgrep".to_owned(),
                file_selection: "git".to_owned(),
                tracked_file_count: files.len(),
                git_revision: None,
                git_dirty: None,
            },
            matches,
            files,
            graph: crate::Graph { nodes: Vec::new() },
        }
    }

    #[test]
    fn smart_diff_keeps_rename_and_adjacent_changes_distinct() {
        let old = test_side("Old.rb", "class OldFeature\n  OLD = 1\nend\n", "OldFeature");
        let new = test_side("New.rb", "class NewFeature\n  NEW = 2\nend\n", "NewFeature");
        let (diff, _) = smart_diff(&old, &new);
        assert_eq!(diff.classification, DiffClassification::Substantive);
        assert!(
            diff.lines
                .iter()
                .any(|line| line.kind == DiffKind::RenameEquivalent)
        );
        assert!(
            diff.lines
                .iter()
                .any(|line| line.kind == DiffKind::Modification)
        );
    }

    #[test]
    fn broad_matches_warn_about_suppression() {
        let (_, warnings) = smart_diff(
            &test_side("old", "Old", "Old"),
            &test_side("new", "New", "New"),
        );
        assert!(!warnings.is_empty());
    }

    #[test]
    fn duplicate_normalized_keys_remain_ambiguous() {
        let files = vec![
            FileResult {
                path: "OldOne.rb".to_owned(),
                match_count: 0,
                is_target: true,
                path_match_ranges: vec![PathMatch {
                    component_index: 0,
                    start: 0,
                    end: 6,
                }],
                content: None,
            },
            FileResult {
                path: "OldTwo.rb".to_owned(),
                match_count: 0,
                is_target: true,
                path_match_ranges: vec![PathMatch {
                    component_index: 0,
                    start: 0,
                    end: 6,
                }],
                content: None,
            },
        ];
        let grouped = groups(&files, &BTreeSet::new());
        assert_eq!(grouped["{CORRELATION}.rb"].len(), 2);
    }

    #[test]
    fn adjacent_matches_each_receive_a_sentinel() {
        let ranges = vec![
            PathMatch {
                component_index: 0,
                start: 0,
                end: 3,
            },
            PathMatch {
                component_index: 0,
                start: 3,
                end: 6,
            },
        ];
        assert_eq!(
            normalize_path("OldOne.rb", &ranges),
            "{CORRELATION}{CORRELATION}.rb"
        );
    }

    #[test]
    fn shared_sprinkle_paths_are_not_correlations() {
        fn report(regex: &str, file: FileResult) -> Report {
            Report {
                report_type: "topo_map".to_owned(),
                format_version: crate::FORMAT_VERSION,
                metadata: crate::Metadata {
                    workspace_directory: "/repo".to_owned(),
                    repository_root: "/repo".to_owned(),
                    scan_directory: "/repo".to_owned(),
                    regex: regex.to_owned(),
                    mode: crate::MapMode::All,
                    searched_at_unix_seconds: 1,
                    matcher: "ripgrep".to_owned(),
                    file_selection: "git".to_owned(),
                    tracked_file_count: 1,
                    git_revision: None,
                    git_dirty: None,
                },
                matches: Vec::new(),
                files: vec![file],
                graph: crate::Graph { nodes: Vec::new() },
            }
        }
        let old = report(
            "Old",
            FileResult {
                path: "shared.rb".to_owned(),
                match_count: 0,
                is_target: true,
                path_match_ranges: vec![PathMatch {
                    component_index: 0,
                    start: 0,
                    end: 6,
                }],
                content: Some(String::new()),
            },
        );
        let new = report(
            "New",
            FileResult {
                path: "shared.rb".to_owned(),
                match_count: 0,
                is_target: false,
                path_match_ranges: Vec::new(),
                content: Some(String::new()),
            },
        );
        let entries = correlate(&old, &new);
        assert!(entries.is_empty());
    }

    #[test]
    fn identical_target_paths_are_not_correlations() {
        let path = "lib/legacy_replacement.rb";
        let old = test_report(
            "Legacy",
            vec![test_file(path, "Legacy Replacement\n", "Legacy", true)],
        );
        let new = test_report(
            "Replacement",
            vec![test_file(path, "Legacy Replacement\n", "Replacement", true)],
        );

        assert!(correlate(&old, &new).is_empty());
    }

    #[test]
    fn fuzzy_pairing_matches_unique_similar_path_targets() {
        let old_content =
            "module LegacyWidget\n  def create\n    shared_one\n    shared_two\n  end\nend\n";
        let new_content = "module ReplacementV2Widget\n  def create\n    shared_one\n    shared_two\n    new_detail\n  end\nend\n";
        let old = test_report(
            "Legacy",
            vec![test_file(
                "lib/legacy_widget/creator.rb",
                old_content,
                "Legacy",
                true,
            )],
        );
        let new = test_report(
            "Replacement",
            vec![test_file(
                "lib/replacement_v2_widget/creator.rb",
                new_content,
                "Replacement",
                true,
            )],
        );
        let old_path = normalize_path(&old.files[0].path, &old.files[0].path_match_ranges)
            .replace(SENTINEL, "");
        let new_path = normalize_path(&new.files[0].path, &new.files[0].path_match_ranges)
            .replace(SENTINEL, "");
        assert!(
            fuzzy_paths_compatible(&old.files[0], &new.files[0]),
            "{} <> {} = {}",
            old_path,
            new_path,
            string_similarity(&old_path, &new_path)
        );
        assert!(
            source_similarity(&side(&old.files[0], &old), &side(&new.files[0], &new)).unwrap()
                >= MIN_FUZZY_SOURCE_SIMILARITY
        );

        let entries = correlate(&old, &new);

        assert_eq!(entries.len(), 1);
        let Entry::FilePair {
            classification,
            old,
            new,
            warnings,
            ..
        } = &entries[0];
        assert_eq!(*classification, Classification::Paired);
        assert_eq!(old[0].path, "lib/legacy_widget/creator.rb");
        assert_eq!(new[0].path, "lib/replacement_v2_widget/creator.rb");
        assert!(warnings[0].contains("Paired by source similarity"));
    }

    #[test]
    fn fuzzy_pairing_ignores_shared_sprinkles() {
        let legacy_content =
            "module LegacyWidget\n  def create\n    shared_one\n    shared_two\n  end\nend\n";
        let replacement_content = "module ReplacementV2Widget\n  def create\n    shared_one\n    shared_two\n    LegacyFallback.call\n  end\nend\n";
        let old = test_report(
            "Legacy",
            vec![
                test_file(
                    "lib/legacy_widget/creator.rb",
                    legacy_content,
                    "Legacy",
                    true,
                ),
                test_file(
                    "lib/replacement_v2_widget/creator.rb",
                    replacement_content,
                    "Legacy",
                    false,
                ),
            ],
        );
        let new = test_report(
            "Replacement",
            vec![test_file(
                "lib/replacement_v2_widget/creator.rb",
                replacement_content,
                "Replacement",
                true,
            )],
        );

        let entries = correlate(&old, &new);

        assert_eq!(entries.len(), 1);
        assert!(matches!(
            &entries[0],
            Entry::FilePair {
                classification: Classification::Paired,
                old,
                new,
                ..
            } if old[0].path == "lib/legacy_widget/creator.rb"
                && new[0].path == "lib/replacement_v2_widget/creator.rb"
        ));
    }

    #[test]
    fn fuzzy_pairing_rejects_tied_candidates() {
        let scores = vec![(0, 0, 0.8), (0, 1, 0.8)];
        assert_eq!(unique_best(&scores, 0, true), None);
    }

    #[test]
    fn invalid_content_offsets_are_not_used_for_normalization() {
        let matches = vec![Match {
            file: "utf8.rb".to_owned(),
            line: 1,
            column: 2,
            end_column: 3,
            text: "é".to_owned(),
        }];
        assert!(content_ranges("é\n", &matches).is_empty());
    }
}
