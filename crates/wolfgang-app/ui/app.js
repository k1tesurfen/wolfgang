// Wolfgang's office: the scene, the conversation and the panels.
// Talks to Rust through Tauri (or to mock.js when opened in a normal browser).
'use strict';

const T = window.__TAURI__;
const api = T
  ? {
      invoke: (cmd, args) => T.core.invoke(cmd, args),
      listen: (ev, f) => T.event.listen(ev, e => f(e.payload)),
      onDrop: f => T.webview.getCurrentWebview().onDragDropEvent(e => f(e.payload)),
    }
  : window.WolfgangMock;

const $ = id => document.getElementById(id);

// Errors are reported to Rust once the UI is up (logged; WOLFGANG_SELFTEST=1 exits on them).
const uiErrors = [];
addEventListener('error', e => uiErrors.push(`${e.message} (${e.filename?.split('/').pop()}:${e.lineno})`));
addEventListener('unhandledrejection', e => uiErrors.push(`unhandled: ${e.reason}`));
const sleep = ms => new Promise(r => setTimeout(r, ms));
// Typewriter: a comfortable pace for short lines, but a whole bubble never takes
// much longer than TYPE_BUDGET_MS (long texts like the summary simply type faster).
const TYPE_MS = 22;
const TYPE_MIN_MS = 6;
const TYPE_BUDGET_MS = 1800;

let SCENE, IMG = {}, UI;
let scale = 4;
let mode = 'idle';          // idle | talk | work | panel
let token = 0;              // bumps on every new view; stale async work checks it
let typing = null;          // { finish() } while the typewriter runs
let choices = [], sel = 0;  // current answers: [{ label, current, small, run }]
let hover = null;

/* ── Loading ───────────────────────────────────────────────────────────── */

const load = src => new Promise((ok, err) => { const i = new Image(); i.onload = () => ok(i); i.onerror = err; i.src = src; });

async function init() {
  SCENE = await (await fetch('pixel/scene.json')).json();
  const files = [SCENE.background, SCENE.wolfgang.sheet, SCENE.ui.sheet, ...Object.values(SCENE.portraits)];
  for (const f of files) IMG[f] = await load('pixel/' + f);
  UI = SCENE.ui.frames;

  try { Sound.enabled = (await api.invoke('settings')).sound; } catch { /* keep default */ }

  fit();
  addEventListener('resize', fit);
  document.fonts?.ready.then(restyle);
  requestAnimationFrame(tick);

  api.onDrop(onDrop);
  // Files dropped on the Dock icon: Rust keeps them until we ask (also covers app launch).
  const takePending = async () => { const p = await api.invoke('take_pending'); if (p.length) dropFiles(p); };
  api.listen('files-opened', takePending);
  api.listen('progress', onProgress);
  api.listen('finished', onFinished);
  api.listen('failed', msg => { mode = 'talk'; play('idle'); oops(msg); });
  api.listen('update-progress', onUpdateProgress);

  setupStage();
  goIdle();
  if (T) takePending();
  if (T) setTimeout(() => api.invoke('frontend_ready', { errors: uiErrors, scale }), 1500);
  setTimeout(checkUpdate, 2500);

  // Demo hooks for looking at the UI in a browser (mock.js).
  const q = new URLSearchParams(location.search);
  if (!T && q.has('demo')) dropFiles(['/demo/' + q.get('demo')]);
  if (!T && q.has('panel')) openPanel(q.get('panel'));
  if (!T && q.get('update') === 'install') setTimeout(installUpdate, 3500);
}

/* ── Scaling and pixel frames ──────────────────────────────────────────── */

function fit() {
  scale = Math.max(1, Math.floor(Math.min(innerWidth / 320, innerHeight / 180)));
  document.documentElement.style.setProperty('--scale', scale);
  restyle();
}

const pieces = {};
function piece(name) {
  if (!pieces[name]) {
    const f = UI[name], c = document.createElement('canvas');
    c.width = f.w; c.height = f.h;
    c.getContext('2d').drawImage(IMG[SCENE.ui.sheet], f.x, f.y, f.w, f.h, 0, 0, f.w, f.h);
    pieces[name] = c.toDataURL();
  }
  return pieces[name];
}

function nine(el, name) {
  const s = UI[name].slice;
  el.style.borderImage = `url(${piece(name)}) ${s} fill / ${s * scale}px stretch`;
  el.style.borderWidth = `${s * scale}px`;
}

