use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use ignore::gitignore::GitignoreBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const FORMAT_VERSION: u8 = 5;
static STATUS_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct Options {
    pattern: Option<String>,
    output: Option<PathBuf>,
    scan_directory: Option<PathBuf>,
    mode: Option<MapMode>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MapMode {
    #[default]
    All,
    Filenames,
    Contents,
    Sprinkles,
}

impl MapMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "all" => Ok(Self::All),
            "filenames" => Ok(Self::Filenames),
            "contents" => Ok(Self::Contents),
            "sprinkles" => Ok(Self::Sprinkles),
            _ => Err(format!(
                "unknown mode `{value}`; expected all, filenames, contents, or sprinkles"
            )),
        }
    }

    fn searches_contents(self) -> bool {
        !matches!(self, Self::Filenames)
    }

    fn checks_filenames(self) -> bool {
        !matches!(self, Self::Contents)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceConfig {
    version: u8,
    scan_dir: String,
    pattern: String,
}

#[derive(Serialize)]
struct Report {
    format_version: u8,
    metadata: Metadata,
    matches: Vec<Match>,
    files: Vec<FileResult>,
    graph: Graph,
}

#[derive(Serialize)]
struct Metadata {
    workspace_directory: String,
    repository_root: String,
    scan_directory: String,
    regex: String,
    mode: MapMode,
    searched_at_unix_seconds: u64,
    matcher: &'static str,
    file_selection: &'static str,
    tracked_file_count: usize,
}

#[derive(Serialize)]
struct Match {
    file: String,
    line: u64,
    column: u64,
    end_column: u64,
    text: String,
}

#[derive(Serialize)]
struct FileResult {
    path: String,
    match_count: usize,
    is_target: bool,
}

#[derive(Serialize)]
struct Graph {
    nodes: Vec<Node>,
}

#[derive(Serialize)]
struct Node {
    id: String,
    kind: &'static str,
    label: String,
    match_count: Option<usize>,
    is_target: bool,
}

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.is_empty()
        || matches!(arguments.first().map(String::as_str), Some("--help" | "-h"))
    {
        print!("{}", main_help());
        return;
    }
    if let Err(error) = run(arguments) {
        clear_status();
        eprintln!("topo: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let options = parse_args(arguments)?;
    let workspace_directory = env::current_dir().map_err(|error| error.to_string())?;
    let workspace_config = load_workspace_config(&workspace_directory)?;
    let pattern = options
        .pattern
        .or_else(|| {
            workspace_config
                .as_ref()
                .map(|config| config.pattern.clone())
        })
        .ok_or_else(|| {
            "no regex supplied; pass one to `topo map` or set pattern in topo.toml".to_owned()
        })?;
    let filename_pattern = compile_pattern(&pattern)?;
    let mode = options.mode.unwrap_or_default();
    let requested_scan_directory = options
        .scan_directory
        .or_else(|| {
            workspace_config
                .as_ref()
                .map(|config| PathBuf::from(&config.scan_dir))
        })
        .unwrap_or_else(|| workspace_directory.clone());
    let scan_directory = resolve_workspace_path(&requested_scan_directory, &workspace_directory)?;
    let scan_directory = fs::canonicalize(&scan_directory)
        .map_err(|error| format!("could not access {}: {error}", scan_directory.display()))?;
    let repository_root = git_root(&scan_directory)?;
    let scope = scan_directory
        .strip_prefix(&repository_root)
        .map_err(|_| "the scan directory must be inside the repository root".to_owned())?;

    let home = env::var_os("HOME").map(PathBuf::from);
    start_status_display(&format!(
        "󰗄  {}  󰉋  {}",
        pattern,
        display_path(&scan_directory, &workspace_directory, home.as_deref())
    ));
    status_working("Listing tracked files");
    let tracked_files = tracked_files(&repository_root, scope)?;
    let tracked_files = filter_ignored_files(&repository_root, tracked_files)?;
    let tracked_files = filter_topoignored_files(
        &repository_root,
        &scan_directory,
        &workspace_directory,
        tracked_files,
    )?;
    let tracked_files = filter_available_files(&repository_root, tracked_files);

    let filename_targets = if mode.checks_filenames() {
        find_filename_targets(&tracked_files, &filename_pattern)
    } else {
        BTreeSet::new()
    };
    let matches = if mode.searches_contents() {
        search(&repository_root, &pattern, &tracked_files, "Searching")?
    } else {
        Vec::new()
    };
    let content_match_counts = content_match_counts(&matches);
    let selected_files = select_files(mode, content_match_counts, &filename_targets);
    let files = collect_file_results(&selected_files);

    let graph = build_graph(&files);
    let searched_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let report = Report {
        format_version: FORMAT_VERSION,
        metadata: Metadata {
            workspace_directory: workspace_directory.display().to_string(),
            repository_root: repository_root.display().to_string(),
            scan_directory: scan_directory.display().to_string(),
            regex: pattern.clone(),
            mode,
            searched_at_unix_seconds,
            matcher: "ripgrep",
            file_selection: "git ls-files --cached, filtered through Git ignore rules, workspace .topoignore, and available working-tree files",
            tracked_file_count: tracked_files.len(),
        },
        matches,
        files,
        graph,
    };

    let output = options.output.unwrap_or_else(|| {
        workspace_directory.join(default_filename(&scan_directory, searched_at_unix_seconds))
    });
    let mermaid_output = output.with_extension("mmd");

    status_working("Writing report");
    write_report(&output, &report)?;
    fs::write(&mermaid_output, mermaid(&report.graph, mode))
        .map_err(|error| format!("could not write {}: {error}", mermaid_output.display()))?;

    clear_status();
    println!(
        "󰗄  {}  󰉋  {}",
        pattern,
        display_path(&scan_directory, &workspace_directory, home.as_deref())
    );
    println!(
        "󰈤  {} occurrences in {} files",
        format_number(report.matches.len()),
        format_number(report.files.len())
    );
    println!(
        "󰈙  {}",
        display_path(&output, &workspace_directory, home.as_deref())
    );
    println!(
        "󰈙  {}",
        display_path(&mermaid_output, &workspace_directory, home.as_deref())
    );
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("map") => {}
        Some("--help") | Some("-h") | None => return Err(usage()),
        Some(command) => return Err(format!("unknown command `{command}`\n\n{}", usage())),
    }

    let mut pattern = None;
    let mut output = None;
    let mut scan_directory = None;
    let mut mode = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--output" | "-o" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--output needs a filename".to_owned())?;
                output = Some(PathBuf::from(path));
            }
            "--dir" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--dir needs a directory".to_owned())?;
                scan_directory = Some(PathBuf::from(path));
            }
            "--mode" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--mode needs a value".to_owned())?;
                mode = Some(MapMode::parse(&value)?);
            }
            "--help" | "-h" => return Err(usage()),
            _ if argument.starts_with('-') => {
                return Err(format!("unexpected argument `{argument}`\n\n{}", usage()));
            }
            _ if pattern.is_none() => pattern = Some(argument),
            _ => return Err(format!("unexpected argument `{argument}`\n\n{}", usage())),
        }
    }

    Ok(Options {
        pattern,
        output,
        scan_directory,
        mode,
    })
}

