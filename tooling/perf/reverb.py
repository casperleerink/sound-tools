"""Compares the built-in reverb of two commits: impulse responses per octave band, and music
renders to listen to. Needs numpy. See README.md for the measures and the pass limits."""

import argparse
import math
import shutil
import sys

import numpy as np

import bench
import projects

BANDS = [125, 250, 500, 1000, 2000, 4000, 8000]
IR_SECONDS, MUSIC_SECONDS = 12, 10
# Pass limits, B against A.
RT_PART, ENERGY_DB, MIXING_MS, CORRELATION, RINGING_DB = 0.10, 1.0, 20.0, 0.1, 3.0


def channels(path):
    return np.array(bench.read_wav(path), dtype=np.float64).reshape(-1, 2).T


def band(ir, hz):
    """The octave band around `hz`, zero padded so nothing wraps. Flat for the middle half octave,
    with a cos² edge half an octave wide on each side, so neighbouring bands add up to 1. A brick
    wall would smear each band in time and lengthen a short decay."""
    size = 2 * ir.shape[1]
    spectrum = np.fft.rfft(ir, size)
    octaves = np.abs(np.log2(np.maximum(np.fft.rfftfreq(size, 1 / projects.RATE), 1e-9) / hz))
    edge = np.clip((octaves - 0.25) / 0.5, 0, 1)
    return np.fft.irfft(spectrum * np.cos(edge * math.pi / 2) ** 2, size)[:, :ir.shape[1]]


def decay_curve(ir):
    """The Schroeder energy decay curve in dB, both channels."""
    energy = np.cumsum((ir ** 2).sum(axis=0)[::-1])[::-1]
    return 10 * np.log10(np.maximum(energy / energy[0], 1e-30))


def decay_time(curve, top, bottom):
    """Seconds to fall 60 dB, from a straight line fitted to the curve between `top` and `bottom` dB."""
    start, end = np.argmax(curve <= top), np.argmax(curve <= bottom)
    if end <= start:
        return float("nan")
    times = np.arange(start, end) / projects.RATE
    return -60 / np.polyfit(times, curve[start:end], 1)[0]