function restyle() {
  if (!UI) return;
  nine($('say'), 'bubble');
  nine($('portrait'), 'portrait-frame');
  nine($('panel'), 'panel');
  document.querySelectorAll('.answer').forEach((a, i) => nine(a, i === sel && !a.classList.contains('input') ? 'answer-hover' : 'answer'));
  document.querySelectorAll('.btn').forEach(b => nine(b, b.classList.contains('on') ? 'button-on' : 'button'));
  document.querySelectorAll('.cursor').forEach(c => (c.style.backgroundImage = `url(${piece('cursor')})`));
  markPanelFocus();
  placeTail();
}

function placeTail() {
  const t = $('tail'), say = $('say');
  t.hidden = say.hidden;
  if (say.hidden) return;
  t.style.backgroundImage = `url(${piece('tail')})`;
  t.style.left = `${(SCENE.speaker[0] - 5) * scale}px`;
  t.style.top = `${say.offsetTop + say.offsetHeight - 2 * scale}px`;
}

/* ── Wolfgang's animation ──────────────────────────────────────────────── */

let anim = 'idle', step = 0, stepAt = 0, animTimer = null;

function play(name, ms, then = 'idle') {
  if (!SCENE.wolfgang.animations[name]) return;
  anim = name; step = 0; stepAt = performance.now();
  clearTimeout(animTimer);
  if (ms) animTimer = setTimeout(() => play(then), ms);
}

function tick(now) {
  const ctx = $('scene').getContext('2d');
  ctx.imageSmoothingEnabled = false;
  const W = SCENE.wolfgang, steps = W.animations[anim];
  while (steps[step][1] > 0 && now - stepAt > steps[step][1]) {
    stepAt += steps[step][1];
    step = (step + 1) % steps.length;
  }
  ctx.drawImage(IMG[SCENE.background], 0, 0);
  const f = W.frames[steps[step][0]];
  ctx.drawImage(IMG[W.sheet], f.x, f.y, f.w, f.h, W.origin[0], W.origin[1], f.w, f.h);
  if (hover && mode !== 'work') {
    const r = SCENE.hotspots[hover].rects[0];
    const sp = SCENE.ui.animations.sparkle, s = UI[sp[Math.floor(now / 260) % sp.length][0]];
    ctx.drawImage(IMG[SCENE.ui.sheet], s.x, s.y, s.w, s.h, r.x + r.w - 4, r.y - 4, s.w, s.h);
  }
  requestAnimationFrame(tick);
}

// Now and then, when nobody needs him, Wolfgang has a sip of coffee.
setInterval(() => { if (mode === 'idle' && anim === 'idle' && Math.random() < 0.35) { play('sip', 2300); Sound.sip(); } }, 20000);

/* ── Speaking ──────────────────────────────────────────────────────────── */

function setFace(mood) {
  if (!mood) { $('portrait').hidden = true; return; }
  const options = SCENE.moods[mood] || SCENE.moods.neutral;
  $('face').src = 'pixel/' + SCENE.portraits[options[Math.floor(Math.random() * options.length)]];
  $('portrait').hidden = false;
}

/** Shows lines in the bubble with a typewriter; resolves when all text is out. */
function say(lines, { mood = null, idle = false } = {}) {
  typing?.finish();
  const box = $('say'), text = $('say-text'), my = token;
  box.hidden = mode === 'panel'; // a panel covers the scene; the bubble comes back when it closes
  box.classList.toggle('idle', idle);
  $('progress').hidden = true;
  setFace(mood);
  fitText(lines);
  const paras = lines.map(() => { const p = document.createElement('p'); text.appendChild(p); return p; });
  placeTail();
  if (!lines.length) return Promise.resolve();
  if (!idle) play('talk');

  const chars = lines.reduce((n, l) => n + l.length, 0);
  const per = Math.max(TYPE_MIN_MS, Math.min(TYPE_MS, TYPE_BUDGET_MS / Math.max(1, chars)));
  const pause = Math.round(per * 5);

  return new Promise(done => {
    let li = 0, ci = 0, timer = null;
    const end = () => {
      clearTimeout(timer);
      paras.forEach((p, i) => (p.textContent = lines[i]));
      typing = null;
      if (anim === 'talk') play('idle');
      placeTail();
      done();
    };
    typing = { finish: end };
    const next = () => {
      if (my !== token) return end();
      if (li >= lines.length) return end();
      ci++;
      paras[li].textContent = lines[li].slice(0, ci);
      if (lines[li][ci - 1] && lines[li][ci - 1] !== ' ' && ci % 2) Sound.blip();
      if (ci >= lines[li].length) { li++; ci = 0; timer = setTimeout(next, pause + 60); }
      else timer = setTimeout(next, /[.,!?…:]/.test(lines[li][ci - 1]) ? pause : per);
    };
    next();
  });
}

