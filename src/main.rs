use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{SystemTime, UNIX_EPOCH},
};

use ignore::gitignore::GitignoreBuilder;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

const FORMAT_VERSION: u8 = 2;

#[derive(Debug)]
struct Options {
    pattern: String,
    output: Option<PathBuf>,
    scan_directory: Option<PathBuf>,
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
    imports: Vec<Import>,
}

#[derive(Clone, Serialize)]
struct Import {
    specifier: String,
    kind: String,
    line: usize,
}

#[derive(Serialize)]
struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Serialize)]
struct Node {
    id: String,
    kind: &'static str,
    label: String,
    match_count: Option<usize>,
}

#[derive(Serialize)]
struct Edge {
    source: String,
    target: String,
    kind: &'static str,
}

fn main() {
    if let Err(error) = run() {
        clear_status();
        eprintln!("topo: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(env::args().skip(1).collect())?;
    let workspace_directory = env::current_dir().map_err(|error| error.to_string())?;
    let scan_directory = options
        .scan_directory
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workspace_directory.join(path)
            }
        })
        .unwrap_or_else(|| workspace_directory.clone());
    let scan_directory = fs::canonicalize(&scan_directory)
        .map_err(|error| format!("could not access {}: {error}", scan_directory.display()))?;
    let repository_root = git_root(&scan_directory)?;
    let scope = scan_directory
        .strip_prefix(&repository_root)
        .map_err(|_| "the scan directory must be inside the repository root".to_owned())?;

    status("Listing tracked files");
    let tracked_files = tracked_files(&repository_root, scope)?;
    status("Filtering ignored files");
    let tracked_files = filter_ignored_files(&repository_root, tracked_files)?;
    status("Applying workspace exclusions");
    let tracked_files = filter_topoignored_files(
        &repository_root,
        &scan_directory,
        &workspace_directory,
        tracked_files,
    )?;
    status("Checking tracked files are available");
    let tracked_files = filter_available_files(&repository_root, tracked_files);

    status(&format!("Searching {} tracked files", tracked_files.len()));
    let matches = search(&repository_root, &options.pattern, &tracked_files)?;

    let mut matches_by_file: BTreeMap<String, usize> = BTreeMap::new();
    for occurrence in &matches {
        *matches_by_file.entry(occurrence.file.clone()).or_default() += 1;
    }

    status(&format!(
        "Extracting imports from {} matching files",
        matches_by_file.len()
    ));
    let files = matches_by_file
        .iter()
        .map(|(path, match_count)| FileResult {
            path: path.clone(),
            match_count: *match_count,
            imports: extract_imports(&repository_root.join(path)),
        })
        .collect::<Vec<_>>();

    status("Constructing graph");
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
            regex: options.pattern,
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

    status("Writing report");
    write_report(&output, &report)?;
    fs::write(&mermaid_output, mermaid(&report.graph))
        .map_err(|error| format!("could not write {}: {error}", mermaid_output.display()))?;

    clear_status();
    let home = env::var_os("HOME").map(PathBuf::from);
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

    let pattern = args.next().ok_or_else(usage)?;
    let mut output = None;
    let mut scan_directory = None;
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
            "--help" | "-h" => return Err(usage()),
            _ => return Err(format!("unexpected argument `{argument}`\n\n{}", usage())),
        }
    }

    Ok(Options {
        pattern,
        output,
        scan_directory,
    })
}

fn usage() -> String {
    "Usage: topo map <regex> [--dir <scan-directory>] [--output <filename>]\n\nUse the current directory as the topo workspace. Search tracked files beneath --dir (or the current directory when omitted), applying .topoignore from the workspace, and write a JSON graph report plus a Mermaid sidecar.".to_owned()
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
    let mut ignored = BTreeSet::new();
    for batch in path_batches(&files) {
        let output = Command::new("git")
            .args(["check-ignore", "--no-index", "--"])
            .args(batch)
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
    let ignore_path = workspace_directory.join(".topoignore");
    if !ignore_path.is_file() {
        return Ok(files);
    }

    let mut builder = GitignoreBuilder::new(scan_directory);
    if let Some(error) = builder.add(&ignore_path) {
        return Err(format!("could not read {}: {error}", ignore_path.display()));
    }
    let matcher = builder
        .build()
        .map_err(|error| format!("could not parse {}: {error}", ignore_path.display()))?;
    Ok(files
        .into_iter()
        .filter(|path| {
            !matcher
                .matched_path_or_any_parents(repository_root.join(path), false)
                .is_ignore()
        })
        .collect())
}

fn filter_available_files(repository_root: &Path, files: Vec<PathBuf>) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter(|path| repository_root.join(path).is_file())
        .collect()
}

