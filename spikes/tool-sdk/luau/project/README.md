# Writing the tool

The window runs `init.luau` (Luau, `--!strict`) inside itself and shows what `render` returns.
Save any `.luau` file and it reloads in a moment, keeping the records. A render error shows an
error card; code that runs past 250 ms is stopped. Errors and `print` go to the window's stderr.

## Shape

```luau
local S = require("./sdk")
local function render(view)
	return S.col({ pad = S.theme.space[4], gap = S.theme.space[2], fill = "bg" }, {
		S.text("Hello", S.theme.text.lg, { color = "text" }),
		view:Memo("list", S.version("tracks"), function() return list() end),
	})
end
return S.tool(render)
```

- `render` runs again after every change. Wrap parts in `view:Memo(key, version, build)`:
  `build` runs only when `version` moves. `S.version(path)` moves when anything under `path` does.
- Layout: `S.row`, `S.col`, `S.box`, `S.text`, `S.spacer`, `S.lines`. Props are in `sdk.luau`
  (`Props`). `pin = { left, top }` places a node inside its parent, out of the flow.
- Look: colours by name (`fill = "surface"`), sizes from `S.theme` (`text`, `space`, `radius`),
  `shadow = "sm"`. All come from `theme.json`, so leave raw numbers for layout only.
- Components are plain functions in `ui/`: read them, change them, add your own.

## Records

`S.state` holds the records. Never write to it: `S.change("tracks/2/solo", true)` asks the
window, and the value comes back into `S.state` (paths count from 1). Element keys
(`solo-1`) count from 0.

## What moves every frame

Never from your code. Bind it: `S.live({ key, x = "tick", scale, add })` moves a node,
`S.live({ key, level = "track/3", w, h })` fills a meter, `S.liveText({ key, text = "position" })`
shows the time, `S.drag({ key, path, value, min, max, length })` drags a fader thumb and sets
`path` on release.

## Check your work

1. `luau-lsp analyze --platform=standard .` checks types: colour and style names, theme keys,
   fields. A misspelled prop name is not a type error: the window logs `unknown prop`.
2. `<window> --snap out.png --at 3 --scenario play` draws it; `--tree out.json` lists every
   element with its key, place, text and colour.