fn main_help() -> String {
    format!(
        "████████╗ ██████╗ ██████╗  ██████╗\n╚══██╔══╝██╔═══██╗██╔══██╗██╔═══██╗\n   ██║   ██║   ██║██████╔╝██║   ██║\n   ██║   ██║   ██║██╔═══╝ ██║   ██║\n   ██║   ╚██████╔╝██║     ╚██████╔╝\n   ╚═╝    ╚═════╝ ╚═╝      ╚═════╝\n\n  Code topology maps  •  v{}\n\nUSAGE\n  topo map [<regex>] [--dir <scan-directory>] [--mode <mode>] [--output <filename>]\n\nCOMMANDS\n  map       Search Git-tracked code and write JSON + Mermaid maps\n\nWORKSPACE\n  topo map                 Use the pattern and directory in topo.toml\n  topo map 'UserService'   Override the configured regex\n\nOPTIONS\n  --mode <mode>            all (default), filenames, contents, or sprinkles\n  -h, --help               Show this help\n",
        env!("CARGO_PKG_VERSION")
    )
}

fn usage() -> String {
    "Usage: topo map [<regex>] [--dir <scan-directory>] [--mode <mode>] [--output <filename>]\n\nMode is all (default), filenames, contents, or sprinkles. Use the current directory as the topo workspace. The regex and scan directory may come from topo.toml; CLI values override them. Apply .topoignore from the workspace and write a JSON graph report plus a Mermaid sidecar.".to_owned()
}