/** Long text gets a smaller font until the bubble fits above Wolfgang's head.
 *  Leaves the bubble empty, but already at its final height. */
function fitText(lines) {
  const box = $('say'), text = $('say-text');
  text.style.minHeight = '';
  text.innerHTML = lines.map(l => `<p>${escapeHtml(l)}</p>`).join('');
  const max = (box.classList.contains('idle') ? 40 : 58) * scale;
  let size = box.classList.contains('idle') ? 6 : 6.5;
  box.style.fontSize = '';
  while (box.offsetHeight > max && size > 4.6) {
    size -= 0.3;
    box.style.fontSize = `${size * scale}px`;
  }
  // Keep the final height while typing, so the bubble (and its tail) doesn't grow line by line.
  text.style.minHeight = `${text.offsetHeight}px`;
  text.innerHTML = '';
}

const escapeHtml = s => s.replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

/* ── Answers ───────────────────────────────────────────────────────────── */

function setChoices(list, { input = null } = {}) {
  choices = list;
  sel = Math.max(0, list.findIndex(c => c.current));
  const box = $('answers');
  box.innerHTML = '';
  // Real answers first (one per line), then small extras side by side (back, Finder …).
  const order = [...list.keys()].sort((a, b) => !!list[a].small - !!list[b].small);
  choices = order.map(i => list[i]);
  const row = document.createElement('div');
  row.className = 'answer-row';
  choices.forEach((c, i) => {
    const a = document.createElement('div');
    a.className = 'answer' + (c.small ? ' small' : '');
    a.innerHTML = `<div class="cursor"></div>${escapeHtml(c.label)}${c.current ? '<span class="star">★</span>' : ''}`;
    a.onmouseenter = () => select(i);
    a.onclick = () => activate(i);
    (c.small ? row : box).appendChild(a);
  });
  if (row.children.length) box.appendChild(row);
  sel = Math.max(0, choices.findIndex(c => c.current));
  if (input) showInput(input, list);
  select(sel, true);
}

function select(i, quiet) {
  if (!choices.length) return;
  sel = (i + choices.length) % choices.length;
  document.querySelectorAll('#answers .answer').forEach((a, j) => a.classList.toggle('sel', j === sel));
  if (!quiet) Sound.move();
  restyle();
}

function activate(i) {
  const c = choices[i];
  if (!c) return;
  typing?.finish(); // answering while Wolfgang still talks is fine
  c.back ? Sound.back() : Sound.confirm();
  c.run();
}

/** The custom answer: a text field in answer style. */
function showInput(spec, others = []) {
  const box = $('answers');
  box.innerHTML = '';
  choices = [];
  const a = document.createElement('div');
  a.className = 'answer input sel';
  const hint = spec.kind === 'number' && spec.min != null
    ? `${spec.min}–${spec.max}${spec.unit ? ' ' + spec.unit : ''} · Enter · Esc = zurück`
    : `Enter · Esc = zurück`;
  a.innerHTML = `<div class="cursor"></div><input type="text" spellcheck="false" autocomplete="off"><div class="hint">${hint}</div>`;
  box.appendChild(a);
  const field = a.querySelector('input');
  field.placeholder = spec.placeholder || '';
  if (spec.kind === 'number') field.inputMode = 'numeric';
  field.onkeydown = e => {
    e.stopPropagation();
    if (e.key === 'Enter' && field.value.trim()) { Sound.confirm(); act(api.invoke('type_answer', { text: field.value })); }
    if (e.key === 'Escape' || (e.ctrlKey && e.key === '[')) { Sound.back(); others.length ? setChoices(others) : back(); }
  };
  restyle();
  setTimeout(() => field.focus(), 0);
}

