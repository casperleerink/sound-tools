# Tone: a sine oscillator

`tone` plays one steady sine. It is not part of the arrangement and ignores the transport.

```json state/drone.json
{
  "tool": "tone",
  "state": {"frequency_hz": 220.0, "gain": 0.2}
}
```

- `frequency_hz`: 1 to 20000. `gain`: linear, 0 to 1.
- It has one port, the mono output `audio`. It is silent until `project.json` connects it, once per device channel: `{"from": {"instance": "drone", "port": "audio"}, "to": {"device_output": 0}}`.
