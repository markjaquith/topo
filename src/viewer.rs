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
    .topography { position: fixed; inset: 0; width: 100%; height: 100%; pointer-events: none; opacity: .22; }
    .topbar, .stats, main { position: relative; }
    .topbar { padding: 20px 24px 16px; border-bottom: 1px solid var(--line); background: linear-gradient(120deg, rgba(23,34,53,.94), rgba(17,23,34,.90)); }
    .brand { color: var(--accent); font-weight: 800; letter-spacing: .12em; text-transform: uppercase; }
    .context { display: flex; flex-wrap: wrap; gap: 12px 24px; margin-top: 12px; color: var(--muted); }
    .context strong { color: var(--text); font-weight: 600; }
    .stats { display: flex; gap: 10px; padding: 12px 24px; border-bottom: 1px solid var(--line); overflow-x: auto; background: rgba(16,21,31,.72); }
    .stat { min-width: 128px; padding: 8px 10px; border: 1px solid var(--line); border-radius: 7px; background: rgba(23,30,43,.88); }
    .stat b { display: block; color: var(--text); font-size: 18px; }
    .stat span { color: var(--muted); font-size: 11px; }
    main { display: grid; grid-template-columns: var(--sidebar-width, 45%) 12px minmax(0, 1fr); height: calc(100vh - 170px); min-height: 420px; }
    aside { overflow: auto; padding: 16px; background: rgba(23,30,43,.88); }
    .resize-handle { display: flex; align-items: center; justify-content: center; cursor: col-resize; background: rgba(16,21,31,.38); touch-action: none; }
    .resize-handle::before { width: 3px; height: 58px; border-radius: 99px; background: var(--line); box-shadow: 0 -8px var(--line), 0 8px var(--line); content: ''; }
    .resize-handle:hover, .resize-handle:focus-visible, .resize-handle.dragging { background: rgba(116,199,236,.16); outline: none; }
    .resize-handle:hover::before, .resize-handle:focus-visible::before, .resize-handle.dragging::before { background: var(--accent); box-shadow: 0 -8px var(--accent), 0 8px var(--accent); }
    .toolbar { position: sticky; top: -16px; padding: 16px 0 12px; background: rgba(23,30,43,.96); z-index: 1; }
    input { width: 100%; padding: 9px 10px; color: var(--text); border: 1px solid var(--line); border-radius: 6px; background: var(--bg); outline: none; }
    input:focus { border-color: var(--accent); }
    .filters { display: flex; gap: 6px; margin-top: 9px; }
    .filter, .tree-control { padding: 5px 9px; color: var(--muted); border: 1px solid var(--line); border-radius: 999px; background: transparent; cursor: pointer; }
    .filter.active { color: #06131c; border-color: var(--accent); background: var(--accent); }
    .tree-control { margin-left: auto; }
    .tree-control:disabled { cursor: default; opacity: .45; }
    details { margin: 2px 0; }
    summary { display: flex; align-items: center; cursor: pointer; color: #cbd5e1; list-style: none; }
    summary::-webkit-details-marker { display: none; }
    summary::before { width: 7px; height: 7px; flex: 0 0 auto; margin: 0 8px 0 3px; border-right: 1.5px solid var(--muted); border-bottom: 1.5px solid var(--muted); content: ''; transform: rotate(-45deg); transform-origin: 50% 50%; transition: transform .12s ease; }
    details[open] > summary::before { transform: rotate(45deg); }
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
    @media (max-width: 800px) { main { grid-template-columns: 1fr; height: auto; } aside { max-height: 52vh; border-bottom: 1px solid var(--line); } .resize-handle { display: none; } section.detail { min-height: 48vh; } }
  </style>
</head>
<body>
  <canvas class="topography" id="topography" aria-hidden="true"></canvas>
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
          <button class="filter" data-filter="filenames">Filenames</button>
          <button class="filter" data-filter="content">Content</button>
          <button class="filter" data-filter="sprinkles">Sprinkles</button>
          <button class="tree-control" id="tree-control" type="button">Expand&nbsp;all</button>
        </div>
      </div>
      <div id="tree"></div>
    </aside>
    <div class="resize-handle" id="resize-handle" role="separator" aria-label="Resize file tree" aria-orientation="vertical" tabindex="0"></div>
    <section class="detail" id="detail"></section>
  </main>
  <script>
    const state = { report: null, query: '', filter: 'all', selected: null, expanded: new Set(), treeInitialized: false };
    const HOME_DIRECTORY = __TOPO_HOME__;
    const byFile = new Map();
    const escapeHtml = value => String(value).replace(/[&<>'"]/g, character => ({'&':'&amp;','<':'&lt;','>':'&gt;',"'":'&#39;','"':'&quot;'}[character]));
    const number = value => new Intl.NumberFormat().format(value);
    const displayPath = path => HOME_DIRECTORY && (path === HOME_DIRECTORY || path.startsWith(`${HOME_DIRECTORY}/`)) ? `~${path.slice(HOME_DIRECTORY.length)}` : path;
    const terrain = { canvas: null, context: null, seed: 0, lastFrame: 0, reducedMotion: false };
    const SIDEBAR_WIDTH_KEY = 'topo.viewer.sidebar-width';
    const contourCases = [[], [[3, 0]], [[0, 1]], [[3, 1]], [[1, 2]], [[3, 2], [0, 1]], [[0, 2]], [[3, 2]], [[2, 3]], [[0, 2]], [[0, 3], [1, 2]], [[1, 2]], [[1, 3]], [[0, 1]], [[3, 0]], []];

    function hashString(value) {
      let hash = 2166136261;
      for (const character of value) hash = Math.imul(hash ^ character.charCodeAt(0), 16777619);
      return hash >>> 0;
    }

    function randomGrid(x, y, seed) {
      let value = Math.imul(x, 374761393) ^ Math.imul(y, 668265263) ^ seed;
      value = Math.imul(value ^ (value >>> 13), 1274126177);
      return ((value ^ (value >>> 16)) >>> 0) / 4294967295;
    }

    function smooth(value) { return value * value * (3 - 2 * value); }
    function lerp(from, to, amount) { return from + (to - from) * amount; }

    function valueNoise(x, y, seed) {
      const x0 = Math.floor(x), y0 = Math.floor(y);
      const tx = smooth(x - x0), ty = smooth(y - y0);
      const top = lerp(randomGrid(x0, y0, seed), randomGrid(x0 + 1, y0, seed), tx);
      const bottom = lerp(randomGrid(x0, y0 + 1, seed), randomGrid(x0 + 1, y0 + 1, seed), tx);
      return lerp(top, bottom, ty) * 2 - 1;
    }

    function terrainValue(x, y, time) {
      const drift = time * .000035;
      const continental = valueNoise(x * .68 + drift, y * .68 - drift * .72, terrain.seed);
      const detail = valueNoise(x * 1.48 - drift * 1.3, y * 1.48 + drift, terrain.seed ^ 0x9e3779b9);
      const ridge = Math.sin(x * .52 + time * .00009) * Math.cos(y * .44 - time * .00007) * .16;
      return continental * .72 + detail * .28 + ridge;
    }

    function edgePoint(edge, x, y, width, height, values, level) {
      const pairs = [[0, 1], [1, 2], [3, 2], [0, 3]];
      const points = [[x, y], [x + width, y], [x + width, y + height], [x, y + height]];
      const [from, to] = pairs[edge];
      const amount = Math.max(0, Math.min(1, (level - values[from]) / (values[to] - values[from] || 1)));
      return [lerp(points[from][0], points[to][0], amount), lerp(points[from][1], points[to][1], amount)];
    }

    function pointKey([x, y]) { return `${Math.round(x * 100)}:${Math.round(y * 100)}`; }
    function midpoint([x1, y1], [x2, y2]) { return [(x1 + x2) / 2, (y1 + y2) / 2]; }

    function stitchSegments(segments) {
      const endpoints = new Map();
      segments.forEach((segment, segmentIndex) => segment.forEach((point, endpointIndex) => {
        const key = pointKey(point);
        if (!endpoints.has(key)) endpoints.set(key, []);
        endpoints.get(key).push([segmentIndex, endpointIndex]);
      }));
      const visited = new Set();
      const takeConnection = key => (endpoints.get(key) || []).find(([segmentIndex]) => !visited.has(segmentIndex));
      const paths = [];
      for (let index = 0; index < segments.length; index++) {
        if (visited.has(index)) continue;
        visited.add(index);
        const points = [...segments[index]];
        const extend = fromEnd => {
          while (true) {
            const endpoint = fromEnd ? points[points.length - 1] : points[0];
            const connection = takeConnection(pointKey(endpoint));
            if (!connection) return;
            const [segmentIndex, endpointIndex] = connection;
            visited.add(segmentIndex);
            const next = segments[segmentIndex][endpointIndex === 0 ? 1 : 0];
            if (fromEnd) points.push(next);
            else points.unshift(next);
          }
        };
        extend(true);
        extend(false);
        paths.push(points);
      }
      return paths;
    }

    function relaxPoints(points, closed) {
      const amount = .18;
      return points.map((point, index) => {
        if (!closed && (index === 0 || index === points.length - 1)) return point;
        const previous = points[(index - 1 + points.length) % points.length];
        const next = points[(index + 1) % points.length];
        return [
          point[0] * (1 - amount) + (previous[0] + next[0]) * amount / 2,
          point[1] * (1 - amount) + (previous[1] + next[1]) * amount / 2,
        ];
      });
    }

    function drawSmoothPath(context, points) {
      if (points.length < 2) return;
      const closed = points.length > 3 && pointKey(points[0]) === pointKey(points[points.length - 1]);
      const path = relaxPoints(closed ? points.slice(0, -1) : points, closed);
      if (closed) {
        context.moveTo(...midpoint(path[path.length - 1], path[0]));
        for (let index = 0; index < path.length; index++) {
          context.quadraticCurveTo(...path[index], ...midpoint(path[index], path[(index + 1) % path.length]));
        }
        context.closePath();
        return;
      }
      context.moveTo(...path[0]);
      for (let index = 1; index < path.length - 1; index++) {
        context.quadraticCurveTo(...path[index], ...midpoint(path[index], path[index + 1]));
      }
      context.lineTo(...path[path.length - 1]);
    }

    function drawContours(context, values, columns, rows, cellWidth, cellHeight, levels, color, alpha) {
      context.strokeStyle = `rgba(${color}, ${alpha})`;
      context.lineWidth = 1;
      context.lineJoin = 'round';
      context.lineCap = 'round';
      for (const level of levels) {
        const segments = [];
        for (let row = 0; row < rows - 1; row++) {
          for (let column = 0; column < columns - 1; column++) {
            const cell = [values[row][column], values[row][column + 1], values[row + 1][column + 1], values[row + 1][column]];
            const index = (cell[0] >= level ? 1 : 0) | (cell[1] >= level ? 2 : 0) | (cell[2] >= level ? 4 : 0) | (cell[3] >= level ? 8 : 0);
            for (const [start, end] of contourCases[index]) {
              const x = column * cellWidth, y = row * cellHeight;
              segments.push([
                edgePoint(start, x, y, cellWidth, cellHeight, cell, level),
                edgePoint(end, x, y, cellWidth, cellHeight, cell, level),
              ]);
            }
          }
        }
        context.beginPath();
        for (const path of stitchSegments(segments)) drawSmoothPath(context, path);
        context.stroke();
      }
    }

    function drawTerrain(time) {
      const canvas = terrain.canvas;
      const density = Math.min(window.devicePixelRatio || 1, 2);
      const width = window.innerWidth, height = window.innerHeight;
      const pixelWidth = Math.round(width * density), pixelHeight = Math.round(height * density);
      if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
        canvas.width = pixelWidth;
        canvas.height = pixelHeight;
      }
      const context = terrain.context;
      context.setTransform(density, 0, 0, density, 0, 0);
      context.clearRect(0, 0, width, height);
      const cellSize = 38;
      const columns = Math.ceil(width / cellSize) + 1;
      const rows = Math.ceil(height / cellSize) + 1;
      const cellWidth = width / (columns - 1), cellHeight = height / (rows - 1);
      const values = Array.from({ length: rows }, (_, row) => Array.from({ length: columns }, (_, column) => terrainValue(column / (columns - 1) * 8, row / (rows - 1) * 5.5, time)));
      drawContours(context, values, columns, rows, cellWidth, cellHeight, [-.78, -.56, -.34, -.12, .1, .32, .54, .76], '116,199,236', .42);
      drawContours(context, values, columns, rows, cellWidth, cellHeight, [.24], '251,191,36', .38);
    }

    function startTerrain(metadata) {
      terrain.canvas = document.querySelector('#topography');
      terrain.context = terrain.canvas.getContext('2d');
      terrain.seed = hashString(`${metadata.scan_directory}:${metadata.regex}`);
      terrain.reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      const frame = time => {
        if (terrain.reducedMotion) return;
        if (time - terrain.lastFrame > 1000 / 14) {
          drawTerrain(time);
          terrain.lastFrame = time;
        }
        requestAnimationFrame(frame);
      };
      drawTerrain(0);
      if (!terrain.reducedMotion) requestAnimationFrame(frame);
      window.addEventListener('resize', () => drawTerrain(performance.now()), { passive: true });
    }

    function setupSidebarResize() {
      const handle = document.querySelector('#resize-handle');
      const minimum = 480;
      const maximum = () => Math.max(minimum, window.innerWidth - 320);
      const setWidth = (width, persist = false) => {
        const clamped = Math.max(minimum, Math.min(maximum(), width));
        document.documentElement.style.setProperty('--sidebar-width', `${clamped}px`);
        handle.setAttribute('aria-valuenow', Math.round(clamped));
        if (persist) {
          try { localStorage.setItem(SIDEBAR_WIDTH_KEY, clamped); } catch (_) {}
        }
        return clamped;
      };
      try {
        const saved = Number(localStorage.getItem(SIDEBAR_WIDTH_KEY));
        if (Number.isFinite(saved) && saved > 0) setWidth(saved);
      } catch (_) {}
      let dragging = false;
      handle.addEventListener('pointerdown', event => {
        dragging = true;
        handle.classList.add('dragging');
        handle.setPointerCapture(event.pointerId);
        document.body.style.cursor = 'col-resize';
        event.preventDefault();
      });
      handle.addEventListener('pointermove', event => {
        if (dragging) setWidth(event.clientX);
      });
      const stopDragging = () => {
        if (!dragging) return;
        dragging = false;
        handle.classList.remove('dragging');
        document.body.style.cursor = '';
        const width = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--sidebar-width'));
        if (Number.isFinite(width)) setWidth(width, true);
      };
      handle.addEventListener('pointerup', stopDragging);
      handle.addEventListener('pointercancel', stopDragging);
      handle.addEventListener('keydown', event => {
        if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
        const width = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--sidebar-width')) || window.innerWidth * .45;
        setWidth(width + (event.key === 'ArrowLeft' ? -24 : 24), true);
        event.preventDefault();
      });
      window.addEventListener('resize', () => {
        const width = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--sidebar-width'));
        if (Number.isFinite(width)) setWidth(width);
      }, { passive: true });
    }

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
      if (state.filter === 'filenames') return file.is_target;
      if (state.filter === 'content') return file.match_count > 0;
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
      control.textContent = allExpanded ? 'Collapse\u00A0all' : 'Expand\u00A0all';
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
        startTerrain(state.report.metadata);
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
        setupSidebarResize();
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
        assert!(VIEWER_HTML.contains("id=\"topography\""));
        assert!(VIEWER_HTML.contains("drawContours"));
        assert!(VIEWER_HTML.contains("stitchSegments"));
        assert!(VIEWER_HTML.contains("quadraticCurveTo"));
        assert!(VIEWER_HTML.contains("relaxPoints"));
        assert!(VIEWER_HTML.contains("context.lineJoin = 'round'"));
        assert!(VIEWER_HTML.contains("context.lineCap = 'round'"));
        assert!(VIEWER_HTML.contains("prefers-reduced-motion"));
        assert!(VIEWER_HTML.contains("<div class=\"brand\">TOPO</div>"));
        assert!(VIEWER_HTML.contains("id=\"tree\""));
        assert!(VIEWER_HTML.contains("id=\"detail\""));
        assert!(VIEWER_HTML.contains("data-filter=\"filenames\">Filenames"));
        assert!(VIEWER_HTML.contains("data-filter=\"content\">Content"));
        assert!(VIEWER_HTML.contains("id=\"tree-control\""));
        assert!(VIEWER_HTML.contains("id=\"resize-handle\""));
        assert!(VIEWER_HTML.contains("localStorage.setItem(SIDEBAR_WIDTH_KEY"));
        assert!(VIEWER_HTML.contains("state.expanded"));
        assert!(!VIEWER_HTML.contains('◆'));
        assert!(
            viewer_html(Some("/Users/test")).contains(r#"const HOME_DIRECTORY = "/Users/test";"#)
        );
    }
}