fn load_workspace_config(workspace_directory: &Path) -> Result<Option<WorkspaceConfig>, String> {
    let config_path = workspace_directory.join("topo.toml");
    if !config_path.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("could not read {}: {error}", config_path.display()))?;
    let config: WorkspaceConfig = toml::from_str(&contents)
        .map_err(|error| format!("invalid {}: {error}", config_path.display()))?;
    if config.version != 1 {
        return Err(format!(
            "invalid {}: version must be 1",
            config_path.display()
        ));
    }
    if config.scan_dir.trim().is_empty() {
        return Err(format!(
            "invalid {}: scan_dir must not be empty",
            config_path.display()
        ));
    }
    if config.pattern.trim().is_empty() {
        return Err(format!(
            "invalid {}: pattern must not be empty",
            config_path.display()
        ));
    }
    Ok(Some(config))
}

fn resolve_workspace_path(path: &Path, workspace_directory: &Path) -> Result<PathBuf, String> {
    let path_text = path.to_str().ok_or("scan directory must be valid UTF-8")?;
    if path_text == "~" {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "cannot expand ~ because HOME is not set".to_owned());
    }
    if let Some(relative_path) = path_text.strip_prefix("~/") {
        let home = env::var_os("HOME").ok_or("cannot expand ~ because HOME is not set")?;
        return Ok(PathBuf::from(home).join(relative_path));
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(workspace_directory.join(path))
    }
}

fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|error| format!("ripgrep could not compile the regex: {error}"))
}

fn git_root(search_directory: &Path) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(search_directory)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err("not inside a Git repository; topo only maps tracked files".to_owned());
    }
    let root = String::from_utf8(output.stdout)
        .map_err(|_| "git returned a non-UTF-8 repository path".to_owned())?;
    Ok(PathBuf::from(root.trim_end()))
}