/* Keyboard. Everyone: ↑↓ Enter Esc Space, 1–9, ⌘O, ⌘,, ?. On top, for vim
   people, letters that do nothing otherwise: j k l h g G i / o s p q, Ctrl-[.
   None of it applies while typing in a text field. */
addEventListener('keydown', e => {
  // ⌘+ / ⌘− / ⌘0: bigger, smaller, best fit (always whole pixel steps).
  if (e.metaKey && T && ['=', '+', '-', '0'].includes(e.key)) {
    e.preventDefault();
    api.invoke('zoom', { delta: e.key === '-' ? -1 : e.key === '0' ? 0 : 1 });
    return;
  }
  if (e.target.tagName === 'INPUT') return;
  if (e.metaKey && e.key === 'o') { e.preventDefault(); pickFiles(); return; }
  if (e.metaKey && e.key === ',') { e.preventDefault(); openPanel('settings'); return; }
  const key = e.ctrlKey && e.key === '[' ? 'Escape' : e.key;
  if ((e.metaKey || e.ctrlKey || e.altKey) && key !== 'Escape') return; // leave other shortcuts to macOS
  if (mode === 'panel') return panelKey(e, key);
  if (typing && key === ' ') { e.preventDefault(); typing.finish(); return; } // Space skips; Enter answers right away
  const first = choices.findIndex(c => !c.small);
  switch (key) {
    case 'ArrowDown': case 'j': select(sel + 1); break;
    case 'ArrowUp': case 'k': select(sel - 1); break;
    case 'g': select(0); break;
    case 'G': select(choices.length - 1); break;
    case 'Enter': case ' ': case 'l': activate(sel); break;
    case 'Escape': case 'h': { const b = choices.findIndex(c => c.back); if (b >= 0) activate(b); break; }
    case 'i': case '/': { const t = choices.findIndex(c => c.typed); if (t >= 0) activate(t); break; }
    case 'o': pickFiles(); break;
    case 's': openPanel('settings'); break;
    case 'p': openPanel('presets'); break;
    case '?': openPanel('keys'); break;
    default:
      // 1–9: that answer right away (the big ones, in the order shown).
      if (/^[1-9]$/.test(key) && first >= 0 && choices[first + Number(key) - 1] && !choices[first + Number(key) - 1].small) activate(first + Number(key) - 1);
      else return;
  }
  e.preventDefault();
});

/** The Finder dialog: files and folders, several at once (desk click, ⌘O, o). */
async function pickFiles() {
  if (mode === 'work' || picking) return;
  picking = true;
  try {
    const paths = await api.invoke('pick_files');
    if (paths.length) dropFiles(paths);
  } catch (e) {
    oops(e);
  } finally {
    picking = false;
  }
}
let picking = false;

/* ── Conversation ──────────────────────────────────────────────────────── */

function act(promise) {
  promise.then(render).catch(oops);
}

async function oops(msg) {
  token++;
  mode = 'talk';
  setChoices([]);
  Sound.sad();
  await say([String(msg)], { mood: 'sorry' });
  setChoices([{ label: 'Okay', run: goIdle }]);
}

async function dropFiles(paths) {
  if (mode === 'work') {
    say(['Moment, ich bin noch mit dem letzten Stapel beschäftigt.'], { mood: 'thinking' });
    return;
  }
  closePanel();
  Sound.plop();
  token++;
  mode = 'talk';
  setChoices([]);
  say(['Moment, ich guck mir das an …'], { idle: true });
  play('happy', 900);
  act(api.invoke('drop_files', { paths }));
}

async function render(view) {
  const my = ++token;
  mode = 'talk';
  setChoices([]);
  const extra = [];
  if (view.output && view.type !== 'start') extra.push({ label: '📂 Im Finder zeigen', small: true, run: () => api.invoke('reveal', { path: view.output }) });

  if (view.type === 'ask') {
    if (view.node === 'done') play('hand', 2600, 'happy');
    const talking = say(view.lines, { mood: view.mood });
    const list = view.answers.map((a, i) => ({ label: a.label, current: a.current, run: () => act(api.invoke('choose', { index: i })) }));
    if (view.input && list.length) list.push({ label: '✎ Selbst eintippen …', typed: true, run: () => showInput(view.input, choices) });
    list.push(...extra);
    if (view.can_go_back) list.push({ label: '← Zurück', small: true, back: true, run: back });
    // Answers show up right away: no need to wait for Wolfgang to finish talking.
    setChoices(list, { input: view.input && !view.answers.length ? view.input : null });
    await talking;
  } else if (view.type === 'start') {
    await say(view.lines, { mood: view.mood });
    if (my !== token) return;
    startWork();
  } else {
    // end / cancelled: say goodbye, then back to the empty desk.
    await say(view.lines, { mood: view.mood });
    if (my !== token) return;
    setChoices([...extra, { label: 'Danke, Wolfgang!', run: goIdle }]);
    if (!extra.length) setTimeout(() => my === token && goIdle(), 6000);
  }
}

