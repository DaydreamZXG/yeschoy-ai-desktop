/**
 * 声音全部现场合成，不带任何音频素材。
 *
 * 浏览器要求先有一次点击才能出声，所以第一次按下时才建 AudioContext；
 * 在那之前调用的声音直接不响，不报错。静音开关记在本机。
 */
const MUTE_KEY = "kaifan.mute";

let context: AudioContext | null = null;
let bus: DynamicsCompressorNode | null = null;
let master: GainNode | null = null;

export function isMuted(): boolean {
  try {
    return localStorage.getItem(MUTE_KEY) === "1";
  } catch {
    return false;
  }
}

export function setMuted(muted: boolean) {
  try {
    if (muted) localStorage.setItem(MUTE_KEY, "1");
    else localStorage.removeItem(MUTE_KEY);
  } catch {
    /* 记不住就只管这一次。 */
  }
  if (master) master.gain.value = muted ? 0 : 0.7;
}

function wake() {
  const Ctor =
    window.AudioContext ??
    (window as unknown as { webkitAudioContext?: typeof AudioContext })
      .webkitAudioContext;
  if (!Ctor) return;
  if (!context) {
    context = new Ctor();
    bus = context.createDynamicsCompressor();
    master = context.createGain();
    master.gain.value = isMuted() ? 0 : 0.7;
    bus.connect(master).connect(context.destination);
  }
  if (context.state === "suspended") void context.resume();
}

let armed = false;
/** 第一次按下之后才允许出声。重复调用无害。 */
export function armSound() {
  if (armed || typeof window === "undefined") return;
  armed = true;
  window.addEventListener("pointerdown", wake, { capture: true });
  window.addEventListener("keydown", wake, { capture: true });
}

function tone(
  frequency: number,
  duration = 0.12,
  type: OscillatorType = "sine",
  volume = 0.16,
  delay = 0,
  glideTo = 0,
) {
  if (!context || !bus) return;
  const oscillator = context.createOscillator();
  const gain = context.createGain();
  const start = context.currentTime + delay;
  oscillator.type = type;
  oscillator.frequency.setValueAtTime(frequency, start);
  if (glideTo)
    oscillator.frequency.exponentialRampToValueAtTime(
      glideTo,
      start + duration,
    );
  gain.gain.setValueAtTime(0.0001, start);
  gain.gain.exponentialRampToValueAtTime(volume, start + 0.008);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
  oscillator.connect(gain).connect(bus);
  oscillator.start(start);
  oscillator.stop(start + duration + 0.05);
}

function noise(
  duration: number,
  volume: number,
  type: BiquadFilterType,
  from: number,
  to = 0,
  delay = 0,
) {
  if (!context || !bus) return;
  const length = Math.ceil(context.sampleRate * duration);
  const buffer = context.createBuffer(1, length, context.sampleRate);
  const channel = buffer.getChannelData(0);
  for (let i = 0; i < length; i++) channel[i] = Math.random() * 2 - 1;
  const source = context.createBufferSource();
  const filter = context.createBiquadFilter();
  const gain = context.createGain();
  const start = context.currentTime + delay;
  source.buffer = buffer;
  filter.type = type;
  filter.frequency.setValueAtTime(from, start);
  if (to) filter.frequency.exponentialRampToValueAtTime(to, start + duration);
  gain.gain.setValueAtTime(volume, start);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
  source.connect(filter).connect(gain).connect(bus);
  source.start(start);
}

export const sound = {
  tone,
  tick: () => tone(1500 + Math.random() * 500, 0.025, "square", 0.025),
  ok: () => {
    tone(880, 0.09, "sine", 0.14);
    tone(1318, 0.16, "sine", 0.14, 0.08);
  },
  pop: (frequency = 620) =>
    tone(frequency, 0.11, "sine", 0.2, 0, frequency * 2),
  squeak: () => {
    tone(900, 0.07, "triangle", 0.18, 0, 1500);
    tone(1400, 0.1, "triangle", 0.14, 0.07, 800);
  },
  sad: () => {
    tone(440, 0.18, "triangle", 0.14, 0, 330);
    tone(330, 0.28, "triangle", 0.14, 0.17, 220);
  },
  whoosh: (delay = 0) => noise(0.5, 0.22, "bandpass", 500, 7000, delay),
  impact: () => {
    tone(150, 0.5, "sine", 0.8, 0, 36);
    noise(1.1, 0.3, "highpass", 3500);
    noise(0.25, 0.4, "lowpass", 900);
  },
  chord: (frequencies: number[], duration = 2.2, delay = 0, volume = 0.05) =>
    frequencies.forEach((frequency) => {
      tone(frequency, duration, "sawtooth", volume, delay);
      tone(frequency * 1.006, duration, "sawtooth", volume, delay);
    }),
  drone: (duration: number) => {
    tone(55, duration, "sawtooth", 0.07);
    tone(82.4, duration, "sine", 0.1);
  },
  sweep: () => noise(0.9, 0.12, "bandpass", 300, 5000),
};

export const prefersCalm = () =>
  typeof window !== "undefined" &&
  !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