fn tracked_files(repository_root: &Path, scope: &Path) -> Result<Vec<PathBuf>, String> {
    let scope = if scope.as_os_str().is_empty() {
        "."
    } else {
        scope.to_str().ok_or("search path is not UTF-8")?
    };
    let output = Command::new("git")
        .args(["ls-files", "--cached", "-z", "--", scope])
        .current_dir(repository_root)
        .output()
        .map_err(|error| format!("could not list tracked files: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
        .collect())
}

fn filter_ignored_files(
    repository_root: &Path,
    files: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    let total = files.len();
    status_progress("Git ignores", 0, total);
    let mut processed = 0;
    let mut ignored = BTreeSet::new();
    for batch in path_batches(&files) {
        let output = Command::new("git")
            .args(["check-ignore", "--no-index", "--"])
            .args(&batch)
            .current_dir(repository_root)
            .output()
            .map_err(|error| format!("could not check ignore rules: {error}"))?;
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(format!(
                "could not apply ignore rules: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        ignored.extend(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        );
        processed += batch.len();
        status_progress("Git ignores", processed, total);
    }
    Ok(files
        .into_iter()
        .filter(|path| !ignored.contains(path))
        .collect())
}

fn filter_topoignored_files(
    repository_root: &Path,
    scan_directory: &Path,
    workspace_directory: &Path,
    files: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    let total = files.len();
    status_progress("Workspace exclusions", 0, total);
    let ignore_path = workspace_directory.join(".topoignore");
    if !ignore_path.is_file() {
        status_progress("Workspace exclusions", total, total);
        return Ok(files);
    }

    let mut builder = GitignoreBuilder::new(scan_directory);
    if let Some(error) = builder.add(&ignore_path) {
        return Err(format!("could not read {}: {error}", ignore_path.display()));
    }
    let matcher = builder
        .build()
        .map_err(|error| format!("could not parse {}: {error}", ignore_path.display()))?;
    let mut included = Vec::with_capacity(total);
    for (index, path) in files.into_iter().enumerate() {
        if !matcher
            .matched_path_or_any_parents(repository_root.join(&path), false)
            .is_ignore()
        {
            included.push(path);
        }
        if should_refresh_progress(index + 1, total) {
            status_progress("Workspace exclusions", index + 1, total);
        }
    }
    Ok(included)
}

fn filter_available_files(repository_root: &Path, files: Vec<PathBuf>) -> Vec<PathBuf> {
    let total = files.len();
    status_progress("Checking files", 0, total);
    let mut available = Vec::with_capacity(total);
    for (index, path) in files.into_iter().enumerate() {
        if repository_root.join(&path).is_file() {
            available.push(path);
        }
        if should_refresh_progress(index + 1, total) {
            status_progress("Checking files", index + 1, total);
        }
    }
    available
}

fn search(
    repository_root: &Path,
    pattern: &str,
    files: &[PathBuf],
    progress_label: &str,
) -> Result<Vec<Match>, String> {
    let total = files.len();
    status_progress(progress_label, 0, total);
    let mut processed = 0;
    let mut matches = Vec::new();
    for batch in path_batches(files) {
        let mut command = Command::new("rg");
        command
            .args([
                "--json",
                "--with-filename",
                "--line-number",
                "--column",
                "-e",
            ])
            .arg(pattern)
            .arg("--")
            .args(&batch)
            .current_dir(repository_root);
        let output = command
            .output()
            .map_err(|error| format!("could not run ripgrep: {error}"))?;
        validate_rg_status(&output.status, &output.stderr)?;
        matches.extend(parse_rg_output(&output.stdout)?);
        processed += batch.len();
        status_progress(progress_label, processed, total);
    }
    Ok(matches)
}

fn path_batches(files: &[PathBuf]) -> Vec<Vec<&PathBuf>> {
    const MAX_ARGUMENT_BYTES: usize = 100_000;
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut bytes = 0;
    for file in files {
        let file_bytes = file.as_os_str().len() + 1;
        if !batch.is_empty() && bytes + file_bytes > MAX_ARGUMENT_BYTES {
            batches.push(batch);
            batch = Vec::new();
            bytes = 0;
        }
        batch.push(file);
        bytes += file_bytes;
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

fn validate_rg_status(status: &ExitStatus, stderr: &[u8]) -> Result<(), String> {
    if status.success() || status.code() == Some(1) {
        return Ok(()); // Exit 1 means ripgrep found no matches.
    }
    let details = String::from_utf8_lossy(stderr);
    let details = details.trim();
    if details.is_empty() {
        Err(format!("ripgrep exited unsuccessfully ({status})"))
    } else {
        Err(format!(
            "ripgrep could not compile or run the regex: {details}"
        ))
    }
}

fn parse_rg_output(output: &[u8]) -> Result<Vec<Match>, String> {
    let mut matches = Vec::new();
    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| format!("could not read ripgrep output: {error}"))?;
        if value["type"] != "match" {
            continue;
        }
        let data = &value["data"];
        let file = data["path"]["text"]
            .as_str()
            .ok_or("ripgrep returned a non-text file path")?
            .to_owned();
        let line_number = data["line_number"]
            .as_u64()
            .ok_or("ripgrep omitted a line number")?;
        let text = data["lines"]["text"]
            .as_str()
            .unwrap_or("<binary line>")
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        for submatch in data["submatches"]
            .as_array()
            .ok_or("ripgrep omitted submatches")?
        {
            let start = submatch["start"]
                .as_u64()
                .ok_or("ripgrep omitted match start")?;
            let end = submatch["end"]
                .as_u64()
                .ok_or("ripgrep omitted match end")?;
            matches.push(Match {
                file: file.clone(),
                line: line_number,
                column: start + 1,
                end_column: end + 1,
                text: text.clone(),
            });
        }
    }
    Ok(matches)
}

#[derive(Clone, Copy)]
struct FileSelection {
    match_count: usize,
    is_target: bool,
}

fn find_filename_targets(files: &[PathBuf], pattern: &Regex) -> BTreeSet<String> {
    let total = files.len();
    status_progress("Finding filename targets", 0, total);
    let mut targets = BTreeSet::new();
    for (index, path) in files.iter().enumerate() {
        let filename = path
            .file_name()
            .and_then(|filename| filename.to_str())
            .unwrap_or_default();
        if pattern.is_match(filename) {
            targets.insert(path.to_string_lossy().into_owned());
        }
        if should_refresh_progress(index + 1, total) {
            status_progress("Finding filename targets", index + 1, total);
        }
    }
    targets
}

fn content_match_counts(matches: &[Match]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for occurrence in matches {
        *counts.entry(occurrence.file.clone()).or_default() += 1;
    }
    counts
}

fn select_files(
    mode: MapMode,
    content_match_counts: BTreeMap<String, usize>,
    filename_targets: &BTreeSet<String>,
) -> BTreeMap<String, FileSelection> {
    let mut selected = BTreeMap::new();
    match mode {
        MapMode::All => {
            for (path, match_count) in content_match_counts {
                selected.insert(
                    path.clone(),
                    FileSelection {
                        match_count,
                        is_target: filename_targets.contains(&path),
                    },
                );
            }
            for path in filename_targets {
                selected.entry(path.clone()).or_insert(FileSelection {
                    match_count: 0,
                    is_target: true,
                });
            }
        }
        MapMode::Filenames => {
            for path in filename_targets {
                selected.insert(
                    path.clone(),
                    FileSelection {
                        match_count: 0,
                        is_target: true,
                    },
                );
            }
        }
        MapMode::Contents => {
            for (path, match_count) in content_match_counts {
                selected.insert(
                    path,
                    FileSelection {
                        match_count,
                        is_target: false,
                    },
                );
            }
        }
        MapMode::Sprinkles => {
            for (path, match_count) in content_match_counts {
                if !filename_targets.contains(&path) {
                    selected.insert(
                        path,
                        FileSelection {
                            match_count,
                            is_target: false,
                        },
                    );
                }
            }
        }
    }
    selected
}

fn collect_file_results(selected_files: &BTreeMap<String, FileSelection>) -> Vec<FileResult> {
    let total = selected_files.len();
    status_progress("Collecting matching files", 0, total);
    let mut files = Vec::with_capacity(total);
    for (index, (path, selection)) in selected_files.iter().enumerate() {
        files.push(FileResult {
            path: path.clone(),
            match_count: selection.match_count,
            is_target: selection.is_target,
        });
        if should_refresh_progress(index + 1, total) {
            status_progress("Collecting matching files", index + 1, total);
        }
    }
    files
}

fn build_graph(files: &[FileResult]) -> Graph {
    let total = files.len();
    status_progress("Constructing graph", 0, total);
    let mut nodes = Vec::with_capacity(total);
    for (index, file) in files.iter().enumerate() {
        nodes.push(Node {
            id: file_node_id(&file.path),
            kind: "file",
            label: file.path.clone(),
            match_count: Some(file.match_count),
            is_target: file.is_target,
        });
        if should_refresh_progress(index + 1, total) {
            status_progress("Constructing graph", index + 1, total);
        }
    }
    Graph { nodes }
}

fn file_node_id(path: &str) -> String {
    format!("file:{path}")
}

fn write_report(path: &Path, report: &Report) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    fs::write(path, json).map_err(|error| format!("could not write {}: {error}", path.display()))
}

#[derive(Default)]
struct DirectoryTree<'a> {
    directories: BTreeMap<String, DirectoryTree<'a>>,
    files: Vec<&'a Node>,
}

fn mermaid(graph: &Graph, mode: MapMode) -> String {
    let mut result = String::from("flowchart LR\n");
    let node_names = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), format!("N{index}")))
        .collect::<BTreeMap<_, _>>();

    let mut directories = DirectoryTree::default();
    for node in &graph.nodes {
        insert_file_node(&mut directories, node);
    }

    let mut next_directory_id = 0;
    write_directory_tree(
        &mut result,
        &directories,
        &node_names,
        &mut next_directory_id,
        &[],
        1,
        mode,
    );
    let target_nodes = graph
        .nodes
        .iter()
        .filter(|node| mode == MapMode::All && node.is_target)
        .map(|node| node_names[&node.id].as_str())
        .collect::<Vec<_>>();
    if !target_nodes.is_empty() {
        result.push_str(
            "    classDef target fill:#FEF3C7,stroke:#D97706,stroke-width:3px,color:#451A03\n",
        );
        result.push_str(&format!("    class {} target\n", target_nodes.join(",")));
    }
    result
}

