# Analyzer: the built-in analyzer

`analyzer` is an effect that leaves the sound as it is, to the bit, and shows the composer what it hears on its card: the spectrum, the peak level, the loudness of the last 400 ms in LUFS, and the note the sound plays with how far off it is in cents. Put one on a track when the composer asks to see a sound or to tune it.

Here the bass shows its sound through an analyzer named `analyzer`:

```json state/arrangement/bass/analyzer.json
{
  "tool": "analyzer",
  "state": {}
}
```

It has nothing to set. Its card is for the composer: to measure a sound yourself, run `sound-tools <project-folder> --analyze`, which measures the same way.
