use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use syntect::{
    easy::HighlightLines,
    highlighting::ThemeSet,
    html::{IncludeBackground, styled_line_to_highlighted_html},
    parsing::SyntaxSet,
};

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
      --added: #86efac;
      --deleted: #fca5a5;
      --renamed: #c4b5fd;
    }
    * { box-sizing: border-box; }
    body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.45 ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; }
    button, input { font: inherit; }
    .topography { position: fixed; inset: 0; width: 100%; height: 100%; pointer-events: none; opacity: .22; }
    .topbar, main { position: relative; }
    .topbar { display: flex; align-items: flex-start; justify-content: space-between; gap: 24px; padding: 20px 24px 16px; border-bottom: 1px solid var(--line); background: linear-gradient(120deg, rgba(23,34,53,.94), rgba(17,23,34,.90)); }
    .header-info { min-width: 0; }
    .brand { margin: 0; color: var(--accent); font-size: 20px; font-weight: 800; letter-spacing: .12em; line-height: 1; text-transform: uppercase; }
    .sidebar-toggle { position: absolute; z-index: 2; top: 12px; left: 50%; display: inline-flex; width: 30px; height: 30px; align-items: center; justify-content: center; padding: 0; color: var(--muted); border: 1px solid var(--line); border-radius: 5px; background: var(--panel); cursor: pointer; transform: translateX(-50%); }
    .sidebar-toggle[hidden] { display: none; }
    .sidebar-toggle svg { width: 19px; height: 19px; fill: currentColor; }
    .sidebar-toggle:hover, .sidebar-toggle:focus-visible { color: var(--accent); border-color: var(--accent); background: var(--panel-raised); outline: none; }
    .context { display: flex; flex-wrap: wrap; gap: 12px 24px; margin-top: 12px; color: var(--muted); }
    .context-item { display: inline-flex; min-width: 0; gap: 6px; align-items: center; }
    .context-item svg { width: 16px; height: 16px; flex: 0 0 auto; fill: var(--accent); }
    .context strong { color: var(--text); font-weight: 600; }
    .stats { display: flex; flex: 0 0 auto; gap: 10px; overflow-x: auto; }
    .stat { min-width: 112px; padding: 8px 10px; border: 1px solid var(--line); border-radius: 7px; background: rgba(23,30,43,.88); }
    .stat b { display: block; color: var(--text); font-size: 18px; }
    .stat span { color: var(--muted); font-size: 11px; }
    main { display: grid; grid-template-columns: var(--sidebar-width, 45%) 12px minmax(0, 1fr); height: calc(100vh - 92px); min-height: 420px; }
    main.sidebar-collapsed { grid-template-columns: 32px minmax(0, 1fr); }
    main.sidebar-collapsed aside { display: none; }
    main.sidebar-collapsed .resize-handle { display: flex; cursor: default; }
    main.sidebar-collapsed .resize-handle::before { display: none; }
    aside { overflow: auto; padding: 16px; background: rgba(23,30,43,.88); }
    .resize-handle { position: relative; display: flex; align-items: center; justify-content: center; cursor: col-resize; background: rgba(16,21,31,.38); touch-action: none; }
    .resize-handle::before { width: 3px; height: 58px; border-radius: 99px; background: var(--line); box-shadow: 0 -8px var(--line), 0 8px var(--line); content: ''; }
    .resize-handle:hover, .resize-handle:focus-visible, .resize-handle.dragging { background: rgba(116,199,236,.16); outline: none; }
    .resize-handle:hover::before, .resize-handle:focus-visible::before, .resize-handle.dragging::before { background: var(--accent); box-shadow: 0 -8px var(--accent), 0 8px var(--accent); }
    .toolbar { position: sticky; top: -16px; padding: 16px 0 12px; background: rgba(23,30,43,.96); z-index: 1; }
    input { width: 100%; padding: 9px 10px; color: var(--text); border: 1px solid var(--line); border-radius: 6px; background: var(--bg); outline: none; }
    input:focus { border-color: var(--accent); }
    input::-webkit-search-cancel-button { cursor: pointer; }
    .filters { display: flex; gap: 6px; margin-top: 9px; }
    .filter, .tree-control { padding: 5px 9px; color: var(--muted); border: 1px solid var(--line); border-radius: 999px; background: transparent; cursor: pointer; }
    .filter.active { color: #06131c; border-color: var(--accent); background: var(--accent); }
    .tree-control { display: inline-flex; width: 32px; height: 32px; align-items: center; justify-content: center; margin-left: auto; padding: 0; }
    .tree-control svg { width: 19px; height: 19px; fill: currentColor; }
    .tree-control:disabled { cursor: default; opacity: .45; }
    .visually-hidden { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
    details { margin: 2px 0; }
    summary { display: flex; align-items: center; cursor: pointer; color: #cbd5e1; list-style: none; }
    summary::-webkit-details-marker { display: none; }
    summary::before { width: 7px; height: 7px; flex: 0 0 auto; margin: 0 8px 0 3px; border-right: 1.5px solid var(--muted); border-bottom: 1.5px solid var(--muted); content: ''; transform: rotate(-45deg); transform-origin: 50% 50%; transition: transform .12s ease; }
    details[open] > summary::before { transform: rotate(45deg); }
    .directory-count { flex: 0 0 auto; margin-left: 10px; color: var(--muted); font-size: 11px; }
    .children { margin-left: 14px; border-left: 1px solid #283244; padding-left: 8px; }
    .file { display: flex; width: 100%; min-height: 36px; gap: 8px; align-items: center; padding: 5px 7px; color: var(--text); border: 1px solid transparent; border-radius: 5px; background: transparent; text-align: left; cursor: pointer; }
    .file:hover, .file.selected { padding-top: 4px; padding-bottom: 4px; background: var(--panel-raised); border-color: var(--line); }
    .path-match { padding: 0; color: #fde68a; background: transparent; font-weight: 700; }
    .file .count { display: inline-flex; min-width: 24px; height: 24px; align-items: center; justify-content: center; margin-left: auto; padding: 0 7px; color: var(--muted); border: 1px solid var(--line); border-radius: 999px; background: var(--bg); font-size: 12px; }
    .file .count.zero { color: #fde68a; border-color: #a16207; background: var(--target-bg); }
    section.detail { overflow: auto; padding: 28px; }
    .empty { max-width: 520px; margin: 18vh auto; color: var(--muted); text-align: center; }
    .path { color: var(--muted); overflow-wrap: anywhere; }
    .badges { display: flex; gap: 8px; margin: 16px 0 22px; }
    .badge { padding: 4px 8px; border-radius: 999px; color: var(--muted); background: var(--panel); border: 1px solid var(--line); }
    .badge.target { color: #fde68a; border-color: #a16207; background: var(--target-bg); }
    .source { margin-top: 22px; overflow: hidden; border: 1px solid var(--line); border-radius: 7px; background: #0d131d; }
    .code-line { display: grid; grid-template-columns: 58px minmax(0, 1fr); min-width: 0; border-left: 3px solid transparent; }
    .code-line + .code-line { border-top: 1px solid rgba(48,59,79,.45); }
    .code-line.match { border-left-color: var(--accent); background: rgba(116,199,236,.12); }
    .inline-match { padding: 0; color: inherit; border-radius: 2px; background: rgba(251,191,36,.38); }
    .line-number { padding: 5px 10px; color: #65758d; border-right: 1px solid var(--line); background: rgba(23,30,43,.62); font-size: 12px; text-align: right; user-select: none; }
    .code-line code { display: block; min-width: 0; padding: 5px 12px; color: #dbeafe; overflow-x: auto; white-space: pre; }
    .code-gap { display: grid; grid-template-columns: 58px minmax(0, 1fr); width: 100%; padding: 0; color: var(--accent); border: 0; border-top: 1px solid rgba(48,59,79,.7); border-bottom: 1px solid rgba(48,59,79,.7); background: rgba(116,199,236,.06); font: inherit; text-align: left; cursor: pointer; }
    .code-gap:hover, .code-gap:focus-visible { background: rgba(116,199,236,.15); outline: none; }
    .code-gap::before { padding: 5px 10px; color: var(--muted); border-right: 1px solid var(--line); content: '…'; text-align: right; }
    .code-gap span { padding: 5px 12px; }
    .source-unavailable { margin-top: 22px; color: var(--muted); }
    .warning { margin: 10px 0; padding: 9px 11px; color: #fde68a; border-left: 3px solid var(--target); background: rgba(251,191,36,.09); }
    .side-paths { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin: 14px 0; }
    .side-path { min-width: 0; padding: 9px 11px; overflow-wrap: anywhere; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); }
    .side-path b { display: block; margin-bottom: 3px; color: var(--muted); font-size: 11px; text-transform: uppercase; }
    .diff { margin-top: 18px; overflow: hidden; border: 1px solid var(--line); border-radius: 7px; background: #0d131d; }
    .diff-line { display: grid; grid-template-columns: 48px 48px minmax(0, 1fr) minmax(0, 1fr); border-left: 3px solid transparent; }
    .diff-line + .diff-line { border-top: 1px solid rgba(48,59,79,.45); }
    .diff-line > span { padding: 5px 8px; color: #65758d; border-right: 1px solid var(--line); text-align: right; user-select: none; }
    .diff-line code { min-width: 0; padding: 5px 10px; overflow-x: auto; border-right: 1px solid var(--line); white-space: pre; }
    .diff-line.addition { border-left-color: var(--added); background: rgba(134,239,172,.08); }
    .diff-line.deletion { border-left-color: var(--deleted); background: rgba(252,165,165,.08); }
    .diff-line.modification { border-left-color: var(--target); background: rgba(251,191,36,.08); }
    .diff-line.rename-equivalent { border-left-color: var(--renamed); background: rgba(196,181,253,.10); }
    .rename-toggle { margin-top: 14px; padding: 6px 9px; color: var(--renamed); border: 1px solid #6d5a9b; border-radius: 5px; background: rgba(196,181,253,.08); cursor: pointer; }
    @media (max-width: 1100px) { .topbar { flex-direction: column; } .stats { width: 100%; justify-content: flex-start; } main { height: calc(100vh - 170px); } }
    @media (max-width: 800px) { main { grid-template-columns: 1fr; height: auto; } aside { max-height: 52vh; border-bottom: 1px solid var(--line); } .resize-handle { display: none; } section.detail { min-height: 48vh; } .side-paths { grid-template-columns: 1fr; } .diff-line { grid-template-columns: 38px 38px minmax(0, 1fr); } .diff-line code + code { grid-column: 3; border-top: 1px dashed var(--line); } }
  </style>
</head>
<body>
  <canvas class="topography" id="topography" aria-hidden="true"></canvas>
  <header class="topbar">
    <div class="header-info">
      <h1 class="brand">TOPO</h1>
      <div class="context" id="context"></div>
    </div>
    <div class="stats" id="stats"></div>
  </header>
  <main>
    <aside>
      <div class="toolbar">
        <input id="query" type="search" placeholder="Filter paths or hits<2">
        <div class="filters">
          <button class="filter active" data-filter="all">All</button>
          <button class="filter" data-filter="paths">Paths</button>
          <button class="filter" data-filter="content">Content</button>
          <button class="filter" data-filter="sprinkles">Sprinkles</button>
          <button class="tree-control" id="tree-control" type="button" aria-label="Expand all folders" title="Expand all folders">
            <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M12 5.83 15.17 9l1.41-1.41L12 3 7.41 7.59 8.83 9 12 5.83zm0 12.34L8.83 15l-1.41 1.41L12 21l4.59-4.59L15.17 15 12 18.17z"/></svg>
            <span class="visually-hidden">Expand all folders</span>
          </button>
        </div>
      </div>
      <div id="tree"></div>
    </aside>
    <div class="resize-handle" id="resize-handle" role="separator" aria-label="Resize file tree" aria-orientation="vertical" tabindex="0">
      <button class="sidebar-toggle" id="sidebar-toggle" type="button" aria-label="Hide sidebar" title="Hide sidebar">
        <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="m11.67 8.59-1.41-1.42L5.43 12l4.83 4.83 1.41-1.42L8.43 12l3.24-3.41zm5.66 0-1.41-1.42L11.09 12l4.83 4.83 1.41-1.42L14.09 12l3.24-3.41z"/></svg>
        <span class="visually-hidden">Hide sidebar</span>
      </button>
    </div>
    <section class="detail" id="detail"></section>
  </main>
  <script>
    const state = { report: null, query: '', filterSpec: { text: '', hitRestrictions: [] }, filter: 'all', selected: null, expanded: new Set(), expandedSourceRanges: new Set(), expandedRenamePaths: new Set(), sidebarCollapsed: false, treeInitialized: false };
    const HOME_DIRECTORY = __TOPO_HOME__;
    const byFile = new Map();
    const escapeHtml = value => String(value).replace(/[&<>'"]/g, character => ({'&':'&amp;','<':'&lt;','>':'&gt;',"'":'&#39;','"':'&quot;'}[character]));
    const number = value => new Intl.NumberFormat().format(value);
    const displayPath = path => HOME_DIRECTORY && (path === HOME_DIRECTORY || path.startsWith(`${HOME_DIRECTORY}/`)) ? `~${path.slice(HOME_DIRECTORY.length)}` : path;
    const terrain = { canvas: null, context: null, seed: 0, lastFrame: 0, reducedMotion: false };
    const SIDEBAR_WIDTH_KEY = 'topo.viewer.sidebar-width';
    const SIDEBAR_COLLAPSED_KEY = 'topo.viewer.sidebar-collapsed';
    const MATERIAL_KEYBOARD_DOUBLE_ARROW_LEFT = 'm11.67 8.59-1.41-1.42L5.43 12l4.83 4.83 1.41-1.42L8.43 12l3.24-3.41zm5.66 0-1.41-1.42L11.09 12l4.83 4.83 1.41-1.42L14.09 12l3.24-3.41z';
    const MATERIAL_KEYBOARD_DOUBLE_ARROW_RIGHT = 'm12.33 15.41 1.41 1.42L18.57 12l-4.83-4.83-1.41 1.42L15.57 12l-3.24 3.41zm-5.66 0 1.41 1.42L12.91 12 8.08 7.17 6.67 8.59 9.91 12l-3.24 3.41z';
    const MATERIAL_UNFOLD_MORE = 'M12 5.83 15.17 9l1.41-1.41L12 3 7.41 7.59 8.83 9 12 5.83zm0 12.34L8.83 15l-1.41 1.41L12 21l4.59-4.59L15.17 15 12 18.17z';
    const MATERIAL_UNFOLD_LESS = 'M7.41 18.41 12 13.83l4.59 4.58L18 17l-6-6-6 6zM7.41 5.59 12 10.17l4.59-4.58L18 7l-6 6-6-6z';
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
        if (state.sidebarCollapsed || event.target.closest('#sidebar-toggle')) return;
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
        if (state.sidebarCollapsed || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
        const width = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--sidebar-width')) || window.innerWidth * .45;
        setWidth(width + (event.key === 'ArrowLeft' ? -24 : 24), true);
        event.preventDefault();
      });
      window.addEventListener('resize', () => {
        const width = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--sidebar-width'));
        if (Number.isFinite(width)) setWidth(width);
      }, { passive: true });
    }

    function applySidebarState() {
      const collapsed = state.sidebarCollapsed && Boolean(state.selected);
      document.querySelector('main').classList.toggle('sidebar-collapsed', collapsed);
      const toggle = document.querySelector('#sidebar-toggle');
      toggle.hidden = !state.selected;
      const label = collapsed ? 'Show sidebar (S)' : 'Hide sidebar (S)';
      const icon = collapsed ? MATERIAL_KEYBOARD_DOUBLE_ARROW_RIGHT : MATERIAL_KEYBOARD_DOUBLE_ARROW_LEFT;
      toggle.setAttribute('aria-label', label);
      toggle.title = label;
      toggle.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="${icon}"/></svg><span class="visually-hidden">${label}</span>`;
    }

    function setSidebarCollapsed(collapsed, persist = false) {
      state.sidebarCollapsed = collapsed;
      applySidebarState();
      if (persist) {
        try { localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(collapsed)); } catch (_) {}
      }
    }

    function setupSidebarToggle() {
      let collapsed = false;
      try { collapsed = localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === 'true'; } catch (_) {}
      setSidebarCollapsed(collapsed);
      document.querySelector('#sidebar-toggle').addEventListener('click', () => setSidebarCollapsed(!state.sidebarCollapsed, true));
      window.addEventListener('keydown', event => {
        if (!state.selected || event.key.toLowerCase() !== 's' || event.metaKey || event.ctrlKey || event.altKey || event.repeat) return;
        if (event.target instanceof Element && (event.target.isContentEditable || event.target.closest('input, textarea, select, [contenteditable="true"]'))) return;
        setSidebarCollapsed(!state.sidebarCollapsed, true);
        event.preventDefault();
      });
      window.addEventListener('keydown', event => {
        if (event.key !== '/' || event.metaKey || event.ctrlKey || event.altKey) return;
        if (event.target instanceof Element && (event.target.isContentEditable || event.target.closest('input, textarea, select, [contenteditable="true"]'))) return;
        setSidebarCollapsed(false);
        document.querySelector('#query').focus();
        event.preventDefault();
      });
    }

    function pathMatchRanges(file, componentIndex) {
      return (file.path_match_ranges || []).filter(range => range.component_index === componentIndex);
    }

    function buildTree(files) {
      const root = { name: '', path: '', dirs: new Map(), files: [] };
      for (const file of files) {
        const parts = file.path.split('/');
        const filename = parts.pop();
        let current = root;
        for (const [componentIndex, part] of parts.entries()) {
          if (!current.dirs.has(part)) {
            current.dirs.set(part, {
              name: part,
              path: current.path ? `${current.path}/${part}` : part,
              matchRanges: pathMatchRanges(file, componentIndex),
              dirs: new Map(),
              files: [],
            });
          }
          current = current.dirs.get(part);
        }
        current.files.push({ ...file, filename });
      }
      return root;
    }

    function parseFilter(query) {
      const hitRestrictions = [];
      const text = query.replace(/(?:^|\s)hits(<=|>=|=|<|>)(\d+)(?=\s|$)/gi, (_, operator, value) => {
        hitRestrictions.push({ operator, value: Number(value) });
        return ' ';
      }).trim().toLowerCase();
      return { text, hitRestrictions };
    }

    function matchesHitRestriction(hitCount, { operator, value }) {
      if (operator === '<') return hitCount < value;
      if (operator === '<=') return hitCount <= value;
      if (operator === '=') return hitCount === value;
      if (operator === '>') return hitCount > value;
      return hitCount >= value;
    }

    function matchesFilter(file) {
      const { text, hitRestrictions } = state.filterSpec;
      if (text && !file.path.toLowerCase().includes(text)) return false;
      if (!hitRestrictions.every(restriction => matchesHitRestriction(file.match_count, restriction))) return false;
      if (state.report.report_type === 'topo_correlation') {
        if (state.filter === 'paths') return file.classification === 'paired';
        if (state.filter === 'content') return ['old_only', 'new_only'].includes(file.classification);
        if (state.filter === 'sprinkles') return file.classification === 'ambiguous';
        return true;
      }
      if (state.filter === 'paths') return file.is_target;
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
      const label = allExpanded ? 'Collapse all folders' : 'Expand all folders';
      const icon = allExpanded ? MATERIAL_UNFOLD_LESS : MATERIAL_UNFOLD_MORE;
      control.setAttribute('aria-label', label);
      control.title = label;
      control.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="${icon}"/></svg><span class="visually-hidden">${label}</span>`;
      control.disabled = paths.length === 0;
    }

    function filenameFor(file) {
      return file.filename || file.path.split('/').pop();
    }

    function classificationLabel(classification) {
      const separator = ['old_only', 'new_only'].includes(classification) ? '\u00a0' : ' ';
      return classification.replace('_', separator);
    }

    function renderPathComponent(component, ranges) {
      if (!ranges.length) return escapeHtml(component);
      const characters = Array.from(component);
      let position = 0;
      const parts = [];
      for (const range of ranges) {
        if (range.start > position) parts.push(escapeHtml(characters.slice(position, range.start).join('')));
        if (range.end > range.start) parts.push(`<mark class="path-match">${escapeHtml(characters.slice(range.start, range.end).join(''))}</mark>`);
        position = Math.max(position, range.end);
      }
      if (position < characters.length) parts.push(escapeHtml(characters.slice(position).join('')));
      return parts.join('');
    }

    function renderFilename(file) {
      const parts = file.path.split('/');
      return renderPathComponent(filenameFor(file), pathMatchRanges(file, parts.length - 1));
    }

    function renderPath(file) {
      return file.path.split('/').map((component, componentIndex) => renderPathComponent(component, pathMatchRanges(file, componentIndex))).join('<span class="path-separator">/</span>');
    }

    function renderTreeNode(node) {
      const directFiles = node.files.filter(matchesFilter).sort((a, b) => a.filename.localeCompare(b.filename));
      const directories = [...node.dirs.values()].filter(child => visibleFileCount(child) > 0).sort((a, b) => a.name.localeCompare(b.name));
      if (!directFiles.length && !directories.length) return '';
      const children = directories.map(child => renderTreeNode(child)).join('') + directFiles.map(file => {
        const selected = state.selected === file.path ? ' selected' : '';
        const target = file.is_target ? ' target' : '';
        const count = state.report.report_type === 'topo_correlation' ? classificationLabel(file.classification) : (file.match_count ? number(file.match_count) : 'target');
        const zero = file.match_count || state.report.report_type === 'topo_correlation' ? '' : ' zero';
        return `<button class="file${target}${selected}" data-path="${escapeHtml(file.path)}"><span>${renderFilename(file)}</span><span class="count${zero}">${count}</span></button>`;
      }).join('');
      if (!node.name) return `<div class="tree-root">${children}</div>`;
      const open = state.expanded.has(node.path) ? ' open' : '';
      return `<details data-path="${escapeHtml(node.path)}"${open}><summary>${renderPathComponent(node.name, node.matchRanges || [])}<span class="directory-count">${number(visibleFileCount(node))}</span></summary><div class="children">${children}</div></details>`;
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

    function sourceLines(file) {
      if (typeof file.content !== 'string') return null;
      if (Array.isArray(file.highlighted_lines)) return file.highlighted_lines;
      const lines = file.content.replace(/\r\n/g, '\n').split('\n');
      if (file.content.endsWith('\n')) lines.pop();
      return lines.map(escapeHtml);
    }

    function renderSource(file, matches) {
      const lines = sourceLines(file);
      if (!lines) return '<p class="source-unavailable">Source text is unavailable for this file.</p>';
      const matchesByLine = new Map();
      for (const match of matches) {
        if (!matchesByLine.has(match.line)) matchesByLine.set(match.line, []);
        matchesByLine.get(match.line).push(match);
      }
      const visibleLines = new Set();
      if (!matchesByLine.size) {
        for (let line = 1; line <= lines.length; line += 1) visibleLines.add(line);
      } else {
        for (const line of matchesByLine.keys()) {
          for (let nearby = Math.max(1, line - 1); nearby <= Math.min(lines.length, line + 1); nearby += 1) visibleLines.add(nearby);
        }
      }

      const rendered = [];
      for (let line = 1; line <= lines.length;) {
        if (visibleLines.has(line)) {
          const lineMatches = matchesByLine.get(line) || [];
          const locations = lineMatches.map(match => `${match.line}:${match.column}`).join(', ');
          const ranges = lineMatches.map(match => ({ start: Math.max(0, match.column - 1), end: Math.max(0, match.end_column - 1) }));
          const rangeAttribute = ranges.length ? ` data-match-ranges="${escapeHtml(JSON.stringify(ranges))}"` : '';
          rendered.push(`<div class="code-line${lineMatches.length ? ' match' : ''}" data-source-line="${line}"${locations ? ` title="Match at ${escapeHtml(locations)}"` : ''}><span class="line-number">${line}</span><code${rangeAttribute}>${lines[line - 1] || ' '}</code></div>`);
          line += 1;
          continue;
        }

        const start = line;
        while (line <= lines.length && !visibleLines.has(line)) line += 1;
        const end = line - 1;
        const range = `${file.path}:${start}-${end}`;
        if (state.expandedSourceRanges.has(range)) {
          for (let expanded = start; expanded <= end; expanded += 1) {
            rendered.push(`<div class="code-line" data-source-line="${expanded}"><span class="line-number">${expanded}</span><code>${lines[expanded - 1] || ' '}</code></div>`);
          }
        } else {
          const count = end - start + 1;
          rendered.push(`<button class="code-gap" type="button" data-source-range="${escapeHtml(range)}"><span>${number(count)} more ${count === 1 ? 'line' : 'lines'}</span></button>`);
        }
      }
      return `<div class="source" aria-label="Source for ${escapeHtml(file.path)}">${rendered.join('')}</div>`;
    }

    function utf8Width(codePoint) {
      if (codePoint <= 0x7f) return 1;
      if (codePoint <= 0x7ff) return 2;
      if (codePoint <= 0xffff) return 3;
      return 4;
    }

    function codeUnitOffsetForByteOffset(text, byteOffset) {
      let bytes = 0;
      for (let offset = 0; offset < text.length;) {
        if (bytes >= byteOffset) return offset;
        const codePoint = text.codePointAt(offset);
        const width = utf8Width(codePoint);
        if (bytes + width > byteOffset) return offset;
        bytes += width;
        offset += codePoint > 0xffff ? 2 : 1;
      }
      return text.length;
    }

    function highlightInlineMatch(code, range) {
      if (range.end <= range.start) return;
      const walker = document.createTreeWalker(code, NodeFilter.SHOW_TEXT);
      const segments = [];
      let byteOffset = 0;
      while (walker.nextNode()) {
        const node = walker.currentNode;
        const length = new TextEncoder().encode(node.data).length;
        const start = Math.max(range.start, byteOffset);
        const end = Math.min(range.end, byteOffset + length);
        if (start < end) segments.push({ node, start: start - byteOffset, end: end - byteOffset });
        byteOffset += length;
      }
      for (const segment of segments.reverse()) {
        const start = codeUnitOffsetForByteOffset(segment.node.data, segment.start);
        const end = codeUnitOffsetForByteOffset(segment.node.data, segment.end);
        const matched = start ? segment.node.splitText(start) : segment.node;
        if (end - start < matched.data.length) matched.splitText(end - start);
        const mark = document.createElement('mark');
        mark.className = 'inline-match';
        matched.replaceWith(mark);
        mark.append(matched);
      }
    }

    function highlightInlineMatches(detail) {
      detail.querySelectorAll('code[data-match-ranges]').forEach(code => {
        try {
          JSON.parse(code.dataset.matchRanges)
            .sort((left, right) => right.start - left.start)
            .forEach(range => highlightInlineMatch(code, range));
        } catch (_) {
          // Ignore malformed match metadata from legacy or hand-edited reports.
        }
      });
    }

    function sourceAnchorForGap(button) {
      const source = button.closest('.source');
      if (!source) return null;
      const matches = [...source.querySelectorAll('.code-line.match')];
      const gapBottom = button.getBoundingClientRect().bottom;
      const following = matches.find(line => line.getBoundingClientRect().top >= gapBottom);
      const anchor = following || matches.at(-1);
      return anchor ? { line: anchor.dataset.sourceLine, top: anchor.getBoundingClientRect().top } : null;
    }

    function restoreSourceAnchor(detail, anchor) {
      if (!anchor) return;
      requestAnimationFrame(() => {
        const line = detail.querySelector(`.code-line.match[data-source-line="${anchor.line}"]`);
        if (line) detail.scrollTop += line.getBoundingClientRect().top - anchor.top;
      });
    }

    function sidePaths(entry) {
      const oldPaths = (Array.isArray(entry.old) ? entry.old : [entry.old]).filter(Boolean).map(side => side.path).join(', ') || 'none';
      const newPaths = (Array.isArray(entry.new) ? entry.new : [entry.new]).filter(Boolean).map(side => side.path).join(', ') || 'none';
      return `<div class="side-paths"><div class="side-path"><b>Old path</b>${escapeHtml(oldPaths)}</div><div class="side-path"><b>New path</b>${escapeHtml(newPaths)}</div></div>`;
    }

    function renderWarnings(warnings = []) {
      return warnings.map(warning => `<div class="warning">${escapeHtml(warning)}</div>`).join('');
    }

    function renderDiffLine(line) {
      const className = line.kind.replace('_', '-');
      const oldText = line.old_text == null ? '' : escapeHtml(line.old_text);
      const newText = line.new_text == null ? '' : escapeHtml(line.new_text);
      return `<div class="diff-line ${className}"><span>${line.old_line || ''}</span><span>${line.new_line || ''}</span><code>${oldText || ' '}</code><code>${newText || ' '}</code></div>`;
    }

    function renderCorrelationDetail(entry) {
      const badges = `<div class="badges"><span class="badge target">${escapeHtml(classificationLabel(entry.classification))}</span><span class="badge">${escapeHtml(entry.entry_type.replace('_', ' '))}</span></div>`;
      const warnings = [...(state.report.compatibility.warnings || []), ...(entry.warnings || [])];
      if (entry.entry_type === 'shared_context') {
        const comparison = entry.region_comparison;
        if (!comparison.available) {
          return `<h2>${escapeHtml(entry.path)}</h2>${badges}${sidePaths(entry)}${renderWarnings(warnings)}<p class="source-unavailable">Shared-context comparison is unavailable because source evidence is missing.</p>`;
        }
        const expanded = state.expandedRenamePaths.has(entry.path);
        const renamed = comparison.regions.filter(region => region.classification === 'rename_equivalent');
        const lines = comparison.regions.filter(region => region.classification !== 'unchanged' && (expanded || region.classification !== 'rename_equivalent')).map(region => renderDiffLine({
          kind: region.classification === 'rename_equivalent' ? 'rename_equivalent' : (region.old_line && region.new_line ? 'modification' : region.old_line ? 'deletion' : 'addition'),
          old_line: region.old_line, new_line: region.new_line, old_text: region.old_text, new_text: region.new_text,
        })).join('');
        const toggle = renamed.length ? `<button class="rename-toggle" type="button" data-rename-toggle>${expanded ? 'Hide' : 'Show'} ${number(renamed.length)} rename-equivalent ${renamed.length === 1 ? 'region' : 'regions'}</button>` : '';
        return `<h2>${escapeHtml(entry.path)}</h2>${badges}${sidePaths(entry)}${renderWarnings(warnings)}<p class="path">Matched-region coverage: ${number(comparison.paired_regions)} paired, ${number(comparison.old_only_regions)} old-only, ${number(comparison.new_only_regions)} new-only. Surrounding file text is intentionally not diffed.</p>${toggle}${lines ? `<div class="diff">${lines}</div>` : '<p class="source-unavailable">No substantive matched-region differences.</p>'}`;
      }
      if (!entry.smart_diff) {
        return `<h2>${escapeHtml(entry.path)}</h2>${badges}${sidePaths(entry)}${renderWarnings(warnings)}<p class="source-unavailable">No diff is computed for unmatched or ambiguous entries. Original evidence is embedded in the report.</p>`;
      }
      const expanded = state.expandedRenamePaths.has(entry.path);
      const substantive = entry.smart_diff.lines.filter(line => !['unchanged', 'rename_equivalent'].includes(line.kind));
      const renamed = entry.smart_diff.lines.filter(line => line.kind === 'rename_equivalent');
      const visible = expanded ? entry.smart_diff.lines.filter(line => line.kind !== 'unchanged') : substantive;
      const toggle = renamed.length ? `<button class="rename-toggle" type="button" data-rename-toggle>${expanded ? 'Hide' : 'Show'} ${number(renamed.length)} rename-equivalent ${renamed.length === 1 ? 'line' : 'lines'}</button>` : '';
      const diff = visible.length ? `<div class="diff">${visible.map(renderDiffLine).join('')}</div>` : '<p class="source-unavailable">No substantive differences. Unchanged and rename-equivalent lines are hidden by default.</p>';
      return `<h2>${escapeHtml(entry.path)}</h2>${badges}${sidePaths(entry)}${renderWarnings(warnings)}<p class="path">Smart diff: ${escapeHtml(entry.smart_diff.classification.replace('_', ' '))}. Original text and line numbers are preserved.</p>${toggle}${diff}`;
    }

    function renderDetail() {
      applySidebarState();
      const detail = document.querySelector('#detail');
      const file = state.report.files.find(candidate => candidate.path === state.selected);
      if (!file) {
        detail.innerHTML = '<div class="empty"><h2>Select a file</h2></div>';
        return;
      }
      if (state.report.report_type === 'topo_correlation') {
        const entry = state.report.entries.find(candidate => candidate.path === state.selected);
        detail.innerHTML = renderCorrelationDetail(entry);
        detail.querySelector('[data-rename-toggle]')?.addEventListener('click', () => {
          if (state.expandedRenamePaths.has(entry.path)) state.expandedRenamePaths.delete(entry.path);
          else state.expandedRenamePaths.add(entry.path);
          renderDetail();
        });
        return;
      }
      const matches = byFile.get(file.path) || [];
      const target = file.is_target ? '<span class="badge target">path match</span>' : '<span class="badge">content match</span>';
      const count = `<span class="badge">${number(file.match_count)} content hits</span>`;
      detail.innerHTML = `<h2>${renderFilename(file)}</h2><div class="path">${renderPath(file)}</div><div class="badges">${target}${count}</div>${renderSource(file, matches)}`;
      highlightInlineMatches(detail);
      detail.querySelectorAll('.code-gap').forEach(button => button.addEventListener('click', () => {
        const anchor = sourceAnchorForGap(button);
        state.expandedSourceRanges.add(button.dataset.sourceRange);
        renderDetail();
        restoreSourceAnchor(detail, anchor);
      }));
    }

    function renderHeader() {
      if (state.report.report_type === 'topo_correlation') {
        const oldMetadata = state.report.old_report.metadata;
        const newMetadata = state.report.new_report.metadata;
        const entries = state.report.entries;
        document.querySelector('#context').innerHTML = `
          <span class="context-item"><strong>${escapeHtml(oldMetadata.regex)}</strong> → <strong>${escapeHtml(newMetadata.regex)}</strong></span>
          <span class="context-item">${escapeHtml(displayPath(oldMetadata.scan_directory))}</span>
          <span class="context-item">Scanned ${escapeHtml(new Date(oldMetadata.searched_at_unix_seconds * 1000).toLocaleString())} → ${escapeHtml(new Date(newMetadata.searched_at_unix_seconds * 1000).toLocaleString())}</span>`;
        document.querySelector('#stats').innerHTML = `
          <div class="stat"><b>${number(entries.length)}</b><span>entries</span></div>
          <div class="stat"><b>${number(entries.filter(entry => entry.classification === 'paired').length)}</b><span>paired</span></div>
          <div class="stat"><b>${number(entries.filter(entry => ['old_only', 'new_only'].includes(entry.classification)).length)}</b><span>unmatched</span></div>
          <div class="stat"><b>${number(entries.filter(entry => entry.classification === 'ambiguous').length)}</b><span>ambiguous</span></div>`;
        return;
      }
      const metadata = state.report.metadata;
      document.querySelector('#context').innerHTML = `
        <span class="context-item" aria-label="Scan directory"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 4H2c-1.11 0-1.99.89-1.99 2L0 18c0 1.11.89 2 2 2h20c1.11 0 2-.89 2-2V8c0-1.11-.89-2-2-2H12l-2-2z"/></svg><span>${escapeHtml(displayPath(metadata.scan_directory))}</span></span>
        <span class="context-item" aria-label="Search pattern"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9.5 3a6.5 6.5 0 0 0 0 13c1.61 0 3.09-.59 4.23-1.57l.27.27v.79l5 4.99L20.49 20l-4.99-5h-.79l-.27-.27A6.47 6.47 0 0 0 16 9.5C16 5.91 13.09 3 9.5 3zm0 2C11.99 5 14 7.01 14 9.5S11.99 14 9.5 14 5 11.99 5 9.5 7.01 5 9.5 5z"/></svg><strong>${escapeHtml(metadata.regex)}</strong></span>`;
      const files = state.report.files;
      const targets = files.filter(file => file.is_target).length;
      const sprinkles = files.filter(file => !file.is_target && file.match_count > 0).length;
      const totalHits = state.report.matches.length;
      document.querySelector('#stats').innerHTML = `
        <div class="stat"><b>${number(files.length)}</b><span>files</span></div>
        <div class="stat"><b>${number(totalHits)}</b><span>content hits</span></div>
        <div class="stat"><b>${number(targets)}</b><span>matched paths</span></div>
        <div class="stat"><b>${number(sprinkles)}</b><span>sprinkle files</span></div>`;
    }

    function filterFromUrl() {
      return new URLSearchParams(window.location.search).get('filter') || '';
    }

    function writeFilterToUrl() {
      const url = new URL(window.location);
      if (state.query) url.searchParams.set('filter', state.query);
      else url.searchParams.delete('filter');
      history.replaceState(null, '', url);
    }

    function setQuery(query, writeUrl = false) {
      state.query = query;
      state.filterSpec = parseFilter(query);
      document.querySelector('#query').value = query;
      if (writeUrl) writeFilterToUrl();
      renderTree();
    }

    async function boot() {
      try {
        state.report = await fetch('/report.json').then(response => {
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          return response.json();
        });
        if (state.report.report_type === 'topo_correlation') {
          state.report.metadata = state.report.old_report.metadata;
          state.report.files = state.report.entries.map(entry => ({
            path: entry.path,
            classification: entry.classification,
            match_count: [...(Array.isArray(entry.old) ? entry.old : [entry.old]), ...(Array.isArray(entry.new) ? entry.new : [entry.new])].filter(Boolean).reduce((sum, side) => sum + side.matches.length, 0),
            is_target: entry.classification === 'paired',
            path_match_ranges: [],
          }));
          const labels = { paths: 'Paired', content: 'Unmatched', sprinkles: 'Ambiguous' };
          document.querySelectorAll('.filter[data-filter]').forEach(button => { if (labels[button.dataset.filter]) button.textContent = labels[button.dataset.filter]; });
        }
        startTerrain(state.report.metadata);
        for (const match of state.report.matches || []) {
          if (!byFile.has(match.file)) byFile.set(match.file, []);
          byFile.get(match.file).push(match);
        }
        renderHeader();
        state.query = filterFromUrl();
        state.filterSpec = parseFilter(state.query);
        document.querySelector('#query').value = state.query;
        renderTree();
        renderDetail();
        document.querySelector('#query').addEventListener('input', event => setQuery(event.target.value, true));
        window.addEventListener('popstate', () => setQuery(filterFromUrl()));
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
        setupSidebarToggle();
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
    let mut report: Value = serde_json::from_slice(&report)
        .map_err(|error| format!("{} is not valid JSON: {error}", report_path.display()))?;
    validate_report(&report)?;
    add_highlighted_lines(&mut report);
    let report = serde_json::to_vec(&report)
        .map_err(|error| format!("could not prepare viewer data: {error}"))?;

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

fn validate_report(report: &Value) -> Result<(), String> {
    let report_type = report
        .get("report_type")
        .and_then(Value::as_str)
        .unwrap_or("topo_map");
    let version = report
        .get("format_version")
        .and_then(Value::as_u64)
        .ok_or("report is missing a numeric format_version")?;
    match report_type {
        "topo_map" if version <= crate::FORMAT_VERSION as u64 => Ok(()),
        "topo_correlation" if version <= 1 => Ok(()),
        "topo_map" | "topo_correlation" => Err(format!(
            "unsupported {report_type} format version {version}"
        )),
        _ => Err(format!("unsupported report type `{report_type}`")),
    }
}

fn add_highlighted_lines(report: &mut Value) {
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = ThemeSet::load_defaults();
    let Some(theme) = theme_set
        .themes
        .get("base16-ocean.dark")
        .or_else(|| theme_set.themes.values().next())
    else {
        return;
    };
    let Some(files) = report.get_mut("files").and_then(Value::as_array_mut) else {
        return;
    };

    for file in files {
        let Some(file_object) = file.as_object_mut() else {
            continue;
        };
        let Some(path) = file_object
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let Some(content) = file_object
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        file_object.insert(
            "highlighted_lines".to_owned(),
            Value::Array(
                highlighted_lines(&content, &path, &syntax_set, theme)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
}

fn highlighted_lines(
    content: &str,
    path: &str,
    syntax_set: &SyntaxSet,
    theme: &syntect::highlighting::Theme,
) -> Vec<String> {
    let syntax = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| syntax_set.find_syntax_by_extension(extension))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme);
    let source_lines = if content.is_empty() {
        vec![""]
    } else {
        content.split_terminator('\n').collect()
    };

    source_lines
        .into_iter()
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            highlighter
                .highlight_line(line, syntax_set)
                .and_then(|regions| {
                    styled_line_to_highlighted_html(&regions, IncludeBackground::No)
                })
                .unwrap_or_else(|_| escape_html(line))
        })
        .collect()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
        assert!(VIEWER_HTML.contains("<h1 class=\"brand\">TOPO</h1>"));
        assert!(VIEWER_HTML.contains("id=\"tree\""));
        assert!(VIEWER_HTML.contains("id=\"detail\""));
        assert!(VIEWER_HTML.contains("data-filter=\"paths\">Paths"));
        assert!(VIEWER_HTML.contains("data-filter=\"content\">Content"));
        assert!(VIEWER_HTML.contains("new URLSearchParams(window.location.search)"));
        assert!(VIEWER_HTML.contains("history.replaceState"));
        assert!(VIEWER_HTML.contains("id=\"tree-control\""));
        assert!(VIEWER_HTML.contains("id=\"resize-handle\""));
        assert!(VIEWER_HTML.contains("id=\"sidebar-toggle\""));
        assert!(VIEWER_HTML.contains("localStorage.setItem(SIDEBAR_WIDTH_KEY"));
        assert!(VIEWER_HTML.contains("SIDEBAR_COLLAPSED_KEY"));
        assert!(VIEWER_HTML.contains("event.key !== '/'"));
        assert!(VIEWER_HTML.contains("state.expanded"));
        assert!(VIEWER_HTML.contains("highlighted_lines"));
        assert!(VIEWER_HTML.contains("code-gap"));
        assert!(VIEWER_HTML.contains("path_match_ranges"));
        assert!(VIEWER_HTML.contains("inline-match"));
        assert!(VIEWER_HTML.contains("restoreSourceAnchor"));
        assert!(!VIEWER_HTML.contains('◆'));
        assert!(
            viewer_html(Some("/Users/test")).contains(r#"const HOME_DIRECTORY = "/Users/test";"#)
        );
    }

    #[test]
    fn viewer_adds_highlighted_ruby_lines_to_source_files() {
        let mut report = serde_json::json!({
            "files": [{
                "path": "app/report.rb",
                "content": "class Report\n  def call\n  end\nend\n"
            }]
        });

        add_highlighted_lines(&mut report);

        let lines = report["files"][0]["highlighted_lines"].as_array().unwrap();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].as_str().unwrap().contains("class"));
    }

    #[test]
    fn viewer_dispatches_legacy_maps_and_correlation_reports() {
        assert!(validate_report(&serde_json::json!({ "format_version": 7 })).is_ok());
        assert!(
            validate_report(
                &serde_json::json!({ "report_type": "topo_correlation", "format_version": 1 })
            )
            .is_ok()
        );
        assert!(
            validate_report(&serde_json::json!({ "report_type": "unknown", "format_version": 1 }))
                .is_err()
        );
        assert!(VIEWER_HTML.contains("renderCorrelationDetail"));
        assert!(
            VIEWER_HTML
                .contains(r"['old_only', 'new_only'].includes(classification) ? '\u00a0' : ' '")
        );
        assert!(VIEWER_HTML.contains("classificationLabel(file.classification)"));
        assert!(VIEWER_HTML.contains("classificationLabel(entry.classification)"));
        assert!(VIEWER_HTML.contains("expandedRenamePaths"));
        assert!(VIEWER_HTML.contains("shared_context"));
    }
}
