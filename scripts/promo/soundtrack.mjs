/**
 * Procedural 48 kHz stereo studio soundtrack + synchronized UI SFX synthesizer
 * for the Termexo Fleet 45-second launch film (pure Node.js, zero dependencies).
 */
import { writeFileSync } from 'node:fs';

const SAMPLE_RATE = 48000;
const DURATION = 45.0;
const TOTAL_SAMPLES = Math.floor(SAMPLE_RATE * DURATION);

// Musical tempo: 116 BPM
const BPM = 116;
const BEAT = 60 / BPM; // 0.51724s

const midiToHz = (m) => 440 * 2 ** ((m - 69) / 12);

// Lush dark-tech progression in F# minor / A major (Apple / Linear launch vibe)
// F#m9 -> Dmaj7 -> Aadd9 -> E6/9
const CHORDS = [
  { bass: 42, pad: [54, 57, 61, 64, 68], arp: [66, 69, 73, 76, 80, 76] }, // F#m9
  { bass: 38, pad: [50, 57, 61, 66, 69], arp: [62, 66, 69, 73, 78, 73] }, // Dmaj7
  { bass: 45, pad: [52, 57, 61, 64, 71], arp: [64, 69, 73, 76, 83, 76] }, // Aadd9
  { bass: 40, pad: [52, 56, 59, 61, 66], arp: [64, 68, 71, 73, 78, 73] }, // E6/9
];

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

function smoothstep(edge0, edge1, x) {
  const t = clamp((x - edge0) / (edge1 - edge0), 0, 1);
  return t * t * (3 - 2 * t);
}

// Simple deterministic PRNG for silky filtered noise (shakers / whooshes)
let seed = 0x5f3759df;
function rand() {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return (seed / 0xffffffff) * 2 - 1;
}

// One-pole low-pass filter state helper
class LowPass {
  constructor() {
    this.y = 0;
  }
  process(x, cutoffHz) {
    const rc = 1 / (2 * Math.PI * Math.max(20, cutoffHz));
    const dt = 1 / SAMPLE_RATE;
    const alpha = dt / (rc + dt);
    this.y += alpha * (x - this.y);
    return this.y;
  }
}

class HighPass {
  constructor() {
    this.xPrev = 0;
    this.y = 0;
  }
  process(x, cutoffHz) {
    const rc = 1 / (2 * Math.PI * Math.max(20, cutoffHz));
    const dt = 1 / SAMPLE_RATE;
    const alpha = rc / (rc + dt);
    this.y = alpha * (this.y + x - this.xPrev);
    this.xPrev = x;
    return this.y;
  }
}

function addWhoosh(L, R, centerSec, durSec = 0.75, amp = 0.16, pan = 0) {
  const start = Math.max(0, Math.floor((centerSec - durSec * 0.65) * SAMPLE_RATE));
  const end = Math.min(TOTAL_SAMPLES, Math.floor((centerSec + durSec * 0.35) * SAMPLE_RATE));
  const lpL = new LowPass();
  const lpR = new LowPass();
  const hpL = new HighPass();
  const hpR = new HighPass();
  const total = Math.max(1, end - start);
  for (let i = start; i < end; i++) {
    const u = (i - start) / total;
    const env = Math.sin(Math.PI * u) ** 1.7;
    const freq = 220 + 2600 * Math.sin(Math.PI * u) ** 1.4;
    const nL = rand();
    const nR = rand();
    const sL = hpL.process(lpL.process(nL, freq), 140) * env * amp * (1 - Math.max(0, pan) * 0.4);
    const sR = hpR.process(lpR.process(nR, freq), 140) * env * amp * (1 - Math.max(0, -pan) * 0.4);
    L[i] += sL;
    R[i] += sR;
  }
}

function addSubImpact(L, R, startSec, amp = 0.34) {
  const start = Math.max(0, Math.floor(startSec * SAMPLE_RATE));
  const len = Math.floor(1.15 * SAMPLE_RATE);
  const end = Math.min(TOTAL_SAMPLES, start + len);
  let phase = 0;
  for (let i = start; i < end; i++) {
    const t = (i - start) / SAMPLE_RATE;
    const freq = 42 + 78 * Math.exp(-t * 8.5);
    phase += (2 * Math.PI * freq) / SAMPLE_RATE;
    const env = Math.min(1, t / 0.008) * Math.exp(-t * 3.1);
    const s = Math.sin(phase) * env * amp;
    L[i] += s;
    R[i] += s;
  }
}

