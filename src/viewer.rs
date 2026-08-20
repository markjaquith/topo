use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::Command,
};

use serde_json::Value;

const VIEWER_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>TOPO</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #10151f;
      --panel: #171e2b;
      --panel-raised: #202938;
      --line: #303b4f;
      --text: #e7edf8;
      --muted: #9aa8bd;
      --accent: #74c7ec;
      --target: #fbbf24;
      --target-bg: #443613;
      --sprinkle: #94a3b8;
    }
    * { box-sizing: border-box; }
    body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.45 ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; }
    button, input { font: inherit; }
    .topography { position: fixed; inset: 0; width: 100%; height: 100%; pointer-events: none; overflow: hidden; opacity: .32; }
    .topography path { vector-effect: non-scaling-stroke; }
    .contours-a, .contours-b { transform-box: fill-box; transform-origin: center; }
    .contours-a { animation: terrain-drift 42s ease-in-out infinite alternate; }
    .contours-b { animation: terrain-breathe 58s ease-in-out infinite alternate; }
    @keyframes terrain-drift { from { transform: translate(-1.5%, .8%) scale(1.025) rotate(-.25deg); } to { transform: translate(1.5%, -1%) scale(1.06) rotate(.25deg); } }
    @keyframes terrain-breathe { from { transform: translate(1%, -1%) scale(.96); opacity: .42; } to { transform: translate(-1%, 1.5%) scale(1.06); opacity: .72; } }
    @media (prefers-reduced-motion: reduce) { .contours-a, .contours-b { animation: none; } }
    .topbar, .stats, main { position: relative; }
    .topbar { padding: 20px 24px 16px; border-bottom: 1px solid var(--line); background: linear-gradient(120deg, rgba(23,34,53,.94), rgba(17,23,34,.90)); }
    .brand { color: var(--accent); font-weight: 800; letter-spacing: .12em; text-transform: uppercase; }
    .context { display: flex; flex-wrap: wrap; gap: 12px 24px; margin-top: 12px; color: var(--muted); }
    .context strong { color: var(--text); font-weight: 600; }
    .stats { display: flex; gap: 10px; padding: 12px 24px; border-bottom: 1px solid var(--line); overflow-x: auto; background: rgba(16,21,31,.72); }
    .stat { min-width: 128px; padding: 8px 10px; border: 1px solid var(--line); border-radius: 7px; background: rgba(23,30,43,.88); }
    .stat b { display: block; color: var(--text); font-size: 18px; }
    .stat span { color: var(--muted); font-size: 11px; }
    main { display: grid; grid-template-columns: minmax(360px, 45%) 1fr; height: calc(100vh - 170px); min-height: 420px; }
    aside { overflow: auto; padding: 16px; border-right: 1px solid var(--line); background: rgba(23,30,43,.88); }
    .toolbar { position: sticky; top: -16px; padding: 16px 0 12px; background: rgba(23,30,43,.96); z-index: 1; }
    input { width: 100%; padding: 9px 10px; color: var(--text); border: 1px solid var(--line); border-radius: 6px; background: var(--bg); outline: none; }
    input:focus { border-color: var(--accent); }
    .filters { display: flex; gap: 6px; margin-top: 9px; }
    .filter, .tree-control { padding: 5px 9px; color: var(--muted); border: 1px solid var(--line); border-radius: 999px; background: transparent; cursor: pointer; }
    .filter.active { color: #06131c; border-color: var(--accent); background: var(--accent); }
    .tree-control { margin-left: auto; }
    .tree-control:disabled { cursor: default; opacity: .45; }
    details { margin: 2px 0; }
    summary { cursor: pointer; color: #cbd5e1; list-style: none; }
    summary::-webkit-details-marker { display: none; }
    summary::before { content: '›'; display: inline-block; width: 14px; color: var(--muted); transition: transform .1s; }
    details[open] > summary::before { transform: rotate(90deg); }
    .directory-count { color: var(--muted); font-size: 11px; }
    .children { margin-left: 14px; border-left: 1px solid #283244; padding-left: 8px; }
    .file { display: flex; width: 100%; gap: 8px; align-items: center; padding: 5px 7px; color: var(--text); border: 1px solid transparent; border-radius: 5px; background: transparent; text-align: left; cursor: pointer; }
    .file:hover, .file.selected { background: var(--panel-raised); border-color: var(--line); }
    .file.target { color: #fde68a; }
    .file .count { display: inline-flex; min-width: 24px; height: 24px; align-items: center; justify-content: center; margin-left: auto; padding: 0 7px; color: var(--muted); border: 1px solid var(--line); border-radius: 999px; background: var(--bg); font-size: 12px; }
    .file .count.zero { color: #fde68a; border-color: #a16207; background: var(--target-bg); }
    section.detail { overflow: auto; padding: 28px; }
    .empty { max-width: 520px; margin: 18vh auto; color: var(--muted); text-align: center; }
    .path { color: var(--muted); overflow-wrap: anywhere; }
    .badges { display: flex; gap: 8px; margin: 16px 0 22px; }
    .badge { padding: 4px 8px; border-radius: 999px; color: var(--muted); background: var(--panel); border: 1px solid var(--line); }
    .badge.target { color: #fde68a; border-color: #a16207; background: var(--target-bg); }
    .matches { border-top: 1px solid var(--line); }
    .match { display: grid; grid-template-columns: 80px minmax(0, 1fr); gap: 12px; padding: 10px 0; border-bottom: 1px solid var(--line); }
    .location { color: var(--accent); white-space: nowrap; }
    code { color: #dbeafe; white-space: pre-wrap; overflow-wrap: anywhere; }
    @media (max-width: 800px) { main { grid-template-columns: 1fr; height: auto; } aside { max-height: 52vh; border-right: 0; border-bottom: 1px solid var(--line); } section.detail { min-height: 48vh; } }
  </style>
</head>
<body>
  <svg class="topography" viewBox="0 0 1440 900" preserveAspectRatio="none" aria-hidden="true">
    <g class="contours-a" fill="none" stroke="#74c7ec" stroke-width="1.25">
      <path d="M-80 186 C80 62 234 80 330 178 S570 304 706 168 S966 50 1086 176 S1308 290 1520 96" />
      <path d="M-80 224 C92 102 228 118 318 210 S570 342 724 210 S954 96 1072 214 S1318 328 1520 136" />
      <path d="M-80 262 C104 144 222 158 304 246 S566 380 742 250 S940 138 1058 252 S1328 366 1520 176" />
      <path d="M-80 300 C116 186 218 198 290 282 S560 420 760 292 S928 180 1044 290 S1338 406 1520 216" />
      <path d="M-80 338 C128 228 214 238 276 318 S556 460 778 334 S916 222 1030 328 S1348 446 1520 256" />
      <path d="M-80 376 C140 270 210 278 262 354 S552 500 796 376 S904 264 1016 366 S1358 486 1520 296" />
    </g>
    <g class="contours-b" fill="none" stroke="#fbbf24" stroke-width="1" opacity=".55">
      <path d="M832 548 C922 468 1064 472 1142 552 C1222 634 1210 744 1126 802 C1038 862 900 834 840 746 C794 678 782 602 832 548 Z" />
      <path d="M858 570 C930 510 1044 510 1112 576 C1178 642 1168 726 1100 774 C1024 826 916 804 866 732 C828 676 820 612 858 570 Z" />
      <path d="M888 594 C946 550 1034 550 1088 600 C1140 650 1132 714 1078 752 C1018 792 934 774 896 718 C866 674 860 628 888 594 Z" />
    </g>
  </svg>
  <header class="topbar">
    <div class="brand">TOPO</div>
    <div class="context" id="context"></div>
  </header>
  <div class="stats" id="stats"></div>
  <main>
    <aside>
      <div class="toolbar">
        <input id="query" type="search" placeholder="Filter files and directories">
        <div class="filters">
          <button class="filter active" data-filter="all">All</button>
          <button class="filter" data-filter="targets">Targets</button>
          <button class="filter" data-filter="sprinkles">Sprinkles</button>
          <button class="tree-control" id="tree-control" type="button">Expand all</button>
        </div>
      </div>
      <div id="tree"></div>
    </aside>
    <section class="detail" id="detail"></section>
  </main>
  <script>
    const state = { report: null, query: '', filter: 'all', selected: null, expanded: new Set(), treeInitialized: false };
    const HOME_DIRECTORY = __TOPO_HOME__;
    const byFile = new Map();
    const escapeHtml = value => String(value).replace(/[&<>'"]/g, character => ({'&':'&amp;','<':'&lt;','>':'&gt;',"'":'&#39;','"':'&quot;'}[character]));
    const number = value => new Intl.NumberFormat().format(value);
    const displayPath = path => HOME_DIRECTORY && (path === HOME_DIRECTORY || path.startsWith(`${HOME_DIRECTORY}/`)) ? `~${path.slice(HOME_DIRECTORY.length)}` : path;

    function buildTree(files) {
      const root = { name: '', path: '', dirs: new Map(), files: [] };
      for (const file of files) {
        const parts = file.path.split('/');
        const filename = parts.pop();
        let current = root;
        for (const part of parts) {
          if (!current.dirs.has(part)) current.dirs.set(part, { name: part, path: current.path ? `${current.path}/${part}` : part, dirs: new Map(), files: [] });
          current = current.dirs.get(part);
        }
        current.files.push({ ...file, filename });
      }
      return root;
    }

    function matchesFilter(file) {
      const query = state.query.trim().toLowerCase();
      if (query && !file.path.toLowerCase().includes(query)) return false;
      if (state.filter === 'targets') return file.is_target;
      if (state.filter === 'sprinkles') return !file.is_target && file.match_count > 0;
      return true;
    }

    function visibleFileCount(node) {
      return node.files.filter(matchesFilter).length + [...node.dirs.values()].reduce((sum, child) => sum + visibleFileCount(child), 0);
    }

    function directoryPaths(node, paths = []) {
      for (const child of node.dirs.values()) {
        paths.push(child.path);
        directoryPaths(child, paths);
      }
      return paths;
    }

    function updateTreeControl(root) {
      const paths = directoryPaths(root);
      const control = document.querySelector('#tree-control');
      const allExpanded = paths.length > 0 && paths.every(path => state.expanded.has(path));
      control.textContent = allExpanded ? 'Collapse all' : 'Expand all';
      control.disabled = paths.length === 0;
    }

    function renderTreeNode(node) {
      const directFiles = node.files.filter(matchesFilter).sort((a, b) => a.filename.localeCompare(b.filename));
      const directories = [...node.dirs.values()].filter(child => visibleFileCount(child) > 0).sort((a, b) => a.name.localeCompare(b.name));
      if (!directFiles.length && !directories.length) return '';
      const children = directories.map(child => renderTreeNode(child)).join('') + directFiles.map(file => {
        const selected = state.selected === file.path ? ' selected' : '';
        const target = file.is_target ? ' target' : '';
        const count = file.match_count ? number(file.match_count) : 'target';
        const zero = file.match_count ? '' : ' zero';
        return `<button class="file${target}${selected}" data-path="${escapeHtml(file.path)}"><span>${escapeHtml(file.filename)}</span><span class="count${zero}">${count}</span></button>`;
      }).join('');
      if (!node.name) return `<div class="tree-root">${children}</div>`;
      const open = state.expanded.has(node.path) ? ' open' : '';
      return `<details data-path="${escapeHtml(node.path)}"${open}><summary>${escapeHtml(node.name)} <span class="directory-count">${number(visibleFileCount(node))}</span></summary><div class="children">${children}</div></details>`;
    }

    function renderTree() {
      const root = buildTree(state.report.files);
      if (!state.treeInitialized) {
        for (const directory of root.dirs.values()) state.expanded.add(directory.path);
        state.treeInitialized = true;
      }
      document.querySelector('#tree').innerHTML = renderTreeNode(root) || '<div class="empty">No matching files</div>';
      updateTreeControl(root);
      document.querySelectorAll('details[data-path]').forEach(details => details.addEventListener('toggle', () => {
        if (details.open) state.expanded.add(details.dataset.path);
        else state.expanded.delete(details.dataset.path);
        updateTreeControl(root);
      }));
      document.querySelectorAll('.file').forEach(button => button.addEventListener('click', () => {
        state.selected = button.dataset.path;
        renderTree();
        renderDetail();
      }));
    }

    function renderDetail() {
      const detail = document.querySelector('#detail');
      const file = state.report.files.find(candidate => candidate.path === state.selected);
      if (!file) {
        detail.innerHTML = '<div class="empty"><h2>Browse the map</h2><p>Select a file to inspect its path, classification, and matching lines.</p></div>';
        return;
      }
      const matches = byFile.get(file.path) || [];
      const target = file.is_target ? '<span class="badge target">filename target</span>' : '<span class="badge">content match</span>';
      const count = `<span class="badge">${number(file.match_count)} content hits</span>`;
      const lines = matches.length ? matches.map(match => `<div class="match"><span class="location">${match.line}:${match.column}</span><code>${escapeHtml(match.text)}</code></div>`).join('') : '<p class="path">No content hits — selected by filename.</p>';
      detail.innerHTML = `<h2>${escapeHtml(file.filename || file.path.split('/').pop())}</h2><div class="path">${escapeHtml(file.path)}</div><div class="badges">${target}${count}</div><div class="matches">${lines}</div>`;
    }

    function renderHeader() {
      const metadata = state.report.metadata;
      document.querySelector('#context').innerHTML = `
        <span>Pattern <strong>${escapeHtml(metadata.regex)}</strong></span>
        <span>Directory ${escapeHtml(displayPath(metadata.scan_directory))}</span>
        <span>mode <strong>${escapeHtml(metadata.mode)}</strong></span>`;
      const files = state.report.files;
      const targets = files.filter(file => file.is_target).length;
      const sprinkles = files.filter(file => !file.is_target && file.match_count > 0).length;
      const totalHits = state.report.matches.length;
      document.querySelector('#stats').innerHTML = `
        <div class="stat"><b>${number(files.length)}</b><span>selected files</span></div>
        <div class="stat"><b>${number(totalHits)}</b><span>content hits</span></div>
        <div class="stat"><b>${number(targets)}</b><span>filename targets</span></div>
        <div class="stat"><b>${number(sprinkles)}</b><span>sprinkle files</span></div>`;
    }

    async function boot() {
      try {
        state.report = await fetch('/report.json').then(response => {
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          return response.json();
        });
        for (const match of state.report.matches) {
          if (!byFile.has(match.file)) byFile.set(match.file, []);
          byFile.get(match.file).push(match);
        }
        renderHeader();
        renderTree();
        renderDetail();
        document.querySelector('#query').addEventListener('input', event => { state.query = event.target.value; renderTree(); });
        document.querySelectorAll('.filter').forEach(button => button.addEventListener('click', () => {
          state.filter = button.dataset.filter;
          document.querySelectorAll('.filter').forEach(candidate => candidate.classList.toggle('active', candidate === button));
          renderTree();
        }));
        document.querySelector('#tree-control').addEventListener('click', () => {
          const paths = directoryPaths(buildTree(state.report.files));
          if (paths.every(path => state.expanded.has(path))) state.expanded.clear();
          else state.expanded = new Set(paths);
          renderTree();
        });
      } catch (error) {
        document.body.innerHTML = `<pre class="empty">Could not load topo report: ${escapeHtml(error.message)}</pre>`;
      }
    }
    boot();
  </script>
</body>
</html>
"##;

pub fn run(report_path: PathBuf, open_browser: bool) -> Result<(), String> {
    let report_path = fs::canonicalize(&report_path)
        .map_err(|error| format!("could not access {}: {error}", report_path.display()))?;
    if !report_path.is_file() {
        return Err(format!("{} is not a file", report_path.display()));
    }
    let report = fs::read(&report_path)
        .map_err(|error| format!("could not read {}: {error}", report_path.display()))?;
    serde_json::from_slice::<Value>(&report)
        .map_err(|error| format!("{} is not valid JSON: {error}", report_path.display()))?;

    let home_directory = env::var("HOME").ok();
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("could not start local viewer: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("could not determine viewer address: {error}"))?;
    let url = format!("http://{address}");

    println!("Report  {}", report_path.display());
    println!("Viewer  {url}");
    println!("Press Ctrl-C to stop the viewer");
    if open_browser {
        if let Err(error) = Command::new("open").arg(&url).spawn() {
            eprintln!("topo: could not open a browser: {error}");
        }
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_connection(stream, &report, home_directory.as_deref()),
            Err(error) => eprintln!("topo: viewer connection failed: {error}"),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, report: &[u8], home_directory: Option<&str>) {
    let mut request_line = String::new();
    {
        let mut reader = BufReader::new(&mut stream);
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");
    match path {
        "/" => {
            let html = viewer_html(home_directory);
            write_response(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                html.as_bytes(),
            )
        }
        "/report.json" => write_response(&mut stream, "200 OK", "application/json", report),
        "/favicon.ico" => write_response(&mut stream, "204 No Content", "text/plain", &[]),
        _ => write_response(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Not found",
        ),
    }
}

fn viewer_html(home_directory: Option<&str>) -> String {
    let home = serde_json::to_string(&home_directory).expect("home directory serializes");
    VIEWER_HTML.replace("__TOPO_HOME__", &home)
}

fn write_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_has_tree_and_detail_structure() {
        assert!(VIEWER_HTML.contains("<title>TOPO</title>"));
        assert!(VIEWER_HTML.contains("class=\"topography\""));
        assert!(VIEWER_HTML.contains("terrain-drift"));
        assert!(VIEWER_HTML.contains("prefers-reduced-motion"));
        assert!(VIEWER_HTML.contains("<div class=\"brand\">TOPO</div>"));
        assert!(VIEWER_HTML.contains("id=\"tree\""));
        assert!(VIEWER_HTML.contains("id=\"detail\""));
        assert!(VIEWER_HTML.contains("data-filter=\"targets\""));
        assert!(VIEWER_HTML.contains("id=\"tree-control\""));
        assert!(VIEWER_HTML.contains("state.expanded"));
        assert!(!VIEWER_HTML.contains('◆'));
        assert!(
            viewer_html(Some("/Users/test")).contains(r#"const HOME_DIRECTORY = "/Users/test";"#)
        );
    }
}
