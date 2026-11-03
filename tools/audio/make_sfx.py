#!/usr/bin/env python3
"""Synthesize the game's sound effects into assets/audio/sfx/*.wav.

Every sound is generated here from oscillators and seeded noise: no recordings,
no downloads, nothing copyrighted (CC0-safe by construction). Output: 16-bit
mono PCM WAV at 44.1 kHz, short, peak about -6 dBFS, faded in and out so the
game never clicks. Deterministic: the same script writes the same bytes.

    python3 tools/audio/make_sfx.py [output-dir]
"""
import os
import sys
import wave

import numpy as np

RATE = 44100
PEAK = 0.5  # about -6 dBFS
LOUDNESS = 0.09  # RMS every sound is brought to before its peaks are limited
rng = np.random.default_rng(20261013)


def t_axis(seconds):
    return np.arange(int(RATE * seconds)) / RATE


def envelope(n, attack=0.005, release=0.05, decay=None):
    """Linear attack, optional exponential decay, linear release (in seconds)."""
    env = np.ones(n)
    a = max(1, int(attack * RATE))
    r = max(1, int(release * RATE))
    env[:a] = np.linspace(0.0, 1.0, a)
    if decay is not None:
        env *= np.exp(-np.arange(n) / (decay * RATE))
    env[-r:] *= np.linspace(1.0, 0.0, r)
    return env


def tone(freq, seconds, partials=((1, 1.0),), decay=None, attack=0.005, release=0.05, vibrato=0.0):
    t = t_axis(seconds)
    wobble = 1.0 + vibrato * np.sin(2 * np.pi * 5.5 * t)
    out = np.zeros_like(t)
    for ratio, gain in partials:
        out += gain * np.sin(2 * np.pi * freq * ratio * wobble * t)
    return out * envelope(len(t), attack, release, decay)


def noise(seconds):
    return rng.standard_normal(int(RATE * seconds))


def lowpass(signal, cutoff):
    """One-pole low-pass filter."""
    alpha = 1.0 - np.exp(-2 * np.pi * cutoff / RATE)
    out = np.empty_like(signal)
    acc = 0.0
    for i, x in enumerate(signal):
        acc += alpha * (x - acc)
        out[i] = acc
    return out


def highpass(signal, cutoff):
    return signal - lowpass(signal, cutoff)


def sweep(f0, f1, seconds, decay=None):
    t = t_axis(seconds)
    freq = f0 * (f1 / f0) ** (t / seconds)
    phase = 2 * np.pi * np.cumsum(freq) / RATE
    return np.sin(phase) * envelope(len(t), 0.003, 0.05, decay)


def place(total, *parts):
    """Mix (start seconds, signal) parts into a buffer of `total` seconds."""
    out = np.zeros(int(RATE * total))
    for start, signal in parts:
        i = int(start * RATE)
        n = min(len(signal), len(out) - i)
        out[i:i + n] += signal[:n]
    return out


def bell(freq, seconds, decay=0.35):
    # Inharmonic partials of a small bell.
    return tone(freq, seconds, ((1, 1.0), (2.76, 0.45), (5.4, 0.2), (8.93, 0.08)), decay=decay, attack=0.002)


def horn(freq, seconds):
    return tone(freq, seconds, ((1, 1.0), (2, 0.5), (3, 0.3), (4, 0.15), (5, 0.08)), attack=0.06, release=0.25, vibrato=0.003)


def select():
    click = lowpass(noise(0.03), 3000) * envelope(int(RATE * 0.03), 0.001, 0.02, decay=0.006)
    return place(0.25, (0.0, click * 0.6), (0.01, bell(1320, 0.22, decay=0.07) * 0.5))


def move():
    steps = []
    for i in range(3):
        thud = lowpass(noise(0.09), 500) * envelope(int(RATE * 0.09), 0.002, 0.03, decay=0.025)
        rustle = lowpass(highpass(noise(0.07), 1500), 5000) * envelope(int(RATE * 0.07), 0.002, 0.03, decay=0.015) * 0.15
        steps.append((0.12 * i, thud + np.pad(rustle, (0, len(thud) - len(rustle)))))
    return place(0.42, *steps)