function addGlassChime(L, R, startSec, midiNotes, amp = 0.12, spread = 0.045) {
  midiNotes.forEach((midi, idx) => {
    const t0 = startSec + idx * spread;
    const start = Math.max(0, Math.floor(t0 * SAMPLE_RATE));
    const len = Math.floor(1.4 * SAMPLE_RATE);
    const end = Math.min(TOTAL_SAMPLES, start + len);
    const freq = midiToHz(midi);
    const pan = midiNotes.length > 1 ? (idx / (midiNotes.length - 1) - 0.5) * 0.6 : 0;
    const gainL = Math.cos(((pan + 1) * Math.PI) / 4);
    const gainR = Math.sin(((pan + 1) * Math.PI) / 4);
    for (let i = start; i < end; i++) {
      const t = (i - start) / SAMPLE_RATE;
      const env = Math.min(1, t / 0.004) * Math.exp(-t * 3.4);
      const fm = Math.sin(2 * Math.PI * freq * 2.0 * t) * Math.exp(-t * 9) * 0.65;
      const tone = (Math.sin(2 * Math.PI * freq * t + fm) + 0.25 * Math.sin(2 * Math.PI * freq * 3 * t) * Math.exp(-t * 7)) * env * amp;
      L[i] += tone * gainL;
      R[i] += tone * gainR;
    }
  });
}

function addClick(L, R, startSec, pitch = 1900, amp = 0.11) {
  const start = Math.max(0, Math.floor(startSec * SAMPLE_RATE));
  const len = Math.floor(0.045 * SAMPLE_RATE);
  const end = Math.min(TOTAL_SAMPLES, start + len);
  for (let i = start; i < end; i++) {
    const t = (i - start) / SAMPLE_RATE;
    const env = Math.exp(-t * 115);
    const s = (Math.sin(2 * Math.PI * pitch * t) * 0.7 + rand() * 0.25) * env * amp;
    L[i] += s * 0.92;
    R[i] += s * 1.05;
  }
}