fn insert_file_node<'a>(tree: &mut DirectoryTree<'a>, node: &'a Node) {
    let path_parts = node.label.split('/').collect::<Vec<_>>();
    let (directories, _) = path_parts.split_at(path_parts.len().saturating_sub(1));
    let mut current = tree;
    for directory in directories {
        current = current
            .directories
            .entry((*directory).to_owned())
            .or_default();
    }
    current.files.push(node);
}

fn write_directory_tree(
    result: &mut String,
    tree: &DirectoryTree<'_>,
    node_names: &BTreeMap<String, String>,
    next_directory_id: &mut usize,
    directory_path: &[String],
    indent: usize,
    mode: MapMode,
) {
    for node in &tree.files {
        let filename = node.label.rsplit('/').next().unwrap_or(&node.label);
        let label = match node.match_count {
            Some(_) if mode == MapMode::Filenames => filename.to_owned(),
            Some(match_count) if mode == MapMode::All && node.is_target => {
                format!("◆ {filename} ({})", format_number(match_count))
            }
            Some(match_count) => format!("{filename} ({})", format_number(match_count)),
            None => filename.to_owned(),
        };
        write_mermaid_node(result, node, node_names, indent, &label);
    }
    for (directory, child) in &tree.directories {
        let mut child_path = directory_path.to_vec();
        child_path.push(directory.clone());
        if child.files.is_empty() {
            write_directory_tree(
                result,
                child,
                node_names,
                next_directory_id,
                &child_path,
                indent,
                mode,
            );
            continue;
        }

        let directory_id = format!("D{next_directory_id}");
        *next_directory_id += 1;
        let padding = "    ".repeat(indent);
        result.push_str(&format!(
            "{padding}subgraph {directory_id}[\"{}\"]\n",
            mermaid_label(&child_path.join("/"))
        ));
        write_directory_tree(
            result,
            child,
            node_names,
            next_directory_id,
            &child_path,
            indent + 1,
            mode,
        );
        result.push_str(&format!("{padding}end\n"));
    }
}

