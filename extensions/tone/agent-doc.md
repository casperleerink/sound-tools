# Tone

`tone` plays one steady sine. It ignores the transport.

```json state/drone.json
{
  "tool": "tone",
  "state": {"frequency_hz": 220.0, "gain": 0.2}
}
```

- `frequency_hz`: 1 to 20000. `gain`: linear, 0 to 1.
- Its one port is the stereo output `audio`. It is silent until `project.json` connects it: `{"from": {"instance": "drone", "port": "audio"}, "to": {"device_output": 0}}`. The number is the left device channel; the right one is the next.