function back() { act(api.invoke('back')); }

async function goIdle() {
  if (update && !update.offered) return offerUpdate();
  const my = ++token;
  mode = 'idle';
  setChoices([{ label: 'Dateien auswählen …', small: true, run: pickFiles }]);
  play('idle');
  const line = await api.invoke('idle_line').catch(() => 'Leg mir einfach was auf den Tisch.');
  if (my === token) say([line], { idle: true });
}

/* ── Working ───────────────────────────────────────────────────────────── */

let workLineTimer = null;

async function startWork() {
  mode = 'work';
  play('type');
  const line = await api.invoke('working_line');
  $('say-text').innerHTML = `<p>${escapeHtml(line)}</p>`;
  $('progress').hidden = false;
  $('fill').style.width = '0%';
  $('progress-label').textContent = 'Geht los …';
  placeTail();
  setChoices([{ label: 'Abbrechen', small: true, back: true, run: () => { api.invoke('cancel'); $('progress-label').textContent = 'Okay, ich hör auf …'; } }]);
  clearInterval(workLineTimer);
  workLineTimer = setInterval(async () => {
    if (mode !== 'work') return clearInterval(workLineTimer);
    const l = await api.invoke('working_line');
    const p = $('say-text').querySelector('p');
    if (p) p.textContent = l;
  }, 4500);
  api.invoke('start').catch(oops);
}

function onProgress(p) {
  if (mode !== 'work') return;
  $('fill').style.width = `${Math.round((p.fraction ?? p.done / Math.max(1, p.total)) * 100)}%`;
  $('progress-label').textContent = `${p.done} von ${p.total} · ${p.current}`;
  placeTail();
  if (p.ok !== null && p.ok !== undefined) Sound.stamp();
}

function onFinished(view) {
  clearInterval(workLineTimer);
  $('progress').hidden = true;
  Sound.jingle();
  render(view);
}

/* ── Updates ───────────────────────────────────────────────────────────── */

let update = null; // { version, offered } when a newer Wolfgang is on GitHub

async function checkUpdate() {
  const found = await api.invoke('check_update').catch(() => null);
  if (!found) return;
  update = { ...found, offered: false };
  if (mode === 'idle') offerUpdate(); // otherwise he asks once the current pile is done
}

async function offerUpdate() {
  update.offered = true;
  token++;
  mode = 'talk';
  play('happy', 900);
  const talking = say([`Psst – es gibt 'nen neuen Wolfgang: Version ${update.version}.`, 'Soll ich mich mal eben auffrischen? Dauert nur kurz, dann bin ich gleich wieder da.'], { mood: 'happy' });
  setChoices([{ label: 'Ja, mach mal!', run: installUpdate }, { label: 'Später', run: goIdle }]);
  await talking;
}

function installUpdate() {
  closePanel();
  token++;
  mode = 'work';
  play('type');
  setChoices([]);
  setFace(null);
  $('say').hidden = false;
  $('say').classList.remove('idle');
  $('say-text').innerHTML = `<p>Ich mach mich mal eben frisch …</p>`;
  $('progress').hidden = false;
  $('fill').style.width = '0%';
  $('progress-label').textContent = 'Hole den neuen Wolfgang …';
  placeTail();
  // On success the app restarts as the new version; we only come back here on errors.
  api.invoke('install_update').catch(e => { update = null; mode = 'talk'; play('idle'); oops(e); });
}

function onUpdateProgress(p) {
  if (mode !== 'work') return;
  const mb = n => (n / 1e6).toFixed(1).replace('.', ',');
  $('fill').style.width = p.total ? `${Math.round((p.done / p.total) * 100)}%` : '50%';
  $('progress-label').textContent = p.total && p.done >= p.total ? 'Gleich wieder da!' : `${mb(p.done)}${p.total ? ` von ${mb(p.total)}` : ''} MB`;
  placeTail();
}

