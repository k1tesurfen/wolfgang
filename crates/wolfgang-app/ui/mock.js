// Stand-in for the Rust side when ui/index.html is opened in a normal browser
// (not inside the app): a short scripted conversation, so the UI can be looked
// at and tested. Inside the app, window.__TAURI__ exists and this does nothing.
//   ?demo=mode|size|confirm|done   start with a dropped pile at that step
//   ?demo=work                       watch Wolfgang work
//   ?panel=settings|presets|about    open a panel
//   ?update                          pretend a newer Wolfgang is on GitHub
//   ?update=install                  … and say yes to it
//   ?addon                           the add-on for „Nur aufschreiben“ is installed
//   ?addon=installing                … or is being downloaded
//   ?character=ully|burgunde         look at the office as Ully or Burgunde
//   ?check                           paste example.com (⌘V) and get the printout
//   ?normal                          „Normaler Modus“ (with &demo for a dropped pile,
//                                    &demo=photos for photos only, &panel=settings)
'use strict';

if (!window.__TAURI__) {
  window.WolfgangMock = (() => {
    const listeners = {};
    const emit = (ev, payload) => (listeners[ev] || []).forEach(f => f(payload));
    const ask = (node, mood, lines, answers, extra = {}) => ({ type: 'ask', node, mood, lines, answers: answers.map(a => (typeof a === 'string' ? { label: a, current: false } : a)), input: null, can_go_back: true, output: null, ...extra });

    // A real report (wolfgang check example.com), for the printout.
    const exampleReport = "WOLFGANGS SEITEN-CHECK                                 11.11.2026, 16:52\nexample.com\n────────────────────────────────────────────────────────────────────────\n\nERGEBNIS\n! Die Seite bleibt auf http – es gibt keine Weiterleitung auf https.\n✓ DNS ist überall gleich: example.com 104.20.23.154, 172.66.147.243,\n  2606:4700:10::6814:179a, 2606:4700:10::ac42:93f3.\n✓ Alle 4 Adressen von example.com antworten gleich.\n✓ Die Seite lädt (200) in 31 ms: „Example Domain“.\n\nDNS  example.com\n  Zone example.com, Nameserver: elliott.ns.cloudflare.com,\n  hera.ns.cloudflare.com\n\n  Eigene Nameserver der Domain (die Wahrheit):\n    elliott.ns.cloudflare.com  (gültig 5 Min.)\n      104.20.23.154, 172.66.147.243, 2606:4700:10::6814:179a,\n      2606:4700:10::ac42:93f3\n    hera.ns.cloudflare.com  (gültig 5 Min.)\n      104.20.23.154, 172.66.147.243, 2606:4700:10::6814:179a,\n      2606:4700:10::ac42:93f3\n\n  Öffentliche Resolver (was die Welt sieht):\n    Cloudflare (1.1.1.1)  (noch 2 Min.)\n      104.20.23.154, 172.66.147.243, 2606:4700:10::6814:179a,\n      2606:4700:10::ac42:93f3\n    Google (8.8.8.8)  (noch 5 Min.)\n      104.20.23.154, 172.66.147.243, 2606:4700:10::6814:179a,\n      2606:4700:10::ac42:93f3\n    Quad9 (9.9.9.9)  (noch 2 Min.)\n      104.20.23.154, 172.66.147.243, 2606:4700:10::6814:179a,\n      2606:4700:10::ac42:93f3\n\n  Dieser Mac:\n    104.20.23.154, 172.66.147.243, 2606:4700:10::6814:179a,\n    2606:4700:10::ac42:93f3\n\nWEG DER ANFRAGE\n   1  200   31 ms\n      http://example.com/\n      an 2606:4700:10::6814:179a, Server: cloudflare, über Cloudflare\n\nJEDE ADRESSE EINZELN\n  example.com:\n    104.20.23.154                           200  39 ms  cloudflare\n    172.66.147.243                          200  39 ms  cloudflare\n    2606:4700:10::6814:179a                 200  41 ms  cloudflare\n    2606:4700:10::ac42:93f3                 200  37 ms  cloudflare\n\nDIE SEITE\n  http://example.com/\n  Status:          200\n  Titel:           Example Domain\n  WordPress:       nein\n  Suchmaschinen:   erlaubt\n\n────────────────────────────────────────────────────────────────────────\n";
    const characterKey = new URLSearchParams(location.search).get('character') || 'wolfgang';
    const character = { key: characterKey, name: characterKey[0].toUpperCase() + characterKey.slice(1) };
    const views = {
      mode: ask('mode', 'thinking', ['Moin! Zeig mal her, was du da hast.', 'Ich seh 42 Fotos und 6 Schriften.', 'Mit einer Datei kann ich nix anfangen, die lass ich einfach liegen.', 'Wofür brauchst du die Bilder?'],
        ['Fürs Web vorbereiten – klein und schnell', 'Hochwertig behalten – nur ein anderes Format'], { can_go_back: false }),
      size: ask('web_size', 'thinking', ['Wie groß sollen sie werden? Ich mein die längste Seite.'],
        ['Klein (800 px) – für Vorschaubilder', { label: 'Normal (1600 px) – passt fast immer', current: true }, 'Groß (2400 px) – für große Titelbilder'],
        { input: { kind: 'number', placeholder: 'z. B. 1920', unit: 'px', min: 100, max: 8000 } }),
      confirm: ask('confirm', 'happy', ['Also: 42 Fotos fürs Web als WebP, 1600 px, Qualität 80, Name „Kunde Müller“ – Dateinamen wie „kunde-mueller-buero-0001.webp“, dazu 6 Schriften als WOFF2. Landet alles in „Kunde-wolfgang“.', 'Passt das so?'],
        ["Los geht's!", 'Nochmal ändern', 'Lass mal, doch nicht']),
      start: { type: 'start', mood: 'happy', lines: [], output: null },
      done: ask('done', 'proud', ['So, erledigt. 41,7 MB rein, 1,2 MB raus – 98 Prozent gespart.', 'Liegt alles in „Kunde/wolfgang_output“.'],
        ['Als Vorlage merken', `Danke, ${character.name}!`], { can_go_back: false, output: '/Users/du/Kunde/wolfgang_output' }),
      end: { type: 'end', mood: 'happy', lines: ['Bis bald!'], output: '/Users/du/Kunde/wolfgang_output' },
      cancelled: { type: 'cancelled', mood: 'neutral', lines: ['Kein Ding. Wenn du’s dir anders überlegst, ich bin hier.'], output: null },
    };
    const next = { mode: ['size', 'confirm'], size: ['confirm', 'confirm', 'confirm'], confirm: ['start', 'mode', 'cancelled'], done: ['end', 'end'] };
    let node = 'mode', history = [];
    let settings = { web_format: 'webp', max_side: 1600, quality: 80, hq_format: 'jpg', separator: 'dash', video_format: 'mp4', video_size: 1080, video_quality: 'balanced', audio_format: 'mp3', sound: true };
    let presets = [{ name: 'Blog-Bilder', description: 'fürs Web als WebP, 1600 px, Qualität 80' }];
    const addonParam = new URLSearchParams(location.search).get('addon');
    let addon = { installed: addonParam === '', installing: false, size: 574_926_293 };
    let addonTimer = null;

    const work = () => {
      const total = 42;
      let done = 0;
      const t = setInterval(() => {
        done++;
        emit('progress', { done, total, current: `Fotos/IMG_${4700 + done}.heic`, fraction: done / total, ok: true });
        if (done === total) { clearInterval(t); node = 'done'; emit('finished', views.done); }
      }, 90);
    };

    // Normal mode: remembered per tab, so switching (which reloads) works in the preview.
    const q = new URLSearchParams(location.search);
    let normalMode = sessionStorage.getItem('normal') ? sessionStorage.getItem('normal') === '1' : q.has('normal');
    const pile = (key, total, known = 0) => ({ key, total, known });
    const deskViews = {
      mixed: {
        piles: [pile('images', 42, 12), pile('videos', 3), pile('audio', 1), pile('pdfs', 1), pile('fonts', 6)], csv_tables: 0, known: 12, fresh: 41,
        unknown: ['Kunde/Notizen.pages', 'Kunde/.DS_Store-alt', 'Kunde/Logo.ai'], duplicates: 1,
        keyword_folders: [{ dir: 'Team', words: 'team meeting koeln' }], folders_without_keywords: ['', 'Buero'], root_name: 'Kunde',
        videos_with_sound: 2, animated_gifs: 1, transparent_gifs: 1, media_tools_missing: false, transcription: addonParam === '',
        output_folder: 'Kunde/wolfgang_output',
      },
      photos: {
        piles: [pile('images', 18)], csv_tables: 0, known: 0, fresh: 18, unknown: [], duplicates: 0,
        keyword_folders: [], folders_without_keywords: [''], root_name: 'Urlaub', videos_with_sound: 0, animated_gifs: 0, transparent_gifs: 0,
        media_tools_missing: false, transcription: false, output_folder: 'Urlaub/wolfgang_output',
      },
    };
    let desk = null;
    const seo = s => s.toLowerCase().replace(/ä/g, 'ae').replace(/ö/g, 'oe').replace(/ü/g, 'ue').replace(/ß/g, 'ss').split(/[^a-z0-9]+/).filter(Boolean);
    const lastRun = { images: { mode: 'web', format: 'webp', max_side: 1600, quality: 80, naming: { prefix: 'Kunde Müller', use_keyword_files: true, folder_keywords: {}, keywords_all: '', separator: 'dash', append_original: false } } };

    const commands = {
      app_prefs: () => ({ normal_mode: normalMode, normal_size: null }),
      set_normal_mode: ({ on }) => { normalMode = on; sessionStorage.setItem('normal', on ? '1' : '0'); return { normal_mode: on }; },
      office_size: () => ({ current: 4, max: 6, default: 4 }),
      set_office_size: ({ scale }) => ({ current: scale, max: 6, default: 4 }),
      desk_drop: ({ paths, add }) => {
        if (add && desk && !paths.length) return desk;
        const kind = (paths[0] || '').split('/').pop();
        desk = { ...(deskViews[kind] || deskViews.mixed), defaults: settings, last_run: lastRun, presets: presets.map(p => ({ name: p.name, settings: lastRun })) };
        return desk;
      },
      desk_clear: () => { desk = null; },
      desk_preview: ({ web }) => {
        const n = web.naming;
        const words = [...seo(n.prefix), ...seo(n.keywords_all || ''), ...(n.append_original ? ['img', '4711'] : []), '0001'];
        const name = n.separator === 'caps' ? words.map(w => w[0].toUpperCase() + w.slice(1)).join('') : words.join(n.separator === 'underscore' ? '_' : '-');
        return `${name}.${web.format}`;
      },
      desk_estimate: ({ settings: s }) => (s.videos ? 'etwa 9 Minuten' : s.video_transcript || s.audio_transcript ? 'etwa 4 Minuten' : null),
      desk_start: () => {
        const total = 30;
        let done = 0;
        const t = setInterval(() => {
          done++;
          emit('progress', { done, total, current: `Kunde/Team/IMG_${4700 + done}.heic`, fraction: done / total, ok: true });
          if (done === total) {
            clearInterval(t);
            emit('desk-finished', { converted: 29, cancelled: false, saved: '41,7 MB → 1,2 MB (97 % kleiner)', duration: '12 Sekunden',
              lines: ['3 Bilder aus PDFs geholt.', '1 Video: Original übernommen, war schon gut gepackt.'],
              failed: [['Kunde/kaputt.heic', 'Das Bild lässt sich nicht lesen']], output: '/Users/du/Kunde/wolfgang_output', output_folder: 'Kunde/wolfgang_output' });
          }
        }, 60);
      },
      save_preset: ({ name }) => { presets = [...presets.filter(p => p.name !== name), { name, description: 'fürs Web als WebP, 1600 px, Qualität 80' }]; return presets.map(p => ({ name: p.name, settings: lastRun })); },
      settings: () => settings,
      set_settings: ({ defaults }) => (settings = defaults),
      presets: () => presets,
      remove_preset: ({ name }) => (presets = presets.filter(p => p.name !== name)),
      about: () => ({ version: '2.1.0 (Vorschau)', cli_installed: false }),
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
      pick_files: () => ['/demo/mode'],
      take_pending: () => [],
      character: () => character,
      clipboard_text: () => 'example.com',
      frontend_ready: () => {},
      check_looking: ({ url }) => {
        if (!url.includes('.')) throw `„${url}“ ist keine Adresse, mit der ich was anfangen kann`;
        return `Moment, ich guck mir ${url} mal an …`;
      },
      check_url: () => new Promise(ok => setTimeout(() => ok({ verdict: 'odd', mood: 'thinking', says: "Geht – aber guck mal: Die Seite bleibt auf http – es gibt keine Weiterleitung auf https.", finding: 'Die Seite bleibt auf http – es gibt keine Weiterleitung auf https.', report: exampleReport }), 2500)),
      transcription: () => addon,
      install_transcription: () => {
        // Pretends to download the add-on in about 8 seconds.
        if (addon.installing) return addon;
        addon = { ...addon, installing: true };
        let done = 0;
        addonTimer = setInterval(() => {
          done = Math.min(addon.size, done + 30_000_000);
          emit('transcription-progress', { done, total: addon.size });
          if (done === addon.size) { clearInterval(addonTimer); addon = { ...addon, installing: false, installed: true }; emit('transcription-done', { Ok: addon }); }
        }, 400);
        return addon;
      },
      cancel_transcription_install: () => { clearInterval(addonTimer); addon = { ...addon, installing: false }; emit('transcription-done', { Ok: addon }); },
      remove_transcription: () => (addon = { ...addon, installed: false }),
      check_update: () => (new URLSearchParams(location.search).has('update') ? { version: '2.2.0' } : null),
      install_update: () => new Promise(() => {
        // Pretends to download 18 MB; in the real app Wolfgang restarts at the end.
        const total = 18_000_000;
        let done = 0;
        const t = setInterval(() => { done = Math.min(total, done + 900_000); emit('update-progress', { done, total }); if (done === total) clearInterval(t); }, 100);
      }),
    };

    if (addonParam === 'installing') setTimeout(() => commands.install_transcription(), 200);

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
