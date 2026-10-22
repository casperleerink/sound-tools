# Analyzer

`analyzer` is an effect that leaves the sound as it is and shows the composer its spectrum, level, loudness and pitch on its card. It has nothing to set.

Here the bass shows its sound through an analyzer named `analyzer`:

```json state/song/bass/chain/analyzer.json
{
  "tool": "analyzer",
  "state": {}
}
```

To measure a sound yourself, run `sound-tools <project-folder> --analyze`, which measures the same way.
