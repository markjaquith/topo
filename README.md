# topo

`topo map <regex>` searches tracked files under the current directory with
ripgrep and writes a reusable map of the results.

```sh
topo map 'UserService' --output user-service.topo.json
```

The JSON report contains the searched directory, regex, timestamp, every
occurrence, matching files, extracted imports, and graph nodes/edges. A Mermaid
sidecar with the same filename and a `.mmd` extension is written alongside it.
Without `--output`, the report is named
`<current-dir-basename>.<unix-timestamp>.topo.json` in the current directory.

Only Git-tracked files are searched. Import extraction currently recognizes
Rust, JavaScript/TypeScript, Python, Go, and Ruby source files.