fn search(repository_root: &Path, pattern: &str, files: &[PathBuf]) -> Result<Vec<Match>, String> {
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
            .args(batch)
            .current_dir(repository_root);
        let output = command
            .output()
            .map_err(|error| format!("could not run ripgrep: {error}"))?;
        validate_rg_status(&output.status, &output.stderr)?;
        matches.extend(parse_rg_output(&output.stdout)?);
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

fn extract_imports(path: &Path) -> Vec<Import> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let regexes = import_patterns(path);

    let mut imports = BTreeSet::new();
    for (index, line) in contents.lines().enumerate() {
        for (kind, regex) in &regexes {
            if let Some(captures) = regex.captures(line) {
                let specifier = captures[1].trim().to_owned();
                if !specifier.is_empty() {
                    imports.insert((specifier, (*kind).to_owned(), index + 1));
                }
            }
        }
    }
    imports
        .into_iter()
        .map(|(specifier, kind, line)| Import {
            specifier,
            kind,
            line,
        })
        .collect()
}

fn import_patterns(path: &Path) -> Vec<(&'static str, Regex)> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    let patterns: &[(&str, &str)] = match extension {
        "rs" => &[("rust_use", r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);")],
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => &[
            (
                "javascript_import",
                r#"^\s*import\s+(?:.+?\s+from\s+)?[\"']([^\"']+)[\"']"#,
            ),
            (
                "javascript_require",
                r#"\brequire\(\s*[\"']([^\"']+)[\"']\s*\)"#,
            ),
        ],
        "py" => &[
            ("python_from", r"^\s*from\s+([A-Za-z_][\w.]*)\s+import\s+"),
            (
                "python_import",
                r"^\s*import\s+([A-Za-z_][\w.]*)(?:\s*(?:as\s+\w+)?\s*(?:,|$))",
            ),
        ],
        "go" => &[("go_import", r#"^\s*(?:import\s+)?\"([^\"]+)\""#)],
        "rb" => &[(
            "ruby_require",
            r#"^\s*require(?:_relative)?\s*[\( ]\s*[\"']([^\"']+)[\"']"#,
        )],
        _ => &[],
    };
    patterns
        .iter()
        .map(|(kind, pattern)| {
            (
                *kind,
                Regex::new(pattern).expect("valid built-in import regex"),
            )
        })
        .collect()
}

fn build_graph(files: &[FileResult]) -> Graph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut import_nodes = BTreeSet::new();
    let mut edge_keys = BTreeSet::new();

    for file in files {
        nodes.push(Node {
            id: file_node_id(&file.path),
            kind: "file",
            label: file.path.clone(),
            match_count: Some(file.match_count),
        });
        for import in &file.imports {
            import_nodes.insert(import.specifier.clone());
            edge_keys.insert((file_node_id(&file.path), import_node_id(&import.specifier)));
        }
    }
    for specifier in import_nodes {
        nodes.push(Node {
            id: import_node_id(&specifier),
            kind: "import",
            label: specifier,
            match_count: None,
        });
    }
    for (source, target) in edge_keys {
        edges.push(Edge {
            source,
            target,
            kind: "imports",
        });
    }
    Graph { nodes, edges }
}

fn file_node_id(path: &str) -> String {
    format!("file:{path}")
}

fn import_node_id(specifier: &str) -> String {
    format!("import:{specifier}")
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

fn mermaid(graph: &Graph) -> String {
    let mut result = String::from("flowchart LR\n");
    let mut node_names = BTreeMap::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        let name = format!("N{index}");
        let suffix = node
            .match_count
            .map(|count| format!(" ({count} matches)"))
            .unwrap_or_default();
        let label = format!("{}{}", node.label, suffix).replace('"', "#quot;");
        result.push_str(&format!("    {name}[\"{label}\"]\n"));
        node_names.insert(&node.id, name);
    }
    for edge in &graph.edges {
        // Edges only reference nodes constructed above.
        result.push_str(&format!(
            "    {} -->|{}| {}\n",
            node_names[&edge.source], edge.kind, node_names[&edge.target]
        ));
    }
    result
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
    let first_group_length = digits.len() % 3;
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.char_indices() {
        if index != 0 && (index - first_group_length) % 3 == 0 {
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

fn status(message: &str) {
    eprint!("\r\x1b[2K{message}");
    let _ = io::stderr().flush();
}

fn clear_status() {
    eprint!("\r\x1b[2K");
    let _ = io::stderr().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_imports_for_the_file_language() {
        let directory = env::temp_dir().join("topo-imports-test");
        fs::create_dir_all(&directory).unwrap();
        let cases = [
            ("example.rs", "use std::fs;", "std::fs"),
            ("example.ts", "import x from 'package';", "package"),
            ("example.py", "from tools.util import run", "tools.util"),
        ];
        for (filename, contents, expected) in cases {
            let path = directory.join(filename);
            fs::write(&path, contents).unwrap();
            let imports = extract_imports(&path);
            assert_eq!(imports.len(), 1);
            assert_eq!(imports[0].specifier, expected);
        }
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
        assert_eq!(format_number(8143), "8,143");
    }
}