fn write_mermaid_node(
    result: &mut String,
    node: &Node,
    node_names: &BTreeMap<String, String>,
    indent: usize,
    label: &str,
) {
    let padding = "    ".repeat(indent);
    result.push_str(&format!(
        "{padding}{}[\"{}\"]\n",
        node_names[&node.id],
        mermaid_label(label)
    ));
}

fn mermaid_label(label: &str) -> String {
    label.replace('"', "#quot;")
}

fn display_path(path: &Path, current_directory: &Path, home: Option<&Path>) -> String {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_directory.join(path)
    };
    if absolute_path.parent() == Some(current_directory) {
        return absolute_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
    }
    if let Some(home) = home {
        if let Ok(relative_path) = absolute_path.strip_prefix(home) {
            return if relative_path.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", relative_path.display())
            };
        }
    }
    absolute_path.display().to_string()
}

fn format_number(number: usize) -> String {
    let digits = number.to_string();
    let first_group_length = match digits.len() % 3 {
        0 => 3,
        length => length,
    };
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.char_indices() {
        if index != 0 && index >= first_group_length && (index - first_group_length) % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

fn default_filename(search_directory: &Path, timestamp: u64) -> String {
    let basename = search_directory
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("topo");
    format!("{basename}.{timestamp}.topo.json")
}

fn should_refresh_progress(completed: usize, total: usize) -> bool {
    if total == 0 || completed == total {
        return true;
    }
    completed.saturating_mul(100) / total != completed.saturating_sub(1).saturating_mul(100) / total
}

fn start_status_display(header: &str) {
    if !io::stderr().is_terminal() {
        return;
    }
    STATUS_ACTIVE.store(true, Ordering::Relaxed);
    eprint!("\x1b[?25l\r\x1b[2K{header}\n");
    let _ = io::stderr().flush();
}

fn status_working(message: &str) {
    status(&format!("󰄬  {message}"));
}

fn status_progress(label: &str, completed: usize, total: usize) {
    const BAR_WIDTH: usize = 16;
    let completed = completed.min(total);
    let (percentage, filled) = if total == 0 {
        (100, BAR_WIDTH)
    } else {
        (completed * 100 / total, completed * BAR_WIDTH / total)
    };
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled));
    let unit = if total == 1 { "file" } else { "files" };
    status(&format!(
        "󰄬  {label}  {bar}  {percentage:>3}%  {} / {} {unit}",
        format_number(completed),
        format_number(total),
    ));
}