/* ── Drag and drop, hotspots ───────────────────────────────────────────── */

function onDrop(e) {
  // The whole desk lights up (its biggest rect; the first one is the mat, for the sparkle).
  const desk = SCENE.hotspots.desk.rects.reduce((a, b) => (b.w * b.h > a.w * a.h ? b : a)), d = $('drop');
  if (e.type === 'enter' || e.type === 'over') {
    Object.assign(d.style, { left: `${desk.x * scale}px`, top: `${desk.y * scale}px`, width: `${desk.w * scale}px`, height: `${desk.h * scale}px` });
    d.classList.add('on');
    if (e.type === 'enter' && mode !== 'work') play('happy');
  } else if (e.type === 'leave') {
    d.classList.remove('on');
    if (anim === 'happy') play('idle');
  } else if (e.type === 'drop') {
    d.classList.remove('on');
    if (e.paths?.length) dropFiles(e.paths);
  }
}

function setupStage() {
  const stage = $('stage'), tip = $('tip');
  const at = e => { const r = stage.getBoundingClientRect(); return [(e.clientX - r.left) / scale, (e.clientY - r.top) / scale]; };
  // The smallest object under the pointer wins (the mug sits on the desk, not the other way round).
  const area = s => Math.max(...s.rects.map(r => r.w * r.h));
  const hit = ([x, y]) =>
    Object.entries(SCENE.hotspots)
      .filter(([, s]) => s.rects.some(r => x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h))
      .sort(([, a], [, b]) => area(a) - area(b))[0];
  stage.addEventListener('mousemove', e => {
    const overUi = e.target.closest('.bubble, .answer, #panel, #portrait');
    const h = overUi || mode === 'work' || mode === 'panel' ? null : hit(at(e));
    const clickable = !!h;
    hover = clickable ? h[0] : null;
    stage.style.cursor = clickable ? 'pointer' : 'default';
    tip.style.display = clickable ? 'block' : 'none';
    if (clickable) {
      const r = stage.getBoundingClientRect();
      tip.textContent = h[1].label;
      tip.style.left = `${e.clientX - r.left + 12}px`;
      tip.style.top = `${e.clientY - r.top + 12}px`;
    }
  });
  stage.addEventListener('click', e => {
    if (typing && e.target.closest('#say')) return typing.finish();
    if (e.target.closest('.bubble, .answer, #panel') || mode === 'work' || mode === 'panel') return;
    const h = hit(at(e));
    if (!h) return;
    const action = h[1].action;
    if (action === 'sip') { play('sip', 2300); Sound.sip(); }
    else if (action === 'pick') pickFiles();
    else if (action === 'settings' || action === 'presets' || action === 'about') openPanel(action);
  });
}

/* ── Panels: settings (cabinet), presets (clipboard), about (computer) ─── */

let panelReturn = null;

function openPanel(kind) {
  if (mode === 'work') return;
  panelReturn = mode;
  mode = 'panel';
  hover = null;
  $('tip').style.display = 'none';
  $('panel').hidden = false;
  $('answers').hidden = true;
  $('say').hidden = true;
  $('tail').hidden = true;
  panelFocus = { row: 0, col: 0, shown: false };
  ({ settings: settingsPanel, presets: presetsPanel, about: aboutPanel, keys: keysPanel })[kind]?.();
  restyle();
}

/* Keyboard in panels: j/k (↑↓) rows, h/l (←→) buttons in a row, Enter/Space
   presses, Esc/q closes. The highlight shows only once the keyboard is used. */
let panelFocus = { row: 0, col: 0, shown: false };

function panelRows() {
  const rows = [...$('panel-body').querySelectorAll('.row, .preset')].filter(r => r.querySelector('.btn'));
  return [...rows, $('panel-close')];
}

