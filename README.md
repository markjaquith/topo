# topo

```sh
cargo install topo-scan
```

`topo` keeps its configuration and generated maps in a **workspace**: the
current directory where you run it. The scanned repository can live elsewhere.

```sh
mkdir -p ~/Dev/topo-workspace
cd ~/Dev/topo-workspace
topo map
```

## Workspace configuration

A workspace `topo.toml` specifies the default repository and regex:

```toml
version = 1
scan_dir = "~/Dev/my-project"
pattern = "UserService"
```

The schema is strict: all three keys are required, `version` must be `1`, and
unknown keys are rejected. `scan_dir` accepts a leading `~`, must resolve to a
directory, and must be inside a Git repository. `pattern` must be a non-empty
regex ripgrep can compile.

`topo map` uses these values. A positional regex and `--dir` are optional CLI
overrides:

```sh
topo map 'AdminUser' --dir ~/Dev/my-project
```

## Map modes

`--mode` controls whether the regex selects path components, contents, or both:

| Mode | Selected files |
| --- | --- |
| `all` (default) | Matching paths and content matches |
| `paths` | Matching path components only; skips the content search |
| `contents` | Content matches only; paths are not considered |
| `sprinkles` | Content matches whose path does not match |

`filenames` remains accepted as a compatibility alias for `paths`.

Without `topo.toml`, topo preserves the original behavior: the positional regex
is required and the current directory is scanned. Topo updates one stderr
progress line for each phase; file-based phases show completed files and a
progress bar.

## Workspace exclusions

A workspace `.topoignore` uses standard `.gitignore` patterns, interpreted
relative to the scan directory. It is applied after Git ignore rules, so it can
remove topology noise without modifying the scanned repository.

```gitignore
# Paths in the scan directory to omit from maps
db/migrate/
db/schema.rb
**/spec/fixtures/
```

The report JSON contains the workspace directory, scan directory, repository
root, regex, timestamp, every occurrence, matching files, and directory graph
nodes. Each matching file has an `is_target` flag when its basename matches the
regex and includes its complete UTF-8 source text (non-text files omit source
text). New reports also include `report_type: "topo_map"`, the current Git
revision, and whether tracked files were dirty. Older map reports remain valid;
they embed source evidence but cannot prove which Git revision it came from.
Without `--output`, the report is named
`<scan-dir-basename>.<unix-timestamp>.topo.json` in the workspace.

## Local viewer

Browse a report with the built-in local web viewer:

```sh
topo view my-project.123.topo.json
```

It opens a browser at an ephemeral `127.0.0.1` address and serves only that
report. The viewer provides a searchable directory tree, target/sprinkle
filters, summary counts, and syntax-highlighted source. It initially shows
matching lines with one line of context; click a collapsed line range to reveal
it. Ruby is highlighted out of the box, with plaintext fallback for unknown
file types. Use `--no-open` to print the URL without launching a browser.

## Correlating implementations

Compare maps made with old and new implementation vocabulary:

```sh
topo correlate old.topo.json new.topo.json --output feature.topo-correlation.json
topo view feature.topo-correlation.json
```

Both inputs must record the same repository root and scan scope. Correlation is
most reliable when both scans were taken from the same Git revision and clean
working tree; the report surfaces missing or conflicting provenance rather than
assuming equivalence.

The correlation report is a distinct `topo_correlation` format. It embeds both
input reports and preserves real paths, source, regexes, occurrences, path
normalization spans, and content normalization spans. Recorded path-match spans
are replaced with `{CORRELATION}` to form an exact logical display key. That key
is never opened as a repository path. Duplicate keys are `ambiguous`, not
guessed; unmatched keys are `old_only` or `new_only`.

For paired files, Topo normalizes only recorded content-match byte ranges before
aligning lines. The viewer hides unchanged and rename-equivalent lines by
default and lets you reveal rename-equivalent lines separately. It always uses
the original text and line numbers for display. If a regex covers most source,
the report warns that normalization may suppress meaningful differences.

When both scans contain the same physical file, Topo emits a distinct
`shared_context` entry. It compares matched regions and reports counterpart
coverage without diffing the whole file against itself.