export function generateSoundtrackWav(outputWavPath) {
  const L = new Float32Array(TOTAL_SAMPLES);
  const R = new Float32Array(TOTAL_SAMPLES);

  // Persistent oscillator phases for smooth detuned stereo pad & warm bass
  const padPhasesL = new Float64Array(10);
  const padPhasesR = new Float64Array(10);
  let bassPhase = 0;
  const padLpL = new LowPass();
  const padLpR = new LowPass();
  const hatHp = new HighPass();

  // Soft stereo ping-pong delay buffer for crystalline arp & sheen
  const delaySamplesL = Math.floor(BEAT * 0.75 * SAMPLE_RATE);
  const delaySamplesR = Math.floor(BEAT * 1.5 * SAMPLE_RATE);
  const delayBufL = new Float32Array(TOTAL_SAMPLES + delaySamplesR + 1);
  const delayBufR = new Float32Array(TOTAL_SAMPLES + delaySamplesR + 1);

  // Schedule crisp glassy arpeggio notes along the groove (Acts 2..7)
  const arpStep = BEAT / 2; // 8th notes
  for (let stepIdx = 0; ; stepIdx++) {
    const tNote = 5.2 + stepIdx * arpStep;
    if (tNote >= 39.0) break;
    const bar = Math.floor(tNote / (BEAT * 4));
    const chord = CHORDS[bar % CHORDS.length];
    const pattern = [0, 2, 1, 3, 2, 4, 3, 5];
    const midi = chord.arp[pattern[stepIdx % pattern.length]];
    const freq = midiToHz(midi);
    const energy =
      smoothstep(5.2, 7.5, tNote) *
      (1 - 0.55 * smoothstep(17.2, 18.0, tNote) * (1 - smoothstep(21.6, 22.4, tNote))) *
      (1 - smoothstep(37.6, 39.2, tNote));
    const noteAmp = 0.068 * energy * (stepIdx % 2 === 0 ? 1.0 : 0.72);
    if (noteAmp < 0.002) continue;

    const pan = stepIdx % 2 === 0 ? -0.35 : 0.35;
    const gL = Math.cos(((pan + 1) * Math.PI) / 4);
    const gR = Math.sin(((pan + 1) * Math.PI) / 4);
    const start = Math.floor(tNote * SAMPLE_RATE);
    const end = Math.min(TOTAL_SAMPLES, start + Math.floor(0.52 * SAMPLE_RATE));
    for (let i = start; i < end; i++) {
      const dt = (i - start) / SAMPLE_RATE;
      const env = Math.min(1, dt / 0.005) * Math.exp(-dt * 6.8);
      const sig =
        (Math.sin(2 * Math.PI * freq * dt) * 0.78 +
          Math.sin(2 * Math.PI * freq * 2 * dt) * 0.22 * Math.exp(-dt * 10)) *
        env *
        noteAmp;
      L[i] += sig * gL;
      R[i] += sig * gR;
      delayBufL[i + delaySamplesL] += sig * gR * 0.34;
      delayBufR[i + delaySamplesR] += sig * gL * 0.24;
    }
  }

  // Main continuous synthesis loop: warm stereo pad, sub-bass pulse, minimal groove
  for (let i = 0; i < TOTAL_SAMPLES; i++) {
    const t = i / SAMPLE_RATE;

    // Chord index (switches every 2 bars = 8 beats)
    const chordIdx = t >= 39.2 ? 2 : Math.floor(t / (BEAT * 8)) % CHORDS.length;
    const chord = CHORDS[chordIdx];

    // Smooth global musical arc
    const introFade = smoothstep(0.0, 0.7, t);
    const outroFade = 1 - smoothstep(42.4, 44.8, t);
    const masterEnv = introFade * outroFade;

    // Beat position for gentle sidechain pumping during active groove
    const grooveActive = smoothstep(5.2, 6.2, t) * (1 - smoothstep(38.6, 39.4, t));
    const beatFrac = ((t - 5.2) % BEAT + BEAT) % BEAT;
    const sidechain = 1 - 0.24 * grooveActive * Math.exp(-beatFrac * 11);

    // 1. Lush warm detuned stereo synth pad
    let rawPadL = 0;
    let rawPadR = 0;
    for (let n = 0; n < chord.pad.length; n++) {
      const baseHz = midiToHz(chord.pad[n]);
      const hzL = baseHz * (1 - 0.0024 * (n % 2 === 0 ? 1 : -1));
      const hzR = baseHz * (1 + 0.0028 * (n % 2 === 0 ? 1 : -1));
      padPhasesL[n] = (padPhasesL[n] + hzL / SAMPLE_RATE) % 1;
      padPhasesR[n] = (padPhasesR[n] + hzR / SAMPLE_RATE) % 1;
      // Softened triangle + warm saw blend
      const triL = 1 - 4 * Math.abs(padPhasesL[n] - 0.5);
      const sawL = 2 * padPhasesL[n] - 1;
      const triR = 1 - 4 * Math.abs(padPhasesR[n] - 0.5);
      const sawR = 2 * padPhasesR[n] - 1;
      rawPadL += triL * 0.65 + sawL * 0.35;
      rawPadR += triR * 0.65 + sawR * 0.35;
    }
    rawPadL /= chord.pad.length;
    rawPadR /= chord.pad.length;

    // Dynamic filter sweep matching story tension
    const cutoff =
      420 +
      1050 * smoothstep(2.0, 6.5, t) -
      450 * smoothstep(17.2, 18.2, t) * (1 - smoothstep(21.5, 22.8, t)) +
      360 * smoothstep(27.5, 31.0, t) -
      620 * smoothstep(39.4, 43.8, t);

    const padAmp = 0.21 * sidechain;
    const fPadL = padLpL.process(rawPadL, cutoff) * padAmp;
    const fPadR = padLpR.process(rawPadR, cutoff) * padAmp;

    // 2. Deep warm sub-bass (pulsing 8th notes during groove, sustained drone in intro/outro)
    const bassHz = midiToHz(chord.bass);
    bassPhase += (2 * Math.PI * bassHz) / SAMPLE_RATE;
    const eighthFrac = ((t - 5.2) % (BEAT / 2) + BEAT / 2) % (BEAT / 2);
    const bassPulse = 0.45 + 0.55 * Math.exp(-eighthFrac * 5.2);
    const bassLevel =
      0.16 * smoothstep(0.6, 3.0, t) * (1 - 0.55 * smoothstep(39.5, 42.5, t)) * (grooveActive > 0.05 ? bassPulse : 0.72);
    const bassSig = (Math.sin(bassPhase) * 0.85 + Math.sin(bassPhase * 2) * 0.15) * bassLevel;

    // 3. Minimal deep kick + silky high-hat shimmer during active acts
    let percL = 0;
    let percR = 0;
    if (grooveActive > 0.01 && t >= 5.2) {
      // Soft modern electronic kick on each beat
      const kickEnv = Math.exp(-beatFrac * 18);
      const kickFreq = 46 + 88 * Math.exp(-beatFrac * 36);
      const kick = Math.sin(2 * Math.PI * kickFreq * beatFrac) * kickEnv * 0.24 * grooveActive;
      percL += kick;
      percR += kick;

      // Soft brushed hat on off-beats
      const offBeat = ((t - 5.2 + BEAT / 2) % BEAT + BEAT) % BEAT;
      if (offBeat < 0.06) {
        const hatEnv = Math.exp(-offBeat * 72);
        const hat = hatHp.process(rand(), 6800) * hatEnv * 0.042 * grooveActive;
        percL += hat * 0.85;
        percR += hat * 1.15;
      }
    }

    L[i] += (fPadL + bassSig + percL + delayBufL[i]) * masterEnv;
    R[i] += (fPadR + bassSig + percR + delayBufR[i]) * masterEnv;
  }

  // ==================== FRAME-ACCURATE CINEMATIC SFX ====================
  // Act 1 -> Act 2 Hero Reveal (5.2s)
  addWhoosh(L, R, 5.1, 0.9, 0.2, 0);
  addSubImpact(L, R, 5.2, 0.36);
  addGlassChime(L, R, 5.35, [69, 73, 76, 81], 0.085, 0.055);

  // Act 3 Mission Deck & Kinetic Typing (10.5s - 17.5s)
  addWhoosh(L, R, 10.45, 0.65, 0.13, -0.3);
  for (let k = 0; k < 11; k++) {
    addClick(L, R, 11.45 + k * 0.145, 1750 + (k % 3) * 180, 0.065);
  }
  addGlassChime(L, R, 13.35, [73, 76, 80], 0.08, 0.04); // Live route forecast lock-on!

  // Act 4 Slide-over Live Terminal Drawer (17.5s) + Human Approval Click (20.28s)
  addClick(L, R, 17.42, 2200, 0.1);
  addWhoosh(L, R, 17.55, 0.55, 0.14, 0.5);
  addClick(L, R, 20.28, 2350, 0.095);
  addGlassChime(L, R, 20.34, [73, 80], 0.08, 0.04);

  // Act 5 L1 -> L3 -> L4 Interactive Ladder Switches (22.2s - 28.0s)
  addWhoosh(L, R, 22.15, 0.65, 0.14, -0.2);
  addClick(L, R, 23.4, 2100, 0.09);
  addGlassChime(L, R, 23.42, [71, 76], 0.075, 0.03); // L1 Sky
  addClick(L, R, 25.0, 2300, 0.09);
  addGlassChime(L, R, 25.02, [73, 78], 0.08, 0.03); // L3 Amber
  addClick(L, R, 26.5, 2500, 0.09);
  addGlassChime(L, R, 26.52, [76, 80, 83], 0.09, 0.035); // L4 Cross-Review

  // Act 6 Swarm Tree & Multi-Machine Topology (28.0s - 34.5s)
  addWhoosh(L, R, 27.95, 0.65, 0.14, 0.25);
  addWhoosh(L, R, 31.3, 0.6, 0.13, -0.35);
  addGlassChime(L, R, 31.55, [69, 76], 0.075, 0.04);

  // Act 7 Analytics 50% Savings & Ctrl+K Palette (34.5s - 39.2s)
  addWhoosh(L, R, 34.45, 0.6, 0.14, 0);
  addGlassChime(L, R, 35.3, [73, 76, 81], 0.085, 0.04);
  addClick(L, R, 37.35, 2400, 0.1); // Ctrl+K pop

  // Act 8 Iconic Finale & npx CTA (39.2s - 45.0s)
  addWhoosh(L, R, 39.15, 0.85, 0.17, 0);
  addSubImpact(L, R, 39.25, 0.3);
  addGlassChime(L, R, 39.6, [69, 73, 76, 81, 85], 0.1, 0.065);

  // Soft-clip mastering + peak normalization to -1.5 dBFS
  let peak = 0;
  for (let i = 0; i < TOTAL_SAMPLES; i++) {
    L[i] = Math.tanh(L[i] * 1.15);
    R[i] = Math.tanh(R[i] * 1.15);
    const m = Math.max(Math.abs(L[i]), Math.abs(R[i]));
    if (m > peak) peak = m;
  }
  const norm = peak > 0 ? 0.84 / peak : 1;

  // Encode 16-bit stereo WAV
  const dataBytes = TOTAL_SAMPLES * 4;
  const buf = Buffer.alloc(44 + dataBytes);
  buf.write('RIFF', 0);
  buf.writeUInt32LE(36 + dataBytes, 4);
  buf.write('WAVE', 8);
  buf.write('fmt ', 12);
  buf.writeUInt32LE(16, 16); // PCM chunk size
  buf.writeUInt16LE(1, 20); // PCM format
  buf.writeUInt16LE(2, 22); // Stereo
  buf.writeUInt32LE(SAMPLE_RATE, 24);
  buf.writeUInt32LE(SAMPLE_RATE * 4, 28); // Byte rate
  buf.writeUInt16LE(4, 32); // Block align
  buf.writeUInt16LE(16, 34); // Bits per sample
  buf.write('data', 36);
  buf.writeUInt32LE(dataBytes, 40);

  let offset = 44;
  for (let i = 0; i < TOTAL_SAMPLES; i++) {
    const sL = Math.round(clamp(L[i] * norm, -1, 1) * 32767);
    const sR = Math.round(clamp(R[i] * norm, -1, 1) * 32767);
    buf.writeInt16LE(sL, offset);
    buf.writeInt16LE(sR, offset + 2);
    offset += 4;
  }

  writeFileSync(outputWavPath, buf);
  return { duration: DURATION, sampleRate: SAMPLE_RATE, bytes: buf.length };
}
