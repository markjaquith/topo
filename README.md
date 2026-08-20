# topo

`topo` keeps its configuration and generated maps in a **workspace**: the
current directory where you run it. The scanned repository can live elsewhere.

```sh
mkdir -p ~/topo/zenpayroll
cd ~/topo/zenpayroll
topo map
```

## Workspace configuration

A workspace `topo.toml` specifies the default repository and regex:

```toml
version = 1
scan_dir = "~/workspace/zenpayroll"
pattern = "UserService"
```

The schema is strict: all three keys are required, `version` must be `1`, and
unknown keys are rejected. `scan_dir` accepts a leading `~`, must resolve to a
directory, and must be inside a Git repository. `pattern` must be a non-empty
regex ripgrep can compile.

`topo map` uses these values. A positional regex and `--dir` are optional CLI
overrides:

```sh
topo map 'AdminUser' --dir ~/workspace/zenpayroll
```

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
root, regex, timestamp, every occurrence, matching files, extracted imports,
and graph nodes/edges. A Mermaid sidecar with the same filename and a `.mmd`
extension is written alongside it. Without `--output`, the report is named
`<scan-dir-basename>.<unix-timestamp>.topo.json` in the workspace.

Import extraction currently recognizes Rust, JavaScript/TypeScript, Python, Go,
and Ruby source files.