function panelKey(e, key) {
  const rows = panelRows();
  const f = panelFocus;
  const buttons = r => (r.classList.contains('btn') ? [r] : [...r.querySelectorAll('.btn')]);
  const move = () => { f.shown = true; f.row = Math.max(0, Math.min(rows.length - 1, f.row)); f.col = Math.max(0, Math.min(buttons(rows[f.row]).length - 1, f.col)); Sound.move(); markPanelFocus(); };
  // The first arrow or vim key only shows where you are (the chosen option of the first row).
  if (!f.shown && ['ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight', 'j', 'k', 'h', 'l', 'g', 'G'].includes(key)) {
    f.col = Math.max(0, buttons(rows[0]).findIndex(b => b.classList.contains('on')));
    move();
    e.preventDefault();
    return;
  }
  switch (key) {
    case 'Escape': case 'q': Sound.confirm(); closePanel(); break;
    case 'ArrowDown': case 'j': f.row++; f.col = Math.max(0, buttons(rows[Math.min(f.row, rows.length - 1)]).findIndex(b => b.classList.contains('on'))); move(); break;
    case 'ArrowUp': case 'k': f.row--; f.col = Math.max(0, buttons(rows[Math.max(f.row, 0)]).findIndex(b => b.classList.contains('on'))); move(); break;
    case 'ArrowRight': case 'l': f.col++; move(); break;
    case 'ArrowLeft': case 'h': f.col--; move(); break;
    case 'g': f.row = 0; move(); break;
    case 'G': f.row = rows.length - 1; move(); break;
    case 'Enter': case ' ':
      if (!f.shown) { Sound.confirm(); closePanel(); break; } // as before: Enter closes
      buttons(rows[f.row])[f.col]?.click();
      break;
    default: return;
  }
  e.preventDefault();
}

function markPanelFocus() {
  document.querySelectorAll('#panel .kbd, #panel .kbd-row').forEach(el => el.classList.remove('kbd', 'kbd-row'));
  if (mode !== 'panel' || !panelFocus.shown) return;
  const rows = panelRows();
  const row = rows[Math.min(panelFocus.row, rows.length - 1)];
  if (!row) return;
  const buttons = row.classList.contains('btn') ? [row] : [...row.querySelectorAll('.btn')];
  row.classList.add('kbd-row');
  buttons[Math.min(panelFocus.col, buttons.length - 1)]?.classList.add('kbd');
}

function keysPanel() {
  $('panel-title').textContent = 'Tastatur';
  const line = (k, what) => `<div class="key"><span class="k">${k}</span><span>${what}</span></div>`;
  $('panel-body').innerHTML = `
    <div class="keys">
      <div>
        <p class="muted">Für alle</p>
        ${line('↑ ↓', 'Antwort aussuchen')}
        ${line('Enter', 'antworten')}
        ${line('1 – 9', 'Antwort direkt')}
        ${line('Esc', 'zurück')}
        ${line('Leertaste', 'Text ganz zeigen')}
        ${line('⌘O', 'Dateien auswählen')}
        ${line('⌘,', 'Einstellungen')}
        ${line('⌘+ ⌘−', 'größer, kleiner')}
        ${line('?', 'diese Übersicht')}
      </div>
      <div>
        <p class="muted">Für Vim-Leute</p>
        ${line('j k', 'runter, rauf')}
        ${line('l', 'antworten')}
        ${line('h', 'zurück')}
        ${line('g G', 'erste, letzte Antwort')}
        ${line('i /', 'selbst eintippen')}
        ${line('o', 'Dateien auswählen')}
        ${line('s p', 'Einstellungen, Vorlagen')}
        ${line('q', 'Fenster zu')}
        ${line('Ctrl-[', 'wie Esc')}
      </div>
    </div>`;
}

function closePanel() {
  if ($('panel').hidden) return;
  $('panel').hidden = true;
  $('answers').hidden = false;
  $('say').hidden = false;
  mode = panelReturn === 'talk' ? 'talk' : 'idle';
  restyle();
}
$('panel-close').onclick = () => { Sound.confirm(); closePanel(); };

function button(label, on, run) {
  const b = document.createElement('div');
  b.className = 'btn' + (on ? ' on' : '');
  b.textContent = label;
  b.onclick = () => { Sound.confirm(); run(); };
  return b;
}

