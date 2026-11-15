--!strict
-- The SDK a tool is written with. Do not edit: the window owns it.
local ui = require("@rune/ui")
local host = require("@app/host")

-- What a ui function returns. Opaque: put it in a children list, nothing else.
export type Node = { __node: never }
export type Child = Node | string | false | nil

-- Colours of theme.json, by name. `hex("#rrggbbaa")` gives any other colour.
export type Color =
	"bg" | "surface" | "surface-raised" | "well" | "border" | "text" | "text-muted" | "accent" | "on-accent"
	| "red" | "orange" | "yellow" | "green" | "teal" | "blue" | "purple" | "pink"
	| { __hex: never }

export type Event = { kind: string, key: string?, shift: boolean?, alt: boolean?, mod: boolean? }

export type Props = {
	key: string?,
	w: (number | "full")?, h: (number | "full")?,
	minW: number?, minH: number?, maxW: number?, maxH: number?,
	grow: number?, shrink: number?,
	pad: number?, px: number?, py: number?, pt: number?, pr: number?, pb: number?, pl: number?,
	gap: number?,
	align: ("start" | "center" | "end" | "stretch")?,
	justify: ("start" | "center" | "end" | "between")?,
	center: boolean?,
	fill: Color?, hover: Color?, press: Color?,
	border: Color?, borderWidth: number?, radius: number?,
	shadow: ("sm" | "md")?,
	clip: boolean?, scrollX: boolean?, scrollY: boolean?, opacity: number?,
	-- Placed inside its parent at these distances from its edges, out of the flow.
	pin: { left: number?, top: number?, right: number?, bottom: number? }?,
	role: string?, label: string?,
	onPress: ((Event) -> ())?,
	-- Text.
	color: Color?, truncate: boolean?, noWrap: boolean?, textAlign: ("start" | "center" | "end")?,
	-- What `lines`, `live`, `liveText` and `drag` read, below.
	points: { number }?, scaleX: number?, width: number?,
	x: ("tick" | "scroll")?, scale: number?, add: number?, level: string?,
	text: "position"?, style: TextName?,
	path: string?, value: number?, min: number?, max: number?, length: number?,
}

-- A text style of theme.json: `text("Hi", theme.text.sm)`.
export type TextStyle = { size: number, lineHeight: number, tracking: number, weight: number, tabular: boolean? }
export type TextName = "xs" | "sm" | "base" | "lg" | "mono"

type Theme = {
	text: { xs: TextStyle, sm: TextStyle, base: TextStyle, lg: TextStyle, mono: TextStyle },
	-- Spacing steps: space[1] is 4 points, space[2] 8, ...
	space: { [number]: number },
	radius: { sm: number, md: number, lg: number, full: number },
}

local raw = host.theme
local text = {}
for name, style in raw.text do
	text[name] = table.freeze(style)
end
local space = {}
for step, points in raw.space do
	space[tonumber(step) :: number] = points
end
local theme: Theme = table.freeze({
	text = table.freeze(text) :: any,
	space = table.freeze(space),
	radius = table.freeze(raw.radius) :: any,
})

local S = {}
S.theme = theme

-- Layout. Children are laid out in a row, a column, or (box) a column too.
S.row = ui.row :: (Props?, { Child }?) -> Node
S.col = ui.col :: (Props?, { Child }?) -> Node
S.box = ui.box :: (Props?, { Child }?) -> Node
S.spacer = ui.spacer :: () -> Node
S.text = ui.text :: (string, TextStyle?, Props?) -> Node
S.hex = ui.hex :: (string) -> Color

-- Straight lines in your own units: x scaled by `scaleX`, y in points. `points` is
-- {x1, y1, x2, y2, ...}, one line per four numbers. For many small marks, such as notes.
S.lines = ui.lines :: (Props) -> Node

-- Values the window moves every frame with no code running. `key` is required.
-- x: the node moves sideways by `source * scale + add` points: "tick" (where the song plays,
-- in ticks) or "scroll" (the timeline's scroll, in points).
-- level: the node is a bar `w` wide that fills `h` from the bottom: "track/<n>" or
-- "track/<n>/right" (n from 1, the master is tracks + 1), 0 to 1.
S.live = ui.live :: (Props, { Child }?) -> Node
-- Text the window changes every frame: "position" (bar.beat.tick).
S.liveText = ui.liveText :: (Props) -> Node
-- Dragged up and down by the pointer with no code running; on release the window sets
-- `path` to the new value, from `min` (at `length` points down) to `max` (at the top).
S.drag = ui.drag :: (Props, { Child }?) -> Node

-- The records, as the window last sent them. Read them; change them only with `change`.
S.state = {} :: any
-- Asks the window to set the record field at `path` ("tracks/2/solo"). The window decides,
-- and the new value comes back into `S.state`.
S.change = host.change :: (string, any) -> ()

local versions: { [string]: number } = {}
-- A number that moves whenever anything at or under `path` changes: the version of a `Memo`.
function S.version(path: string): number
	return versions[path] or 0
end

local function bump(path: string)
	local at = ""
	versions[at] = (versions[at] or 0) + 1
	for part in string.gmatch(path, "[^/]+") do
		at = if at == "" then part else `{at}/{part}`
		versions[at] = (versions[at] or 0) + 1
	end
end

-- What the window calls. `render(view)` returns the tool's root node; `view:Memo(key, version,
-- build)` reuses a part while its version stays the same.
function S.tool(render: (any) -> Node)
	return {
		render = render,
		load = function(state: any)
			S.state = state
			bump("")
		end,
		set = function(path: string, value: any)
			local parts = string.split(path, "/")
			local at = S.state
			for i = 1, #parts - 1 do
				local part = parts[i]
				at = at[tonumber(part) or part]
			end
			local last = parts[#parts]
			at[tonumber(last) or last] = value
			bump(path)
		end,
		saveHotState = function()
			return { state = S.state, versions = versions }
		end,
		loadHotState = function(saved: any)
			S.state = saved.state
			versions = saved.versions
			bump("")
		end,
	}
end

return S
