// Little chiptune sounds, synthesised with WebAudio (no sound files).
'use strict';

const Sound = (() => {
  let ctx = null;
  let enabled = true;
  let lastBlip = 0;
  let unlocked = false; // no sound before the first click or key (browser rule)

  const ac = () => {
    if (!ctx) ctx = new (window.AudioContext || window.webkitAudioContext)();
    if (ctx.state === 'suspended') ctx.resume();
    return ctx;
  };
  // Browsers only allow sound after a user gesture.
  const unlock = () => { unlocked = true; if (enabled) ac(); };
  addEventListener('pointerdown', unlock, { once: true });
  addEventListener('keydown', unlock, { once: true });

  function tone(freq, dur, { type = 'square', vol = 0.04, when = 0, slide = 0 } = {}) {
    if (!enabled || !unlocked) return;
    const c = ac(), t = c.currentTime + when;
    const o = c.createOscillator(), g = c.createGain();
    o.type = type;
    o.frequency.setValueAtTime(freq, t);
    if (slide) o.frequency.linearRampToValueAtTime(freq + slide, t + dur);
    g.gain.setValueAtTime(vol, t);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g).connect(c.destination);
    o.start(t);
    o.stop(t + dur + 0.02);
  }

  function noise(dur, { vol = 0.06, cutoff = 900, when = 0 } = {}) {
    if (!enabled || !unlocked) return;
    const c = ac(), t = c.currentTime + when;
    const buf = c.createBuffer(1, Math.ceil(c.sampleRate * dur), c.sampleRate);
    const d = buf.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / d.length);
    const src = c.createBufferSource(), f = c.createBiquadFilter(), g = c.createGain();
    src.buffer = buf;
    f.type = 'lowpass';
    f.frequency.value = cutoff;
    g.gain.value = vol;
    src.connect(f).connect(g).connect(c.destination);
    src.start(t);
  }

  return {
    get enabled() { return enabled; },
    set enabled(v) { enabled = !!v; },
    /** Wolfgang's voice while text types: short, slightly varying blips. */
    blip() {
      const now = performance.now();
      if (now - lastBlip < 55) return;
      lastBlip = now;
      tone(300 + Math.random() * 90, 0.04, { type: 'triangle', vol: 0.05 });
    },
    move() { tone(660, 0.03, { vol: 0.02 }); },
    confirm() { tone(523, 0.05, { vol: 0.03 }); tone(784, 0.07, { vol: 0.03, when: 0.05 }); },
    back() { tone(523, 0.05, { vol: 0.03 }); tone(392, 0.07, { vol: 0.03, when: 0.05 }); },
    /** A stamp per finished file. */
    stamp() { noise(0.09, { vol: 0.09, cutoff: 600 }); tone(110, 0.06, { type: 'sine', vol: 0.06 }); },
    /** Files land on the desk. */
    plop() { noise(0.12, { vol: 0.07, cutoff: 1400 }); tone(220, 0.08, { type: 'sine', vol: 0.05, slide: -60 }); },
    jingle() { [523, 659, 784, 1047].forEach((f, i) => tone(f, 0.14, { vol: 0.035, when: i * 0.09 })); },
    sad() { [392, 349, 311].forEach((f, i) => tone(f, 0.18, { type: 'triangle', vol: 0.05, when: i * 0.14 })); },
    sip() { noise(0.25, { vol: 0.03, cutoff: 500 }); tone(180, 0.2, { type: 'sine', vol: 0.03, slide: 40, when: 0.05 }); },
  };
})();
