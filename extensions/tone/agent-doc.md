# Tone: a sine oscillator

`tone` plays one steady sine. It is not part of the arrangement and ignores the transport.

```json state/drone.json
{
  "tool": "tone",
  "state": {"frequency_hz": 220.0, "gain": 0.2}
}
```

- `frequency_hz`: 1 to 20000. `gain`: linear, 0 to 1.
- It has one port, the stereo output `audio`. It is silent until `project.json` connects it: `{"from": {"instance": "drone", "port": "audio"}, "to": {"device_output": 0}}`. The number is the first device channel; the right channel goes to the one after it.
