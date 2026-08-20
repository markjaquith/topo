# topo

`topo` keeps its configuration and generated maps in a **workspace**: the
current directory where you run it. The scanned repository can live elsewhere.

```sh
mkdir -p ~/topo/zenpayroll
cd ~/topo/zenpayroll

# .topoignore lives here, outside the scanned repository
topo map 'UserService' --dir ~/workspace/zenpayroll
```

`--dir` selects the Git directory to scan. When omitted, topo scans the current
directory for backwards compatibility. Topo searches only Git-tracked,
non-ignored files under that directory.

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
