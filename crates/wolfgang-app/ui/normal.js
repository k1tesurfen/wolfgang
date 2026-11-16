// „Normaler Modus“: the same engine as the office, as a plain tool. One big drop
// zone; after a drop, one card per pile whose first decision unfolds the rest;
// a toolbar at the bottom with the settings and „Konvertieren“. No character,
// no sounds. app.js hands over to Normal.start() when this app is in normal mode.
'use strict';

const Normal = (() => {
  let api, tauri, character;
  let view = 'empty';    // empty | cards | settings | check
  let back = 'empty';    // where „Fertig“ in the settings goes
  let desk = null;       // what the drop looks like (DeskView from Rust)
  let cards = {};        // the choices per pile
  let again = false;     // „trotzdem nochmal machen“ for files converted before
  let template = '';     // the template picked in the drop-down
  let running = false;
  let result = null;     // after a run
  let progress = null;   // { done, total, current, fraction } while running
  let estimate = null;   // „etwa 9 Minuten“
  let error = '';        // shown in the toolbar until the next action
  let check = null;      // { url, view } for the website check
  let update = null;     // { version } when a newer version is on GitHub
  let updating = null;   // { done, total } while it downloads
  let addon = null, addonDone = 0, addonNote = '';
  let standardsOpen = false;

  const $ = id => document.getElementById(id);

  /** Tiny element builder: el('div.a.b', { onclick }, child, 'text', …) */
  function el(spec, props = {}, ...kids) {
    const [tag, ...cls] = spec.split('.');
    const e = document.createElement(tag || 'div');
    if (cls.length) e.className = cls.join(' ');
    for (const [k, v] of Object.entries(props || {})) {
      if (v === undefined || v === null || v === false) continue;
      if (k.startsWith('on')) e[k] = v;
      else if (k === 'text') e.textContent = v;
      else if (k === 'html') e.innerHTML = v;
      else if (k in e && k !== 'list') e[k] = v;
      else e.setAttribute(k, v === true ? '' : v);
    }
    for (const k of kids.flat()) if (k !== null && k !== undefined && k !== false) e.append(k);
    return e;
  }

  const plural = (n, one, many) => (n === 1 ? `1 ${one}` : `${n} ${many}`);
  const icons = {
    drop: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M12 15V4m0 0-4 4m4-4 4 4"/><path d="M4 14v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4"/></svg>',
    gear: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>',
  };

  /* ── Start ───────────────────────────────────────────────────────────── */

  function start(opts) {
    ({ api, character } = opts);
    tauri = opts.tauri;
    document.body.classList.add('normal');
    document.body.append(el('div', { id: 'n-app' },
      el('div', { id: 'n-drag', 'data-tauri-drag-region': true }),
      el('main', { id: 'n-main' }),
      el('footer', { id: 'n-bar' })));

    api.onDrop(onDrop);
    const takePending = async () => { const p = await api.invoke('take_pending'); if (p.length) dropped(p); };
    api.listen('files-opened', takePending);
    api.listen('progress', p => { if (running) { progress = p; drawBar(); } });
    api.listen('desk-finished', finished);
    api.listen('failed', msg => { running = false; progress = null; fail(msg); draw(); });
    api.listen('update-progress', p => { updating = p; if (view === 'settings') draw(); });
    api.listen('transcription-progress', p => { addonDone = p.done; if (addon) addon.installing = true; if (view === 'settings') draw(); });
    api.listen('transcription-done', r => {
      addonNote = r.Err ? String(r.Err) : '';
      addon = r.Ok || { ...addon, installing: false };
      if (desk) desk.transcription = addon.installed;
      draw();
    });
    addEventListener('keydown', onKey);
    addEventListener('paste', e => {
      if (e.target.tagName === 'INPUT' || view !== 'empty') return;
      const text = e.clipboardData?.getData('text/plain')?.trim();
      if (text) { e.preventDefault(); checkUrl(text); }
    });

    draw();
    if (tauri) {
      takePending();
      setTimeout(() => api.invoke('frontend_ready', { errors: opts.errors, scale: 1 }), 1200);
    }
    setTimeout(async () => { update = await api.invoke('check_update').catch(() => null); drawBar(); }, 2500);

    // Looking at it in a browser (mock.js): `?normal&demo` drops a pile, `?normal&panel=settings`.
    const q = new URLSearchParams(location.search);
    // `&pick=images:web,videos:mp4` makes those decisions; `&run` converts right away.
    if (!tauri && q.has('demo')) dropped(['/demo/' + (q.get('demo') || 'mixed')]).then(() => {
      for (const [pile, choice] of (q.get('pick') || '').split(',').filter(Boolean).map(x => x.split(':'))) {
        document.querySelector(`#card-${pile} .seg.big button:nth-child(${['web', 'mp4', 'mp3', 'pictures'].includes(choice) ? 1 : ['hq', 'webm', 'aac', 'smaller'].includes(choice) ? 2 : 3})`)?.click();
      }
      if (q.has('run')) setTimeout(run, 300);
    });
    if (!tauri && q.get('panel') === 'settings') openSettings();
    if (!tauri && q.has('check')) setTimeout(() => checkUrl('example.com'), 300);
  }

  /* ── Drawing ─────────────────────────────────────────────────────────── */

  function draw() {
    const main = $('n-main');
    main.innerHTML = '';
    if (view === 'settings') main.append(...settingsView());
    else if (view === 'check') main.append(checkView());
    else if (view === 'cards') main.append(...cardsView());
    else main.append(dropZone());
    drawBar();
  }

  function dropZone() {
    return el('section', { id: 'n-drop', onclick: pick, title: 'Dateien auswählen (⌘O)' },
      el('div', { html: icons.drop }),
      el('div.big', { text: 'Dateien oder Ordner hierher ziehen' }),
      el('div.small', { text: 'oder klicken zum Auswählen · eine Webadresse mit ⌘V einfügen, um die Seite zu prüfen' }));
  }

  /* ── The toolbar ─────────────────────────────────────────────────────── */

  function drawBar() {
    const bar = $('n-bar');
    bar.innerHTML = '';
    const status = el('div.status');
    const right = [];
    if (view === 'settings') {
      bar.append(status, el('button.pill.primary', { text: 'Fertig', onclick: closeSettings }));
      return;
    }
    const gear = el('button.pill.icon.quiet', { title: 'Einstellungen (⌘,)', html: icons.gear, onclick: openSettings });
    if (update) gear.append(el('span', { style: 'position:absolute;margin:-18px 0 0 18px;width:7px;height:7px;border-radius:50%;background:var(--accent)' }));
    bar.append(gear, status);

    if (running) {
      const p = progress || { done: 0, total: 0, fraction: 0, current: '' };
      const fill = el('div', { style: `width:${Math.round((p.fraction ?? 0) * 100)}%` });
      status.append(el('div.bar', {}, fill), el('span.text', { text: p.total ? `${p.done} von ${p.total} · ${p.current}` : 'Geht los …' }));
      right.push(el('button.pill', { text: 'Abbrechen', onclick: () => { api.invoke('cancel'); status.lastChild.textContent = 'Wird abgebrochen …'; } }));
    } else if (view === 'check') {
      if (check?.view) {
        right.push(el('button.pill', { text: 'Kopieren', onclick: copyReport }));
      } else {
        status.append(el('span.text', { text: 'Wird geprüft …' }));
      }
      right.push(el('button.pill.primary', { text: 'Schließen', onclick: reset }));
    } else if (view === 'cards' && result) {
      if (result.output) right.push(el('button.pill', { text: 'Im Finder zeigen', onclick: () => api.invoke('reveal', { path: result.output }) }));
      right.push(el('button.pill.primary', { text: 'Neue Dateien', onclick: reset }));
    } else if (view === 'cards') {
      const ready = readiness();
      status.append(el('span.text', { text: error || ready.why || (estimate ? `Dauert ${estimate}.` : '') }));
      if (error) status.lastChild.style.color = 'var(--error)';
      right.push(el('button.pill.quiet', { text: 'Leeren', title: 'Alles wieder vom Tisch (Esc)', onclick: reset }));
      right.push(el('button.pill', { text: 'Als Vorlage speichern …', disabled: !ready.decided, onclick: savePreset }));
      right.push(el('button.pill.primary', { text: 'Konvertieren', title: '⌘↩', disabled: !ready.ok, onclick: run }));
    } else if (error) {
      status.append(el('span.text', { text: error, style: 'color:var(--error)' }));
    }
    bar.append(...right);
  }

  const fail = msg => { error = String(msg); drawBar(); };

  /* ── Dropping ────────────────────────────────────────────────────────── */

  function onDrop(e) {
    const over = $('n-dragover');
    if (e.type === 'enter' || e.type === 'over') {
      if (!over && !running) $('n-app').append(el('div', { id: 'n-dragover', text: view === 'cards' && !result ? 'Dazulegen' : 'Loslassen zum Konvertieren' }));
    } else if (e.type === 'leave') {
      over?.remove();
    } else if (e.type === 'drop') {
      over?.remove();
      if (e.paths?.length) dropped(e.paths);
    }
  }

  async function pick() {
    if (running || pick.busy) return;
    pick.busy = true;
    try {
      const paths = await api.invoke('pick_files');
      if (paths.length) dropped(paths);
    } catch (e) {
      fail(e);
    } finally {
      pick.busy = false;
    }
  }

  /** New files: next to what's there, or a fresh start after a run (or from elsewhere). */
  async function dropped(paths) {
    if (running) return fail('Bitte warten, bis das Konvertieren fertig ist.');
    const add = view === 'cards' && !result;
    if (!add) { cards = {}; customOpen.clear(); again = false; template = ''; result = null; }
    error = '';
    try {
      desk = await api.invoke('desk_drop', { paths, add });
    } catch (e) {
      return fail(e);
    }
    view = 'cards';
    for (const p of desk.piles) cards[p.key] ||= {};
    draw();
    refresh();
  }

  function reset() {
    if (running) return;
    api.invoke('desk_clear').catch(() => {});
    desk = null; cards = {}; customOpen.clear(); result = null; check = null; error = ''; estimate = null; again = false; template = '';
    view = 'empty';
    draw();
  }

  /* ── The cards ───────────────────────────────────────────────────────── */

  const PILES = {
    images: ['Foto', 'Fotos'],
    videos: ['Video', 'Videos'],
    audio: ['Audiodatei', 'Audiodateien'],
    pdfs: ['PDF', 'PDFs'],
    tables: ['Tabelle', 'Tabellen'],
    fonts: ['Schrift', 'Schriften'],
  };

  function cardsView() {
    const out = [];
    if (result) out.push(resultCard());
    out.push(infoLines());
    for (const p of desk.piles) out.push(el('section.panel', { id: `card-${p.key}` }, ...card(p)));
    if (running || result) out.slice(result ? 2 : 1).forEach(c => c.classList.add('locked'));
    return out;
  }

  /** Above the cards: the template, files converted before, files nobody can read. */
  function infoLines() {
    const box = el('div.info');
    const choices = [['', 'Keine Vorlage']];
    if (desk.last_run) choices.push(['@last', 'Wie letztes Mal']);
    for (const p of desk.presets) choices.push([p.name, `Vorlage „${p.name}“`]);
    if (choices.length > 1 && !result) {
      const sel = el('select', { onchange: e => applyTemplate(e.target.value), disabled: running }, ...choices.map(([v, t]) => el('option', { value: v, text: t, selected: v === template })));
      box.append(el('div.field', {}, el('div.label', { text: 'Vorlage' }), el('div.control', {}, sel)));
    }
    if (desk.known > 0 && !result) {
      const box2 = el('input', { type: 'checkbox', checked: again, disabled: running, onchange: e => { again = e.target.checked; refresh(); } });
      box.append(el('label.switch', {}, box2, el('span', { text: `${plural(desk.known, 'Datei ist', 'Dateien sind')} schon konvertiert – trotzdem nochmal machen` })));
    }
    if (desk.unknown.length) {
      box.append(el('details', {}, el('summary', { text: `${plural(desk.unknown.length, 'Datei kann', 'Dateien können')} nicht verarbeitet werden – Details` }),
        el('ul', {}, ...desk.unknown.map(u => el('li', { text: u })))));
    }
    if (desk.duplicates) box.append(el('div', { text: `${plural(desk.duplicates, 'Datei ist doppelt', 'Dateien sind doppelt')} und wird nur einmal konvertiert.` }));
    if (desk.media_tools_missing) box.append(el('div', { text: 'Für Videos und Audio fehlt ffmpeg in dieser Installation – die bleiben liegen.' }));
    return box;
  }

  function title(p) {
    const [one, many] = PILES[p.key];
    return el('h2', {}, el('span', { text: p.total === 1 ? `1 ${one}` : `${p.total} ${many}` }),
      p.known ? el('span.count', { text: `davon ${p.known} schon konvertiert` }) : null);
  }

  /** A segmented choice. `options`: [[label, value, disabled?]] */
  function seg(options, value, pick, big = false) {
    return el(`div.seg${big ? '.big' : ''}`, {}, ...options.map(([label, v, off]) =>
      el(`button${v === value ? '.on' : ''}`, { text: label, disabled: !!off || running, onclick: () => pick(v) })));
  }

  const field = (label, ...control) => el('div.field', {}, el('div.label', { text: label }), el('div.control', {}, ...control));
  const toggle = (label, checked, set) =>
    el('label.switch', {}, el('input', { type: 'checkbox', checked, disabled: running, onchange: e => { set(e.target.checked); refresh(); } }), el('span', { text: label }));

  /** Pills plus „Eigene“: picking „Eigene“ shows a number field. `key` names the field
   *  (so it stays open across redraws). */
  const customOpen = new Set();
  function sizeChoice(key, options, value, set, { min, max, unit }) {
    const known = options.some(o => o[1] === value);
    const custom = !known || customOpen.has(key);
    const input = el('input', { type: 'number', id: `n-${key}`, min, max, value: known ? '' : value ?? '', placeholder: `${min}–${max}`,
      oninput: e => {
        const n = Number(e.target.value);
        const ok = Number.isInteger(n) && n >= min && n <= max;
        e.target.classList.toggle('bad', !ok && e.target.value !== '');
        if (ok) { set(n, true); refresh(); }
      } });
    const choice = seg([...options, ['Eigene …', '@custom']], custom ? '@custom' : value, v => {
      if (v === '@custom') { customOpen.add(key); redraw(); setTimeout(() => document.getElementById(`n-${key}`)?.focus(), 0); return; }
      customOpen.delete(key);
      set(v);
      redraw();
    });
    return [choice, custom ? el('span.control', {}, input, el('span.hint', { text: unit })) : null];
  }

  function card(p) {
    const c = cards[p.key];
    const d = desk.defaults, last = desk.last_run || {};
    switch (p.key) {
      case 'images': {
        const decide = mode => {
          c.mode = mode;
          if (mode === 'web' && !c.web) c.web = webFrom(last.images?.mode === 'web' ? last.images : null);
          if (mode === 'hq' && !c.hq) c.hq = last.images?.mode === 'high_quality' ? last.images.format : d.hq_format;
          redraw();
        };
        const out = [title(p), seg([['Fürs Web vorbereiten', 'web'], ['Hochwertig behalten', 'hq']], c.mode, decide, true)];
        if (c.mode === 'hq') {
          out.push(el('p.hint', { text: 'Originalgröße und volle Qualität, nur ein anderes Format. Die Dateinamen bleiben.' }),
            el('div.sub', {}, field('Format', seg([['JPG', 'jpg'], ['WebP', 'webp'], ['AVIF', 'avif'], ['PNG', 'png']], c.hq, v => { c.hq = v; redraw(); }))));
        }
        if (c.mode === 'web') out.push(webFields(c));
        return out;
      }
      case 'videos': {
        const decide = choice => {
          c.choice = choice;
          if (choice !== 'transcript') c.video = { ...(c.video || (last.videos ? { ...last.videos } : { size: d.video_size, quality: d.video_quality, sound: true, poster: true, background: 'white' })), format: choice };
          else c.timestamps ??= last.video_transcript?.timestamps ?? true;
          redraw();
        };
        const out = [title(p), seg([['MP4 – läuft überall', 'mp4'], ['WebM – kleiner, dauert länger', 'webm'], transcriptOption()], c.choice, decide, true)];
        if (!desk.transcription) out.push(el('p.hint', { text: 'Aufschreiben braucht ein Zusatzmodul (575 MB). Es lässt sich in den Einstellungen installieren.' }));
        if (desk.animated_gifs) out.push(el('p.hint', { text: `${plural(desk.animated_gifs, 'animiertes GIF wird', 'animierte GIFs werden')} zu ${desk.animated_gifs === 1 ? 'einem Video' : 'Videos'} – viel kleiner, sieht genauso aus.` }));
        if (c.choice === 'transcript') out.push(transcriptFields(c, 'Videos'));
        else if (c.choice) {
          const v = c.video;
          out.push(el('div.sub', {},
            field('Größe', ...sizeChoice('video-size', [['720p', 720], ['1080p', 1080], ['Original', null]], v.size, (n) => { v.size = n; }, { min: 144, max: 4320, unit: 'p (kürzere Seite)' })),
            field('Qualität', seg([['Kleiner', 'smaller'], ['Ausgewogen', 'balanced'], ['Schöner', 'nicer']], v.quality, q => { v.quality = q; redraw(); })),
            desk.videos_with_sound ? field('Ton', toggle('Mit Ton (aus für Hintergrund- und Loop-Videos)', v.sound, x => (v.sound = x))) : null,
            desk.transparent_gifs ? field('Hintergrund', seg([['Weiß', 'white'], ['Schwarz', 'black']], v.background, b => { v.background = b; redraw(); })) : null,
            field('Vorschaubild', toggle('Als WebP dazulegen', v.poster, x => (v.poster = x)))));
        }
        return out;
      }
      case 'audio': {
        const decide = choice => {
          c.choice = choice;
          if (choice !== 'transcript') c.content ??= last.audio?.content ?? 'music';
          else c.timestamps ??= last.audio_transcript?.timestamps ?? true;
          redraw();
        };
        const out = [title(p), seg([['MP3 – läuft überall', 'mp3'], ['AAC (.m4a) – klingt besser', 'aac'], transcriptOption()], c.choice, decide, true)];
        if (c.choice === 'transcript') out.push(transcriptFields(c, 'Audiodateien'));
        else if (c.choice) {
          out.push(el('div.sub', {}, field('Inhalt', seg([['Sprache', 'speech'], ['Musik', 'music'], ['Musik, beste Qualität', 'best']], c.content, x => { c.content = x; redraw(); })),
            el('p.hint', { text: c.content === 'speech' ? 'Mono, klein, gleichmäßig laut – für Podcasts und Interviews.' : 'Stereo; die Bitrate passt sich an.' })));
        }
        return out;
      }
      case 'pdfs':
        return [title(p), seg([['Bilder rausholen', 'pictures'], ['Kleiner machen – für den Download', 'smaller']], c.mode, m => { c.mode = m; redraw(); }, true),
          c.mode ? el('p.hint', { text: c.mode === 'pictures' ? 'Die Bilder kommen so heraus, wie sie drin sind, je PDF in einem eigenen Ordner.' : 'Zu große Bilder werden verkleinert, Überbleibsel aus dem Layoutprogramm fliegen raus.' }) : null];
      case 'tables': {
        const excel = p.total - desk.csv_tables;
        const what = [excel ? `${plural(excel, 'Excel-Datei wird', 'Excel-Dateien werden')} zu CSV (eine Datei pro Blatt)` : '', desk.csv_tables ? `${plural(desk.csv_tables, 'CSV-Datei wird', 'CSV-Dateien werden')} zu Excel` : ''].filter(Boolean).join(', ');
        return [title(p), el('p', { text: `${what}.` })];
      }
      case 'fonts':
        return [title(p), el('p', { text: 'Werden zu WOFF2 – eins zu eins, ohne Glyphen wegzuschneiden.' })];
    }
    return [title(p)];
  }

  const transcriptOption = () => ['Nur aufschreiben, was gesagt wird', 'transcript', !desk.transcription];

  function transcriptFields(c, what) {
    return el('div.sub', {},
      el('p.hint', { text: `Die ${what} werden nicht umgewandelt: jede Aufnahme wird als .txt aufgeschrieben (Deutsch, auf diesem Mac).` }),
      field('Zeitangaben', toggle('[00:12:30] vor jedem Absatz', c.timestamps, x => (c.timestamps = x))));
  }

  /** Web settings for a fresh decision: like last time (but not its keywords) or the defaults. */
  function webFrom(last) {
    const d = desk.defaults;
    const naming = { prefix: '', use_keyword_files: true, folder_keywords: {}, keywords_all: '', separator: d.separator, append_original: false };
    if (!last) return { format: d.web_format, max_side: d.max_side, quality: d.quality, naming };
    return { format: last.format, max_side: last.max_side, quality: last.quality, naming: { ...naming, prefix: last.naming?.prefix || '', separator: last.naming?.separator || d.separator, append_original: !!last.naming?.append_original } };
  }

  function webFields(c) {
    const w = c.web, n = w.naming;
    const text = (value, placeholder, set) => el('input', { type: 'text', value, placeholder, spellcheck: false, disabled: running, oninput: e => { set(e.target.value); preview(); } });
    const rows = [
      field('Format', seg([['WebP', 'webp'], ['AVIF – kleiner, dauert länger', 'avif']], w.format, f => { w.format = f; redraw(); })),
      field('Größe', ...sizeChoice('web-size', [['Klein (800 px)', 800], ['Normal (1600 px)', 1600], ['Groß (2400 px)', 2400]], w.max_side, s => { w.max_side = s; }, { min: 100, max: 8000, unit: 'px (längste Seite)' })),
      field('Qualität', ...sizeChoice('web-quality', [['Kleiner (65)', 65], ['Ausgewogen (80)', 80], ['Schöner (90)', 90]], w.quality, q => { w.quality = q; }, { min: 1, max: 100, unit: 'von 100' })),
      field('Präfix', text(n.prefix, 'z. B. Kunde oder Projekt', v => (n.prefix = v))),
    ];
    // Keywords: .txt files where there are some, otherwise one field (or one per folder).
    if (desk.keyword_folders.length) {
      rows.push(field('Aus .txt', toggle(`Stichwörter aus ${plural(desk.keyword_folders.length, 'Textdatei', 'Textdateien')} verwenden`, n.use_keyword_files, x => { n.use_keyword_files = x; redraw(); })));
      if (n.use_keyword_files) rows.push(el('p.hint', { text: desk.keyword_folders.map(k => `${folderName(k.dir)}: ${k.words}`).join(' · ') }));
    }
    const open = desk.folders_without_keywords.concat(n.use_keyword_files ? [] : desk.keyword_folders.map(k => k.dir));
    if (open.length) {
      const perFolder = c.perFolder && open.length > 1;
      if (!perFolder) rows.push(field(desk.keyword_folders.length && n.use_keyword_files ? 'Stichwörter (Rest)' : 'Stichwörter', text(n.keywords_all, 'Worum geht’s auf den Bildern?', v => (n.keywords_all = v))));
      if (open.length > 1) rows.push(field('', toggle('Je Ordner einzeln', !!c.perFolder, x => { c.perFolder = x; redraw(); })));
      if (perFolder) for (const dir of open) rows.push(field(folderName(dir), text(n.folder_keywords[dir] || '', n.keywords_all || 'Stichwörter', v => (n.folder_keywords[dir] = v))));
    }
    rows.push(
      field('Wörter trennen', seg([['bild-name', 'dash'], ['bild_name', 'underscore'], ['BildName', 'caps']], n.separator, s => { n.separator = s; redraw(); })),
      field('Originalname', toggle('hinten anhängen', n.append_original, x => (n.append_original = x))),
      field('Beispiel', el('span.preview', { id: 'n-preview', text: '…' })));
    return el('div.sub', {}, ...rows);
  }

  const folderName = dir => (dir ? dir : desk.root_name);

  /** Redraws the cards (keeping the scroll position) after a choice changed. */
  function redraw() {
    const main = $('n-main'), top = main.scrollTop;
    draw();
    main.scrollTop = top;
    refresh();
  }

  /** The things Rust works out: the example name, how long it takes. */
  function refresh() {
    drawBar();
    preview();
    clearTimeout(refresh.t);
    refresh.t = setTimeout(async () => {
      if (!desk || view !== 'cards' || result) return;
      const r = readiness();
      estimate = r.ok ? await api.invoke('desk_estimate', { settings: settings(), again }).catch(() => null) : null;
      drawBar();
    }, 150);
  }

  function preview() {
    clearTimeout(preview.t);
    preview.t = setTimeout(async () => {
      const box = $('n-preview'), w = cards.images?.web;
      if (!box || !w) return;
      box.textContent = await api.invoke('desk_preview', { web: w }).catch(() => '…');
    }, 120);
  }

  /** The settings the cards stand for (`RunSettings` in Rust). */
  function settings() {
    const s = {};
    const { images: i, videos: v, audio: a, pdfs: p } = cards;
    if (i?.mode === 'web') s.images = { mode: 'web', ...i.web };
    if (i?.mode === 'hq') s.images = { mode: 'high_quality', format: i.hq };
    if (v?.choice === 'transcript') s.video_transcript = { timestamps: v.timestamps };
    else if (v?.choice) s.videos = v.video;
    if (a?.choice === 'transcript') s.audio_transcript = { timestamps: a.timestamps };
    else if (a?.choice) s.audio = { format: a.choice, content: a.content };
    if (p?.mode) s.pdfs = { mode: p.mode };
    return s;
  }

  /** Can it start? `decided`: every card has its first decision; `ok`: and there's something to do. */
  function readiness() {
    if (!desk) return { ok: false, decided: false };
    const asks = desk.piles.filter(p => ['images', 'videos', 'audio', 'pdfs'].includes(p.key));
    const open = asks.filter(p => { const c = cards[p.key]; return !(c.mode || c.choice); });
    const decided = open.length === 0;
    if (!decided) return { ok: false, decided, why: open.length === asks.length && asks.length > 1 ? 'Wähle für jeden Stapel, was passieren soll.' : `Noch offen: ${open.map(p => PILES[p.key][1]).join(', ')}.` };
    if (!desk.fresh && !again) return { ok: false, decided, why: 'Alles schon konvertiert.' };
    if (!desk.piles.length) return { ok: false, decided, why: 'Damit lässt sich nichts anfangen.' };
    return { ok: true, decided };
  }

  function applyTemplate(name) {
    template = name;
    const s = name === '@last' ? desk.last_run : desk.presets.find(p => p.name === name)?.settings;
    if (s) {
      const c = cards;
      if (c.images && s.images) {
        c.images.mode = s.images.mode === 'web' ? 'web' : 'hq';
        if (s.images.mode === 'web') c.images.web = webFrom(s.images);
        else c.images.hq = s.images.format;
      }
      if (c.videos && (s.videos || s.video_transcript)) {
        if (s.video_transcript && desk.transcription) Object.assign(c.videos, { choice: 'transcript', timestamps: s.video_transcript.timestamps });
        else if (s.videos) Object.assign(c.videos, { choice: s.videos.format, video: { ...s.videos } });
      }
      if (c.audio && (s.audio || s.audio_transcript)) {
        if (s.audio_transcript && desk.transcription) Object.assign(c.audio, { choice: 'transcript', timestamps: s.audio_transcript.timestamps });
        else if (s.audio) Object.assign(c.audio, { choice: s.audio.format, content: s.audio.content });
      }
      if (c.pdfs && s.pdfs) c.pdfs.mode = s.pdfs.mode;
    }
    redraw();
  }

  /* ── Running ─────────────────────────────────────────────────────────── */

  async function run() {
    if (running || !readiness().ok) return;
    error = '';
    running = true;
    progress = null;
    redraw();
    try {
      await api.invoke('desk_start', { settings: settings(), again });
    } catch (e) {
      running = false;
      fail(e);
      redraw();
    }
  }

  function finished(r) {
    running = false;
    progress = null;
    result = r;
    draw();
    $('n-main').scrollTop = 0;
  }

  function resultCard() {
    const r = result;
    const head = r.cancelled ? `Abgebrochen – ${plural(r.converted, 'Datei', 'Dateien')} fertig.` : r.converted ? `Fertig: ${plural(r.converted, 'Datei', 'Dateien')} in ${r.duration}.` : 'Das hat nicht geklappt.';
    const card = el('section.panel.result', {}, el('h2', { text: r.failed.length && !r.converted ? 'Fehler' : 'Ergebnis' }), el('div.big', { text: head }));
    if (r.saved) card.append(el('p', { text: r.saved }));
    if (r.lines.length) card.append(el('ul', {}, ...r.lines.map(l => el('li', { text: l }))));
    if (r.failed.length) {
      card.append(el('p', { text: `${plural(r.failed.length, 'Datei hat', 'Dateien haben')} nicht geklappt:` }),
        el('ul.failed', {}, ...r.failed.slice(0, 20).map(([f, e]) => el('li', {}, el('b', { text: f }), el('span', { text: ` – ${e}` })))));
    }
    if (r.output) card.append(el('p.hint', { text: `Liegt in „${r.output_folder}“.` }));
    return card;
  }

  /* ── Save as template ────────────────────────────────────────────────── */

  function savePreset() {
    const input = el('input', { type: 'text', placeholder: 'z. B. Blog-Bilder', spellcheck: false });
    const note = el('p.hint');
    const close = () => $('n-dialog')?.remove();
    const save = async () => {
      if (!input.value.trim()) return input.focus();
      try {
        const presets = await api.invoke('save_preset', { name: input.value, settings: settings() });
        desk.presets = presets;
        template = input.value.trim();
        close();
        redraw();
      } catch (e) {
        note.textContent = String(e);
      }
    };
    input.onkeydown = e => { e.stopPropagation(); if (e.key === 'Enter') save(); if (e.key === 'Escape') close(); };
    $('n-app').append(el('div', { id: 'n-dialog', onclick: e => { if (e.target.id === 'n-dialog') close(); } },
      el('div.panel', {}, el('h2', { text: 'Als Vorlage speichern' }),
        el('p.hint', { text: 'Die Einstellungen dieser Karten werden unter dem Namen gemerkt (Stichwörter nicht). Eine Vorlage mit gleichem Namen wird ersetzt.' }),
        input, note,
        el('div.actions', {}, el('button.pill.quiet', { text: 'Abbrechen', onclick: close }), el('button.pill.primary', { text: 'Speichern', onclick: save })))));
    setTimeout(() => input.focus(), 0);
  }

  /* ── The website check (⌘V on the empty drop zone) ───────────────────── */

  async function pasteUrl() {
    if (view !== 'empty' || running) return;
    const text = (await api.invoke('clipboard_text').catch(() => null))?.trim();
    if (text) checkUrl(text);
  }

  async function checkUrl(url) {
    try {
      await api.invoke('check_looking', { url }); // only checks that it's an address
    } catch (e) {
      return fail(e);
    }
    error = '';
    const mine = (check = { url, view: null });
    view = 'check';
    draw();
    try {
      mine.view = await api.invoke('check_url', { url });
    } catch (e) {
      if (check === mine) { reset(); fail(e); }
      return;
    }
    if (check === mine && view === 'check') draw();
  }

  const VERDICTS = { fine: 'Alles in Ordnung', odd: 'Erreichbar, aber etwas ist komisch', dns: 'Problem beim DNS', server: 'Problem beim Server oder Hosting', wordpress: 'Problem bei WordPress' };

  function checkView() {
    const v = check.view;
    if (!v) return el('section.panel', {}, el('h2', { text: 'Webseite prüfen' }), el('div.verdict', {}, el('div.spinner'), el('span', { text: `${check.url} wird geprüft …` })));
    const level = v.verdict === 'fine' ? 'fine' : v.verdict === 'odd' ? 'odd' : 'problem';
    return el('section.panel', {}, el('h2', { text: 'Webseite prüfen' }),
      el('div.verdict', {}, el(`div.dot.${level}`), el('span', { text: VERDICTS[v.verdict] || v.verdict })),
      v.finding ? el('p', { text: v.finding }) : null,
      el('pre.report', { text: v.report }));
  }

  async function copyReport(e) {
    const b = e?.target;
    try {
      if (tauri) await api.invoke('copy_text', { text: check.view.report });
      else await navigator.clipboard.writeText(check.view.report);
      if (b) b.textContent = 'Kopiert ✓';
    } catch {
      if (b) b.textContent = 'Ging nicht';
    }
    if (b) setTimeout(() => (b.textContent = 'Kopieren'), 1600);
  }

  /* ── Settings ────────────────────────────────────────────────────────── */

  async function openSettings() {
    if (view === 'settings') return;
    back = view;
    view = 'settings';
    standardsOpen = false;
    const [d, presets, about, a] = await Promise.all([api.invoke('settings'), api.invoke('presets'), api.invoke('about'), api.invoke('transcription')]);
    Object.assign(openSettings, { d, presets, about });
    addon = a;
    draw();
    $('n-main').scrollTop = 0;
  }

  function closeSettings() {
    view = back === 'settings' ? 'empty' : back;
    $('n-dialog')?.remove();
    if (view === 'cards' && desk) {
      // Defaults or the add-on may have changed: look again, keep the choices.
      api.invoke('desk_drop', { paths: [], add: true }).then(v => { desk = v; redraw(); }).catch(() => redraw());
      return;
    }
    draw();
  }

  function settingsView() {
    let { d, presets, about } = openSettings;
    const save = async patch => {
      try { openSettings.d = await api.invoke('set_settings', { defaults: { ...d, ...patch } }); } catch (e) { fail(e); }
      redraw();
    };
    const pick = (options, key) => seg(options, d[key], v => save({ [key]: v }));

    const general = el('section.panel', {}, el('h2', { text: 'Allgemein' }),
      field('Ansicht', el('button.pill', { text: 'Spiel-Modus', disabled: running, onclick: toGame }), el('span.hint', { text: running ? 'Geht nach dem Konvertieren.' : `Das Büro mit ${character.name}: alles als Gespräch.` })));

    const head = el('button.fold-head', { 'aria-expanded': String(standardsOpen), onclick: () => { standardsOpen = !standardsOpen; redraw(); } },
      el('span.arrow', { text: '▸' }), el('span', { text: 'Meine Standards' }));
    const standards = el('section.panel', {}, head);
    if (standardsOpen) {
      standards.append(el('p.hint', { text: 'Damit sind die Karten vorbelegt, wenn es kein „letztes Mal“ gibt.' }),
        field('Fürs Web am liebsten', pick([['WebP', 'webp'], ['AVIF', 'avif']], 'web_format')),
        field('Größe fürs Web', pick([['800 px', 800], ['1600 px', 1600], ['2400 px', 2400]], 'max_side')),
        field('Qualität fürs Web', pick([['65', 65], ['80', 80], ['90', 90]], 'quality')),
        field('Hochwertig am liebsten', pick([['JPG', 'jpg'], ['WebP', 'webp'], ['AVIF', 'avif'], ['PNG', 'png']], 'hq_format')),
        field('Wörter trennen mit', pick([['bild-name', 'dash'], ['bild_name', 'underscore'], ['BildName', 'caps']], 'separator')),
        field('Videos am liebsten', pick([['MP4', 'mp4'], ['WebM', 'webm']], 'video_format')),
        field('Videogröße', pick([['720p', 720], ['1080p', 1080], ['Original', null]], 'video_size')),
        field('Videoqualität', pick([['Kleiner', 'smaller'], ['Ausgewogen', 'balanced'], ['Schöner', 'nicer']], 'video_quality')),
        field('Audio am liebsten', pick([['MP3', 'mp3'], ['AAC', 'aac']], 'audio_format')));
    }

    const templates = el('section.panel', {}, el('h2', { text: 'Vorlagen' }));
    if (!presets.length) templates.append(el('p.hint', { text: 'Noch keine. Nach einem Drop: „Als Vorlage speichern …“ unten in der Leiste.' }));
    for (const p of presets) {
      templates.append(el('div.preset-row', {}, el('div', {}, el('div', { text: p.name }), el('div.desc', { text: p.description })),
        el('button.pill', { text: 'Löschen', onclick: async () => { openSettings.presets = await api.invoke('remove_preset', { name: p.name }); redraw(); } })));
    }

    const mb = n => Math.round(n / 1e6);
    const transcription = el('section.panel', {}, el('h2', { text: 'Besprechungen aufschreiben' }),
      el('p.hint', { text: 'Ein Zusatzmodul, das auf diesem Mac aufschreibt, was in Video- und Audioaufnahmen gesagt wird. Nichts wird hochgeladen.' }));
    if (addon.installed) {
      transcription.append(field('Zusatzmodul', el('span', { text: 'Installiert ✓' }), el('button.pill', { text: 'Entfernen', onclick: async () => {
        try { addon = await api.invoke('remove_transcription'); addonNote = ''; } catch (e) { addonNote = String(e); }
        redraw();
      } })));
    } else if (addon.installing) {
      transcription.append(field('Zusatzmodul', el('div.bar', {}, el('div', { style: `width:${Math.round((addonDone / addon.size) * 100)}%` })),
        el('span.hint', { text: `${mb(addonDone)} von ${mb(addon.size)} MB` }), el('button.pill', { text: 'Abbrechen', onclick: () => api.invoke('cancel_transcription_install') })));
    } else {
      transcription.append(field('Zusatzmodul', el('button.pill', { text: `Installieren (${mb(addon.size)} MB)`, onclick: async () => {
        addonNote = ''; addonDone = 0;
        addon = await api.invoke('install_transcription');
        redraw();
      } })));
    }
    if (addonNote) transcription.append(el('p', { text: addonNote, style: 'color:var(--error)' }));

    const cli = el('section.panel', {}, el('h2', { text: 'Kommandozeile' }),
      field('wolfgang im Terminal', about.cli_installed ? el('span', { text: 'Installiert ✓' }) : el('button.pill', { text: 'Installieren', onclick: async () => {
        try { openSettings.about = await api.invoke('install_cli'); openSettings.cliNote = 'Fertig! „wolfgang“ in einem neuen Terminalfenster ausprobieren.'; }
        catch (e) { openSettings.cliNote = String(e); }
        redraw();
      } })),
      openSettings.cliNote ? el('p.hint', { text: openSettings.cliNote }) : null);

    const version = el('section.panel', {}, el('h2', { text: 'Version' }));
    if (updating) {
      const pct = updating.total ? Math.round((updating.done / updating.total) * 100) : 50;
      version.append(field(`${character.name} ${about.version}`, el('div.bar', {}, el('div', { style: `width:${pct}%` })), el('span.hint', { text: updating.total && updating.done >= updating.total ? 'Startet gleich neu …' : 'Wird geladen …' })));
    } else if (update) {
      version.append(field(`${character.name} ${about.version}`, el('button.pill.primary', { text: `Version ${update.version} installieren`, disabled: running, onclick: installUpdate })));
    } else {
      version.append(field(`${character.name} ${about.version}`, el('button.pill', { text: 'Nach neuer Version suchen', onclick: async e => {
        e.target.textContent = 'Suche …';
        update = await api.invoke('check_update').catch(() => null);
        if (!update) openSettings.versionNote = 'Das ist die neueste Version.';
        redraw();
      } })));
      if (openSettings.versionNote) version.append(el('p.hint', { text: openSettings.versionNote }));
    }
    return [general, standards, templates, transcription, cli, version];
  }

  async function toGame() {
    try {
      await api.invoke('set_normal_mode', { on: false });
      location.reload();
    } catch (e) {
      fail(e);
    }
  }

  function installUpdate() {
    updating = { done: 0, total: null };
    redraw();
    // On success the app restarts as the new version; we only come back here on errors.
    api.invoke('install_update').catch(e => { updating = null; update = null; openSettings.versionNote = String(e); redraw(); });
  }

  /* ── Keyboard ────────────────────────────────────────────────────────── */

  function onKey(e) {
    if ($('n-dialog')) return;
    const typing = e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT';
    if (e.metaKey && e.key === ',') { e.preventDefault(); view === 'settings' ? closeSettings() : openSettings(); return; }
    if (e.metaKey && e.key === 'o') { e.preventDefault(); pick(); return; }
    if (e.metaKey && e.key === 'Enter' && view === 'cards' && !result) { e.preventDefault(); run(); return; }
    if (typing) return;
    if (e.metaKey && e.key === 'v') { e.preventDefault(); pasteUrl(); return; }
    if (e.key === 'Escape') {
      if (view === 'settings') closeSettings();
      else if (view === 'check' || (view === 'cards' && !running)) reset();
      else return;
      e.preventDefault();
    }
  }

  return { start };
})();
