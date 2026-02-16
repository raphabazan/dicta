// Generate sound effects using Web Audio API

function createAudioContext(): AudioContext {
  const ctx = new AudioContext();
  if (ctx.state === "suspended") {
    ctx.resume();
  }
  return ctx;
}

export function playStartSound() {
  const audioContext = createAudioContext();
  const oscillator = audioContext.createOscillator();
  const gainNode = audioContext.createGain();

  oscillator.connect(gainNode);
  gainNode.connect(audioContext.destination);

  // Start sound: ascending beep (800Hz -> 1000Hz)
  oscillator.frequency.setValueAtTime(800, audioContext.currentTime);
  oscillator.frequency.linearRampToValueAtTime(1000, audioContext.currentTime + 0.1);

  gainNode.gain.setValueAtTime(0.3, audioContext.currentTime);
  gainNode.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.15);

  oscillator.start(audioContext.currentTime);
  oscillator.stop(audioContext.currentTime + 0.15);
}

export function playStopSound() {
  const audioContext = createAudioContext();
  const oscillator = audioContext.createOscillator();
  const gainNode = audioContext.createGain();

  oscillator.connect(gainNode);
  gainNode.connect(audioContext.destination);

  // Stop sound: descending beep (1000Hz -> 600Hz)
  oscillator.frequency.setValueAtTime(1000, audioContext.currentTime);
  oscillator.frequency.linearRampToValueAtTime(600, audioContext.currentTime + 0.15);

  gainNode.gain.setValueAtTime(0.3, audioContext.currentTime);
  gainNode.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.2);

  oscillator.start(audioContext.currentTime);
  oscillator.stop(audioContext.currentTime + 0.2);
}

export function playCancelSound() {
  const audioContext = createAudioContext();
  const oscillator = audioContext.createOscillator();
  const gainNode = audioContext.createGain();

  oscillator.connect(gainNode);
  gainNode.connect(audioContext.destination);

  // Cancel sound: short error beep (400Hz)
  oscillator.frequency.setValueAtTime(400, audioContext.currentTime);

  gainNode.gain.setValueAtTime(0.3, audioContext.currentTime);
  gainNode.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.1);

  oscillator.start(audioContext.currentTime);
  oscillator.stop(audioContext.currentTime + 0.1);
}

export function playResponseSound() {
  const audioContext = createAudioContext();

  // First chime: 880Hz
  const osc1 = audioContext.createOscillator();
  const gain1 = audioContext.createGain();
  osc1.connect(gain1);
  gain1.connect(audioContext.destination);
  osc1.frequency.setValueAtTime(880, audioContext.currentTime);
  gain1.gain.setValueAtTime(0.3, audioContext.currentTime);
  gain1.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.12);
  osc1.start(audioContext.currentTime);
  osc1.stop(audioContext.currentTime + 0.12);

  // Second chime: 1100Hz (slightly delayed)
  const osc2 = audioContext.createOscillator();
  const gain2 = audioContext.createGain();
  osc2.connect(gain2);
  gain2.connect(audioContext.destination);
  osc2.frequency.setValueAtTime(1100, audioContext.currentTime + 0.1);
  gain2.gain.setValueAtTime(0.01, audioContext.currentTime);
  gain2.gain.setValueAtTime(0.3, audioContext.currentTime + 0.1);
  gain2.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.25);
  osc2.start(audioContext.currentTime + 0.1);
  osc2.stop(audioContext.currentTime + 0.25);
}