async function settingsPanel() {
  $('panel-title').textContent = 'Einstellungen';
  let d = await api.invoke('settings');
  const body = $('panel-body');
  const save = async patch => {
    d = await api.invoke('set_settings', { defaults: { ...d, ...patch } });
    Sound.enabled = d.sound;
    draw();
  };
  const row = (label, options, value, key) => {
    const r = document.createElement('div');
    r.className = 'row';
    r.innerHTML = `<div class="label">${label}</div>`;
    const seg = document.createElement('div');
    seg.className = 'seg';
    const values = options.map(o => o[1]);
    if (!values.includes(value)) options = [...options, [value === null ? 'Original' : `${value}${key === 'video_size' ? 'p' : ''}`, value]];
    options.forEach(([text, v]) => seg.appendChild(button(text, v === value, () => save({ [key]: v }))));
    r.appendChild(seg);
    body.appendChild(r);
  };
  const draw = () => {
    body.innerHTML = '';
    row('Fürs Web am liebsten', [['WebP', 'webp'], ['AVIF', 'avif']], d.web_format, 'web_format');
    row('Größe fürs Web', [['800', 800], ['1600', 1600], ['2400', 2400]], d.max_side, 'max_side');
    row('Qualität fürs Web', [['65', 65], ['80', 80], ['90', 90]], d.quality, 'quality');
    row('Hochwertig am liebsten', [['JPG', 'jpg'], ['WebP', 'webp'], ['AVIF', 'avif'], ['PNG', 'png']], d.hq_format, 'hq_format');
    row('Wörter trennen mit', [['bild-name', 'dash'], ['bild_name', 'underscore'], ['BildName', 'caps']], d.separator, 'separator');
    row('Videos am liebsten', [['MP4', 'mp4'], ['WebM', 'webm']], d.video_format, 'video_format');
    row('Videogröße', [['720p', 720], ['1080p', 1080], ['Original', null]], d.video_size, 'video_size');
    row('Videoqualität', [['Kleiner', 'smaller'], ['Ausgewogen', 'balanced'], ['Schöner', 'nicer']], d.video_quality, 'video_quality');
    row('Audio am liebsten', [['MP3', 'mp3'], ['AAC', 'aac']], d.audio_format, 'audio_format');
    row('Geräusche', [['An', true], ['Aus', false]], d.sound, 'sound');
    restyle();
  };
  draw();
}

async function presetsPanel() {
  $('panel-title').textContent = 'Vorlagen';
  const body = $('panel-body');
  const draw = list => {
    body.innerHTML = '';
    if (!list.length) {
      body.innerHTML = '<p class="muted">Noch keine Vorlagen. Nach einer Runde kann ich mir deine Einstellungen merken – sag einfach „Als Vorlage merken“.</p>';
    }
    list.forEach(p => {
      const r = document.createElement('div');
      r.className = 'preset';
      r.innerHTML = `<div><div>${escapeHtml(p.name)}</div><div class="desc">${escapeHtml(p.description)}</div></div>`;
      r.appendChild(button('Weg damit', false, async () => draw(await api.invoke('remove_preset', { name: p.name }))));
      body.appendChild(r);
    });
    restyle();
  };
  draw(await api.invoke('presets'));
}

async function aboutPanel() {
  $('panel-title').textContent = 'Über Wolfgang';
  const body = $('panel-body');
  const draw = (a, note = '') => {
    body.innerHTML = `
      <p>Wolfgang ${escapeHtml(a.version)}<br><span class="muted">Wandelt Bilder, Videos, Audio und Schriften um, damit du's nicht musst.</span></p>
      ${update ? `<div class="row update"><div class="label">Neu: Version ${escapeHtml(update.version)}</div></div>` : ''}
      <div class="row cli"><div class="label">Wolfgang im Terminal (<code>wolfgang</code>)</div></div>
      <p class="muted">${escapeHtml(note)}</p>
      <p class="muted">Tastenkürzel: <b>?</b> zeigt alle, auch für Vim-Leute.</p>
      <p class="muted">Schrift: Pixelify Sans (SIL Open Font License)<br>Videos und Audio: FFmpeg (LGPL 2.1, ffmpeg.org)</p>`;
    body.querySelector('.update')?.appendChild(button('Auffrischen', false, installUpdate));
    const r = body.querySelector('.cli');
    r.appendChild(a.cli_installed ? button('Installiert ✓', true, () => {}) : button('Installieren', false, async () => {
      try { draw(await api.invoke('install_cli'), 'Fertig! Probier mal „wolfgang“ in einem neuen Terminal.'); }
      catch (e) { draw(a, String(e)); }
    }));
    restyle();
  };
  draw(await api.invoke('about'));
}

init().catch(e => { uiErrors.push(`init: ${e}`); if (T) api.invoke('frontend_ready', { errors: uiErrors, scale: 0 }); });