fn status(message: &str) {
    eprint!("\r\x1b[2K{message}");
    let _ = io::stderr().flush();
}

fn clear_status() {
    eprint!("\r\x1b[2K");
    if STATUS_ACTIVE.swap(false, Ordering::Relaxed) {
        eprint!("\x1b[1A\r\x1b[2K\x1b[?25h");
    }
    let _ = io::stderr().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_help_introduces_topo() {
        let help = main_help();
        assert!(help.starts_with("████████╗"));
        assert!(help.contains("Code topology maps"));
        assert!(help.contains("topo map [<regex>]"));
    }

    #[test]
    fn workspace_config_is_strictly_validated() {
        let directory = env::temp_dir().join(format!("topo-config-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let config_path = directory.join("topo.toml");
        fs::write(
            &config_path,
            "version = 1\nscan_dir = \"~/workspace/example\"\npattern = \"Example\"\n",
        )
        .unwrap();
        let config = load_workspace_config(&directory).unwrap().unwrap();
        assert_eq!(config.scan_dir, "~/workspace/example");
        assert_eq!(config.pattern, "Example");

        fs::write(
            &config_path,
            "version = 1\nscan_dir = \"~/workspace/example\"\npattern = \"Example\"\nignore = \"db/migrate\"\n",
        )
        .unwrap();
        assert!(
            load_workspace_config(&directory)
                .unwrap_err()
                .contains("unknown field")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn workspace_topoignore_filters_scan_paths() {
        let directory = env::temp_dir().join(format!("topo-ignore-test-{}", std::process::id()));
        let repository_root = directory.join("repository");
        let scan_directory = repository_root.join("component");
        let workspace_directory = directory.join("workspace");
        fs::create_dir_all(&scan_directory).unwrap();
        fs::create_dir_all(&workspace_directory).unwrap();
        fs::write(
            workspace_directory.join(".topoignore"),
            "db/migrate/\n**/schema.rb\n",
        )
        .unwrap();

        let files = filter_topoignored_files(
            &repository_root,
            &scan_directory,
            &workspace_directory,
            vec![
                PathBuf::from("component/app/service.rb"),
                PathBuf::from("component/db/migrate/001_create_users.rb"),
                PathBuf::from("component/db/schema.rb"),
            ],
        )
        .unwrap();

        fs::remove_dir_all(directory).unwrap();
        assert_eq!(files, vec![PathBuf::from("component/app/service.rb")]);
    }

    #[test]
    fn default_output_uses_directory_basename() {
        assert_eq!(
            default_filename(Path::new("/work/topo"), 42),
            "topo.42.topo.json"
        );
    }

    #[test]
    fn map_modes_select_targets_and_sprinkles() {
        let content_matches = BTreeMap::from([
            ("airwallex_client.rb".to_owned(), 3),
            ("payments_service.rb".to_owned(), 2),
        ]);
        let targets = BTreeSet::from([
            "airwallex_client.rb".to_owned(),
            "airwallex_webhook.rb".to_owned(),
        ]);

        let all = select_files(MapMode::All, content_matches.clone(), &targets);
        assert_eq!(all.len(), 3);
        assert_eq!(all["airwallex_client.rb"].match_count, 3);
        assert!(all["airwallex_client.rb"].is_target);
        assert_eq!(all["airwallex_webhook.rb"].match_count, 0);

        let filenames = select_files(MapMode::Filenames, content_matches.clone(), &targets);
        assert_eq!(filenames.len(), 2);
        assert!(filenames.values().all(|selection| selection.is_target));
        assert!(
            filenames
                .values()
                .all(|selection| selection.match_count == 0)
        );

        let contents = select_files(MapMode::Contents, content_matches.clone(), &targets);
        assert_eq!(contents.len(), 2);
        assert!(contents.values().all(|selection| !selection.is_target));

        let sprinkles = select_files(MapMode::Sprinkles, content_matches, &targets);
        assert_eq!(sprinkles.len(), 1);
        assert_eq!(sprinkles["payments_service.rb"].match_count, 2);
    }

    #[test]
    fn mermaid_groups_only_direct_file_directories_with_full_paths() {
        let graph = Graph {
            nodes: vec![
                Node {
                    id: "file:packs/payments/app/client.rb".to_owned(),
                    kind: "file",
                    label: "packs/payments/app/client.rb".to_owned(),
                    match_count: Some(1),
                    is_target: true,
                },
                Node {
                    id: "file:packs/payments/lib/helper.rb".to_owned(),
                    kind: "file",
                    label: "packs/payments/lib/helper.rb".to_owned(),
                    match_count: Some(1),
                    is_target: false,
                },
                Node {
                    id: "file:src/main.rs".to_owned(),
                    kind: "file",
                    label: "src/main.rs".to_owned(),
                    match_count: Some(1),
                    is_target: false,
                },
            ],
        };

        let diagram = mermaid(&graph, MapMode::All);
        assert!(!diagram.contains("subgraph D0[\"packs\"]"));
        assert!(diagram.contains("subgraph D0[\"packs/payments/app\"]"));
        assert!(diagram.contains("subgraph D1[\"packs/payments/lib\"]"));
        assert!(diagram.contains("subgraph D2[\"src\"]"));
        assert!(diagram.contains("N0[\"◆ client.rb (1)\"]"));
        assert!(diagram.contains(
            "classDef target fill:#FEF3C7,stroke:#D97706,stroke-width:3px,color:#451A03"
        ));
        assert!(diagram.contains("class N0 target"));
        assert!(!diagram.contains("-->"));

        let filename_diagram = mermaid(&graph, MapMode::Filenames);
        assert!(filename_diagram.contains("N0[\"client.rb\"]"));
        assert!(!filename_diagram.contains("◆"));
        assert!(!filename_diagram.contains("(1)"));
        assert!(!filename_diagram.contains("classDef target"));
    }

    #[test]
    fn progress_refreshes_at_percentage_boundaries() {
        assert!(should_refresh_progress(0, 0));
        assert!(!should_refresh_progress(1, 200));
        assert!(should_refresh_progress(2, 200));
        assert!(should_refresh_progress(200, 200));
    }

    #[test]
    fn formats_summary_paths_compactly() {
        let current_directory = Path::new("/Users/test/workspace/project");
        assert_eq!(
            display_path(
                Path::new("project.42.topo.json"),
                current_directory,
                Some(Path::new("/Users/test"))
            ),
            "project.42.topo.json"
        );
        assert_eq!(
            display_path(
                Path::new("/Users/test/elsewhere/report.topo.json"),
                current_directory,
                Some(Path::new("/Users/test"))
            ),
            "~/elsewhere/report.topo.json"
        );
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(8_143), "8,143");
        assert_eq!(format_number(61_028), "61,028");
        assert_eq!(format_number(1_610_028), "1,610,028");
    }
}