def melee():
    hit = highpass(noise(0.05), 1500) * envelope(int(RATE * 0.05), 0.001, 0.02, decay=0.01)
    ring = tone(1830, 0.5, ((1, 1.0), (1.47, 0.6), (2.31, 0.4), (3.07, 0.2)), decay=0.09, attack=0.001)
    clash2 = tone(2410, 0.35, ((1, 1.0), (1.53, 0.5)), decay=0.06, attack=0.001)
    body = lowpass(noise(0.12), 400) * envelope(int(RATE * 0.12), 0.002, 0.04, decay=0.03)
    return place(0.6, (0.0, hit), (0.0, ring * 0.35), (0.07, clash2 * 0.25), (0.0, body * 0.8))


def archery():
    n = int(RATE * 0.35)
    whoosh = highpass(lowpass(noise(0.35), 2400), 600) * np.sin(np.linspace(0, np.pi, n)) ** 2
    thunk = lowpass(noise(0.08), 700) * envelope(int(RATE * 0.08), 0.001, 0.03, decay=0.02)
    twang = tone(196, 0.3, ((1, 1.0), (2, 0.4), (3, 0.2)), decay=0.07, attack=0.001)
    return place(0.75, (0.0, twang * 0.35), (0.05, whoosh * 0.5), (0.4, thunk))


def gunpowder():
    crack = lowpass(highpass(noise(0.06), 1200), 6000) * envelope(int(RATE * 0.06), 0.0005, 0.03, decay=0.008)
    boom = lowpass(noise(0.4), 300) * envelope(int(RATE * 0.4), 0.001, 0.1, decay=0.08) * 2.0
    tail = lowpass(noise(0.5), 1200) * envelope(int(RATE * 0.5), 0.02, 0.2, decay=0.15) * 0.15
    return place(0.75, (0.0, crack * 0.6), (0.0, boom * 1.4), (0.05, tail))


def artillery():
    thump = sweep(90, 35, 0.9, decay=0.25)
    blast = lowpass(noise(0.9), 220) * envelope(int(RATE * 0.9), 0.002, 0.2, decay=0.22)
    crack = highpass(noise(0.05), 900) * envelope(int(RATE * 0.05), 0.0005, 0.02, decay=0.01)
    return place(1.1, (0.0, thump * 1.2), (0.0, blast * 1.8), (0.0, crack * 0.6))


def naval():
    boom = lowpass(noise(0.7), 260) * envelope(int(RATE * 0.7), 0.002, 0.2, decay=0.16)
    thump = sweep(80, 40, 0.6, decay=0.18)
    n = int(RATE * 0.6)
    splash = highpass(lowpass(noise(0.6), 5000), 900) * np.sin(np.linspace(0, np.pi, n)) ** 3
    return place(1.3, (0.0, boom * 1.5), (0.0, thump), (0.55, splash * 0.45))


def air():
    n = int(RATE * 0.8)
    drone = highpass(lowpass(noise(0.8), 1800), 300) * np.sin(np.linspace(0, np.pi, n)) ** 2
    pitch = sweep(420, 180, 0.8) * 0.15
    blast = lowpass(noise(0.6), 300) * envelope(int(RATE * 0.6), 0.002, 0.15, decay=0.15)
    return place(1.35, (0.0, drone * 0.6 + pitch), (0.7, blast * 1.4))


def death():
    fall = sweep(220, 70, 0.6, decay=0.3) * 0.5
    thud = lowpass(noise(0.2), 250) * envelope(int(RATE * 0.2), 0.002, 0.06, decay=0.05)
    groan = tone(110, 0.5, ((1, 1.0), (2, 0.3)), attack=0.03, release=0.3, decay=0.2) * 0.3
    return place(0.85, (0.0, groan), (0.05, fall), (0.45, thud * 1.2))


def chord(root, intervals, seconds, voice=horn):
    return sum(voice(root * 2 ** (i / 12), seconds) for i in intervals) / len(intervals)