def echo_density(signal, window_ms=20, hop_ms=1, until_ms=500):
    """Normalized echo density profile (Abel and Huang 2006): the part of a Hann window of samples
    further from 0 than its standard deviation, over that of a Gaussian noise. 1 is diffuse."""
    width, hop = projects.RATE * window_ms // 1000, projects.RATE * hop_ms // 1000
    weights = np.hanning(width)
    weights /= weights.sum()
    windows = np.lib.stride_tricks.sliding_window_view(signal[:projects.RATE * until_ms // 1000 + width], width)[::hop]
    deviation = np.sqrt((windows ** 2 * weights).sum(axis=1, keepdims=True))
    return (weights * (np.abs(windows) > deviation)).sum(axis=1) / math.erfc(1 / math.sqrt(2))


def ringing(ir, curve):
    """The highest narrow peak of the tail's spectrum over the median of the third of an octave
    around it, in dB, and its frequency. Each piece of the tail counts alike, loud or late."""
    start = projects.RATE // 20
    end = max(int(np.argmax(curve <= -60)) or ir.shape[1], start + projects.RATE * 3 // 10)
    size = 4096
    tail = ir[:, start:end]
    pieces = [tail[:, at:at + size] * np.hanning(size) for at in range(0, tail.shape[1] - size, size // 2)]
    power = sum(spectrum / spectrum.sum() for spectrum in
                ((np.abs(np.fft.rfft(piece)) ** 2).sum(axis=0) for piece in pieces))
    db = 10 * np.log10(power + 1e-30)
    frequencies = np.fft.rfftfreq(size, 1 / projects.RATE)
    best = (-math.inf, 0.0)
    for index in np.nonzero((frequencies >= 100) & (frequencies <= 12_000))[0]:
        near = (frequencies >= frequencies[index] * 2 ** (-1 / 6)) & (frequencies <= frequencies[index] * 2 ** (1 / 6))
        best = max(best, (db[index] - np.median(db[near]), frequencies[index]))
    return best


def db(value):
    return 20 * math.log10(value) if value > 0 else -math.inf


def measure(path):
    ir = channels(path)[:, int(projects.CLICK_SECONDS * projects.RATE):]
    bands = []
    for hz in BANDS:
        part = band(ir, hz)
        curve = decay_curve(part)
        bands.append({"t30": decay_time(curve, -5, -35), "edt": decay_time(curve, 0, -10),
                      "energy": 10 * math.log10((part ** 2).sum()),
                      "correlation": float(np.corrcoef(part)[0, 1])})
    density = echo_density(ir[0])
    diffuse = np.nonzero(density >= 0.95)[0]
    return {"bands": bands, "mixing_ms": float(diffuse[0]) if len(diffuse) else math.inf,
            "density": [float(density[ms]) for ms in (20, 50, 100, 200)],
            "peak": db(np.abs(ir).max()), "rms": db(math.sqrt((ir[:, :projects.RATE] ** 2).mean())),
            "correlation": float(np.corrcoef(ir)[0, 1]), "ringing": ringing(ir, decay_curve(ir))}


def compare(name, a, b, null_db):
    """Prints both and returns what fails the limits."""
    print(f"\n{name}: A then B. Null {null_db:.1f} dB. Mixing time (echo density 0.95): "
          f"{a['mixing_ms']:.0f} / {b['mixing_ms']:.0f} ms. Echo density at 20, 50, 100, 200 ms: "
          f"{' '.join(f'{x:.2f}' for x in a['density'])} / {' '.join(f'{x:.2f}' for x in b['density'])}. "
          f"Peak {a['peak']:.1f} / {b['peak']:.1f} dBFS, RMS of 1 s {a['rms']:.1f} / {b['rms']:.1f} dBFS, "
          f"correlation {a['correlation']:.2f} / {b['correlation']:.2f}. Highest tail peak "
          f"{a['ringing'][0]:.1f} dB at {a['ringing'][1]:.0f} Hz / {b['ringing'][0]:.1f} dB at {b['ringing'][1]:.0f} Hz.\n")
    print("| band Hz | T30 s | EDT s | energy dB | correlation |\n| ---: | --- | --- | --- | --- |")
    failures = []
    for hz, x, y in zip(BANDS, a["bands"], b["bands"]):
        print(f"| {hz} | {x['t30']:.2f} / {y['t30']:.2f} | {x['edt']:.2f} / {y['edt']:.2f} "
              f"| {x['energy']:.1f} / {y['energy']:.1f} | {x['correlation']:.2f} / {y['correlation']:.2f} |")
        if not abs(y["t30"] / x["t30"] - 1) <= RT_PART:
            failures.append(f"{name} {hz} Hz: T30 {x['t30']:.2f} -> {y['t30']:.2f} s")
        if not abs(y["energy"] - x["energy"]) <= ENERGY_DB:
            failures.append(f"{name} {hz} Hz: energy {x['energy']:.1f} -> {y['energy']:.1f} dB")
        if not abs(y["correlation"] - x["correlation"]) <= CORRELATION:
            failures.append(f"{name} {hz} Hz: correlation {x['correlation']:.2f} -> {y['correlation']:.2f}")
    if not b["mixing_ms"] <= a["mixing_ms"] + MIXING_MS:
        failures.append(f"{name}: mixing time {a['mixing_ms']:.0f} -> {b['mixing_ms']:.0f} ms")
    if not b["ringing"][0] <= a["ringing"][0] + RINGING_DB:
        failures.append(f"{name}: tail peak {a['ringing'][0]:.1f} -> {b['ringing'][0]:.1f} dB")
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("refs", nargs=2, help="A and B, such as main HEAD")
    arguments = parser.parse_args()
    (bench.ROOT / "home").mkdir(parents=True, exist_ok=True)
    builds = [bench.Build(ref) for ref in arguments.refs]
    folder = bench.ROOT / "reverb" / "-".join(build.sha[:7] for build in builds)
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    failures = []
    for name in projects.reverb_names():
        seconds = IR_SECONDS if name.startswith("rv-ir-") else MUSIC_SECONDS
        paths = []
        for label, build in zip("ab", builds):
            build.make_projects([name])
            paths.append(folder / f"{name}-{label}-{build.sha[:7]}.wav")
            bench.render_once(build, name, seconds, paths[-1])
        a, b = map(channels, paths)
        null_db = db(np.abs(a - b).max())
        if name.startswith("rv-ir-"):
            failures += compare(name, measure(paths[0]), measure(paths[1]), null_db)
        else:
            print(f"\n{name}: null {null_db:.1f} dB. Peak {db(np.abs(a).max()):.1f} / {db(np.abs(b).max()):.1f} dBFS, "
                  f"RMS {db(math.sqrt((a ** 2).mean())):.1f} / {db(math.sqrt((b ** 2).mean())):.1f} dBFS")
    print(f"\nA {builds[0]}, B {builds[1]}. WAVs to listen to: {folder}")
    print("\n".join(["\nFails:", *failures]) if failures else "\nPasses every limit.")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
