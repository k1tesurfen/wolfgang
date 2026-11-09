// Stand-in for the Rust side when ui/index.html is opened in a normal browser
// (not inside the app): a short scripted conversation, so the UI can be looked
// at and tested. Inside the app, window.__TAURI__ exists and this does nothing.
//   ?demo=mode|size|confirm|done   start with a dropped pile at that step
//   ?demo=work                       watch Wolfgang work
//   ?panel=settings|presets|about    open a panel
'use strict';

if (!window.__TAURI__) {
  window.WolfgangMock = (() => {
    const listeners = {};
    const emit = (ev, payload) => (listeners[ev] || []).forEach(f => f(payload));
    const ask = (node, mood, lines, answers, extra = {}) => ({ type: 'ask', node, mood, lines, answers: answers.map(a => (typeof a === 'string' ? { label: a, current: false } : a)), input: null, can_go_back: true, output: null, ...extra });

    const views = {
      mode: ask('mode', 'thinking', ['Moin! Zeig mal her, was du da hast.', 'Ich seh 42 Fotos und 6 Schriften.', 'Mit einer Datei kann ich nix anfangen, die lass ich einfach liegen.', 'Wofür brauchst du die Bilder?'],
        ['Fürs Web vorbereiten – klein und schnell', 'Hochwertig behalten – nur ein anderes Format'], { can_go_back: false }),
      size: ask('web_size', 'thinking', ['Wie groß sollen sie werden? Ich mein die längste Seite.'],
        ['Klein (800 px) – für Vorschaubilder', { label: 'Normal (1600 px) – passt fast immer', current: true }, 'Groß (2400 px) – für große Titelbilder'],
        { input: { kind: 'number', placeholder: 'z. B. 1920', unit: 'px', min: 100, max: 8000 } }),
      confirm: ask('confirm', 'happy', ['Also: 42 Fotos fürs Web als WebP, 1600 px, Qualität 80, Name „Kunde Müller“ – Dateinamen wie „kunde-mueller-buero-0001.webp“, dazu 6 Schriften als WOFF2. Landet alles in „Kunde-wolfgang“.', 'Passt das so?'],
        ["Los geht's!", 'Nochmal ändern', 'Lass mal, doch nicht']),
      start: { type: 'start', mood: 'happy', lines: [], output: null },
      done: ask('done', 'proud', ['So, erledigt. 41,7 MB rein, 1,2 MB raus – 98 Prozent gespart.', 'Liegt alles in „Kunde-wolfgang“.'],
        ['Als Vorlage merken', 'Danke, Wolfgang!'], { can_go_back: false, output: '/Users/du/Kunde-wolfgang' }),
      end: { type: 'end', mood: 'happy', lines: ['Bis bald!'], output: '/Users/du/Kunde-wolfgang' },
      cancelled: { type: 'cancelled', mood: 'neutral', lines: ['Kein Ding. Wenn du’s dir anders überlegst, ich bin hier.'], output: null },
    };
    const next = { mode: ['size', 'confirm'], size: ['confirm', 'confirm', 'confirm'], confirm: ['start', 'mode', 'cancelled'], done: ['end', 'end'] };
    let node = 'mode', history = [];
    let settings = { web_format: 'webp', max_side: 1600, quality: 80, hq_format: 'jpg', separator: 'dash', sound: true };
    let presets = [{ name: 'Blog-Bilder', description: 'fürs Web als WebP, 1600 px, Qualität 80' }];

    const work = () => {
      const total = 42;
      let done = 0;
      const t = setInterval(() => {
        done++;
        emit('progress', { done, total, current: `Fotos/IMG_${4700 + done}.heic`, ok: true });
        if (done === total) { clearInterval(t); node = 'done'; emit('finished', views.done); }
      }, 90);
    };

    const commands = {
      settings: () => settings,
      set_settings: ({ defaults }) => (settings = defaults),
      presets: () => presets,
      remove_preset: ({ name }) => (presets = presets.filter(p => p.name !== name)),
      about: () => ({ version: '2.0.0 (Vorschau)', cli_installed: false }),
      install_cli: () => { throw 'In der Vorschau geht das nicht.'; },
      idle_line: () => 'Leg mir einfach was auf den Tisch. Bilder, Schriften … ich guck’s mir an.',
      working_line: () => ['Moment, bin dran …', 'Jubdidubdidub. Was? Ja gleich fertig.', 'Pixel sortieren, Bytes rausschütteln …'][Math.floor(Math.random() * 3)],
      drop_files: ({ paths }) => {
        const want = (paths[0] || '').split('/').pop();
        node = views[want] ? want : 'mode';
        history = [];
        if (want === 'work') { node = 'start'; return { ...views.start, lines: ['Los geht’s.'] }; }
        return views[node];
      },
      choose: ({ index }) => {
        history.push(node);
        node = next[node]?.[index] || 'mode';
        if (node === 'start') return views.start;
        return views[node];
      },
      type_answer: () => { history.push(node); node = 'confirm'; return views.confirm; },
      back: () => { node = history.pop() || 'mode'; return views[node]; },
      start: () => { work(); },
      cancel: () => {},
      reveal: () => {},
      take_pending: () => [],
    };

    return {
      invoke: (cmd, args = {}) => new Promise((ok, err) => setTimeout(() => { try { ok(commands[cmd](args)); } catch (e) { err(e); } }, 30)),
      listen: (ev, f) => { (listeners[ev] ||= []).push(f); },
      onDrop: f => {
        // Normal browser drag & drop (no real paths, but enough to try the flow).
        addEventListener('dragenter', e => { e.preventDefault(); f({ type: 'enter' }); });
        addEventListener('dragover', e => { e.preventDefault(); f({ type: 'over' }); });
        addEventListener('dragleave', e => { if (!e.relatedTarget) f({ type: 'leave' }); });
        addEventListener('drop', e => { e.preventDefault(); f({ type: 'drop', paths: [...e.dataTransfer.files].map(x => x.name) }); });
      },
    };
  })();
}