def city_founded():
    parts = [(0.0, horn(196, 0.9) * 0.6), (0.18, horn(247, 0.8) * 0.5), (0.36, horn(294, 0.9) * 0.5)]
    pad = chord(196, (0, 4, 7, 12), 1.2) * 0.5
    return place(1.6, *parts, (0.55, pad))


def building():
    taps = [(0.11 * i, bell(1568 * (1 if i % 2 == 0 else 1.335), 0.3, decay=0.05) * 0.4) for i in range(3)]
    knock = lowpass(noise(0.06), 900) * envelope(int(RATE * 0.06), 0.001, 0.02, decay=0.012)
    return place(0.95, (0.0, knock), (0.12, knock), *taps, (0.35, bell(784, 0.55, decay=0.2) * 0.5))


def wonder():
    fanfare = [(0.0, horn(262, 0.5)), (0.22, horn(330, 0.45)), (0.44, horn(392, 0.45)), (0.66, horn(523, 1.1))]
    pad = chord(262, (0, 4, 7, 12, 16), 1.6) * 0.8
    shimmer = sum(bell(2093 * 2 ** (i / 12), 0.8, decay=0.3) for i in (0, 4, 7)) * 0.12
    return place(2.4, *[(s, x * 0.45) for s, x in fanfare], (0.66, pad), (0.7, shimmer))


def tech():
    notes = [523, 659, 784, 1047]
    return place(1.2, *[(0.09 * i, bell(f, 0.6, decay=0.25) * 0.45) for i, f in enumerate(notes)])


def policy():
    gong = tone(147, 1.4, ((1, 1.0), (2.01, 0.5), (2.98, 0.3), (4.2, 0.2)), decay=0.45, attack=0.01)
    pad = chord(294, (0, 7, 12), 1.2, voice=lambda f, s: tone(f, s, ((1, 1.0), (2, 0.2)), attack=0.15, release=0.4))
    return place(1.5, (0.0, gong * 0.6), (0.1, pad * 0.5))


def faith():
    voice = lambda f, s: tone(f, s, ((1, 1.0), (2, 0.35), (3, 0.15)), attack=0.25, release=0.5, vibrato=0.004)
    pad = chord(220, (0, 7, 12, 16), 1.6, voice=voice)
    return place(1.8, (0.0, pad * 0.9), (0.4, bell(1760, 0.9, decay=0.3) * 0.2))


def notification():
    return place(0.6, (0.0, bell(988, 0.4, decay=0.12) * 0.5), (0.13, bell(1319, 0.45, decay=0.14) * 0.5))


SOUNDS = {
    "select": select,
    "move": move,
    "melee": melee,
    "archery": archery,
    "gunpowder": gunpowder,
    "artillery": artillery,
    "naval": naval,
    "air": air,
    "death": death,
    "city_founded": city_founded,
    "building": building,
    "wonder": wonder,
    "tech": tech,
    "policy": policy,
    "faith": faith,
    "notification": notification,
}


def write(path, signal):
    signal = signal - np.mean(signal)
    fade = min(len(signal) // 4, int(0.01 * RATE))
    signal[:fade] *= np.linspace(0.0, 1.0, fade)
    signal[-fade:] *= np.linspace(1.0, 0.0, fade)
    rms = np.sqrt(np.mean(signal ** 2))
    if rms > 0:
        signal = signal / rms * LOUDNESS
    # Soft limiter: transients (cracks, clicks) stay under PEAK without a hard clip.
    signal = PEAK * np.tanh(signal / PEAK)
    pcm = (signal * 32767).astype("<i2")
    with wave.open(path, "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(pcm.tobytes())


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    target = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "..", "assets", "audio", "sfx")
    target = os.path.realpath(target)
    os.makedirs(target, exist_ok=True)
    for name, make in SOUNDS.items():
        path = os.path.join(target, name + ".wav")
        write(path, make())
        print(f"{name}.wav {os.path.getsize(path)} bytes")


if __name__ == "__main__":
    main()
