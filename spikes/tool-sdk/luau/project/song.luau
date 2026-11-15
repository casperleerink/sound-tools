--!strict
-- The song page: a transport, a ruler, a row per track (its header and its clips in time), the
-- playhead, and a mixer with a strip per track and the master.
local S = require("./sdk")
local Button = require("./ui/button")
local Toggle = require("./ui/toggle")
local Fader = require("./ui/fader")
local Meter = require("./ui/meter")

type Note = { start: number, length: number, pitch: number, velocity: number }
type Clip = { name: string, start: number, length: number, notes: { Note } }
type Track = { name: string, colour: number, mute: boolean, solo: boolean, armed: boolean, gain_db: number, clips: { Clip } }
type Song = {
	transport: { playing: boolean },
	-- Points per tick, and which page of the timeline is in view (its scroll / PAGE).
	view: { zoom: number, page: number },
	tracks: { Track },
	master: { gain_db: number },
}

local BAR, BEAT = 3840, 960
local ROW, HEADER, LEAD, RULER, TRANSPORT, MIXER, STRIP = 64, 176, 8, 28, 44, 232, 72
local WINDOW_H = 900
local LOW, HIGH = 48, 84
local PAGE = 1200
local WINDOW_W = 1440
local METER = 140

local function song(): Song
	return S.state :: Song
end

local function tint(colour: number, alpha: number): S.Color
	return S.hex(string.format("#%06x%02x", colour, math.floor(alpha * 255)))
end

-- The ticks the built part of the timeline spans: the page in view and one on each side.
local function built(): (number, number)
	local view = song().view
	local left = math.max(view.page * PAGE - PAGE, 0)
	local right = view.page * PAGE + WINDOW_W + PAGE
	local function tick(x: number): number
		return math.max(x - LEAD, 0) / view.zoom
	end
	return math.floor(tick(left) / BAR) * BAR, math.ceil(tick(right) / BAR) * BAR
end

local function transport(): S.Node
	local playing = song().transport.playing
	return S.row({ w = "full", h = TRANSPORT, px = S.theme.space[3], gap = S.theme.space[4], align = "center", fill = "surface" }, {
		Button({
			key = "play",
			label = if playing then "Stop" else "Play",
			variant = if playing then "primary" else "secondary",
			onPress = function()
				S.change("transport/playing", not playing)
			end,
		}),
		S.liveText({ key = "time", text = "position", style = "mono", color = "text", w = 90 }),
		S.text("120 bpm", S.theme.text.sm, { color = "text-muted" }),
	})
end

local function ruler(view: any): S.Node
	local from, to = built()
	local zoom = song().view.zoom
	local marks = view:Memo("ruler", `{zoom}:{from}:{to}`, function()
		local points = {}
		local labels: { S.Child } = {}
		for tick = from, to, BEAT do
			local bar = tick % BAR == 0
			table.insert(points, tick)
			table.insert(points, if bar then 8 else 18)
			table.insert(points, tick)
			table.insert(points, RULER)
			if bar then
				local x = LEAD + tick * zoom
				table.insert(labels, S.text(`{tick // BAR + 1}`, S.theme.text.xs, { color = "text-muted", pin = { left = x + 4, top = 2 } }))
			end
		end
		local width = LEAD + to * zoom
		local children: { S.Child } = { S.lines({ points = points, scaleX = zoom, width = 1, color = "border", w = width, h = RULER, pin = { left = LEAD, top = 0 } }) }
		for _, label in labels do
			table.insert(children, label)
		end
		return S.box({ w = width, h = RULER }, children)
	end)
	return S.row({ w = "full", h = RULER, fill = "surface" }, {
		S.box({ w = HEADER, h = RULER }),
		S.box({ grow = 1, h = RULER, clip = true }, {
			S.live({ key = "ruler-scroll", x = "scroll", pin = { left = 0, top = 0 } }, { marks }),
		}),
	})
end

local function header(i: number, track: Track): S.Node
	local path = `tracks/{i}`
	-- Element keys count from 0.
	local n = i - 1
	return S.row({ w = HEADER, h = ROW, fill = "surface" }, {
		S.box({ w = 4, h = ROW, fill = tint(track.colour, 1) }),
		S.col({ grow = 1, pad = S.theme.space[2], gap = S.theme.space[2] }, {
			S.text(track.name, S.theme.text.sm, { color = "text", truncate = true }),
			S.row({ gap = S.theme.space[1] }, {
				Toggle({ key = `mute-{n}`, label = "M", on = track.mute, color = "orange", onPress = function()
					S.change(`{path}/mute`, not track.mute)
				end }),
				Toggle({ key = `solo-{n}`, label = "S", on = track.solo, color = "green", onPress = function()
					S.change(`{path}/solo`, not track.solo)
				end }),
				Toggle({ key = `arm-{n}`, label = "R", on = track.armed, color = "red", onPress = function()
					S.change(`{path}/armed`, not track.armed)
				end }),
			}),
		}),
		S.box({ pad = S.theme.space[1] }, {
			Meter({ key = `meter-head-{n}`, level = `track/{i}`, w = 4, h = ROW - 8, color = tint(track.colour, 1) }),
		}),
	})
end

-- Each clip's notes as lines, in ticks across and points down; kept while the clip is the same.
local shapes: { [Clip]: { number } } = setmetatable({}, { __mode = "k" }) :: any
local function noteLines(clip: Clip, h: number): { number }
	local cached = shapes[clip]
	if cached then
		return cached
	end
	local points = table.create(#clip.notes * 4)
	for _, note in clip.notes do
		local y = 16 + (1 - (note.pitch - LOW) / (HIGH - LOW)) * (h - 20)
		table.insert(points, note.start)
		table.insert(points, y)
		table.insert(points, note.start + note.length)
		table.insert(points, y)
	end
	shapes[clip] = points
	return points
end

local function lane(track: Track, from: number, to: number): S.Node
	local zoom = song().view.zoom
	local width = LEAD + to * zoom
	local bars = {}
	for tick = from, to, BAR do
		table.insert(bars, tick)
		table.insert(bars, 0)
		table.insert(bars, tick)
		table.insert(bars, ROW)
	end
	local children: { S.Child } = {
		S.lines({ points = bars, scaleX = zoom, width = 1, color = "border", w = width, h = ROW, pin = { left = LEAD, top = 0 } }),
	}
	local h = ROW - 6
	for _, clip in track.clips do
		if clip.start + clip.length >= from and clip.start <= to then
			table.insert(children, S.box({
				pin = { left = LEAD + clip.start * zoom, top = 3 },
				w = clip.length * zoom,
				h = h,
				radius = S.theme.radius.sm,
				fill = tint(track.colour, 0.18),
				border = tint(track.colour, 0.8),
				borderWidth = 1,
				clip = true,
			}, {
				S.text(clip.name, S.theme.text.xs, { color = "text", pin = { left = 4, top = 2 } }),
				S.lines({ points = noteLines(clip, h), scaleX = zoom, width = 2, color = tint(track.colour, 0.9), w = clip.length * zoom, h = h, pin = { left = 0, top = 0 } }),
			}))
		end
	end
	return S.box({ w = width, h = ROW }, children)
end

local function trackRow(view: any, i: number, track: Track): S.Node
	local from, to = built()
	local zoom = song().view.zoom
	return S.row({ w = "full", h = ROW }, {
		view:Memo(`head-{i}`, S.version(`tracks/{i}`), function()
			return header(i, track)
		end),
		S.box({ grow = 1, h = ROW, clip = true }, {
			S.live({ key = `lane-scroll-{i}`, x = "scroll", pin = { left = 0, top = 0 } }, {
				view:Memo(`lane-{i}`, `{S.version(`tracks/{i}/clips`)}:{zoom}:{from}:{to}`, function()
					return lane(track, from, to)
				end),
			}),
		}),
	})
end

local function strip(i: number, name: string, path: string, gain: number, colour: S.Color): S.Node
	local n = i - 1
	return S.col({ key = `strip-{n}`, w = STRIP, h = "full", py = S.theme.space[3], gap = S.theme.space[3], align = "center" }, {
		S.text(name, S.theme.text.sm, { color = "text", truncate = true }),
		S.row({ gap = 6 }, {
			Fader({ key = `fader-{n}`, path = `{path}/gain_db`, value = gain, min = -60, max = 6, h = METER }),
			Meter({ key = `meter-l-{n}`, level = `track/{i}`, w = 4, h = METER, color = colour }),
			Meter({ key = `meter-r-{n}`, level = `track/{i}/right`, w = 4, h = METER, color = colour }),
		}),
		S.text(string.format("%.1f dB", if math.abs(gain) < 0.05 then 0 else gain), S.theme.text.xs, { color = "text-muted" }),
	})
end

local function mixer(view: any): S.Node
	local tracks = song().tracks
	local strips: { S.Child } = {}
	for i, track in tracks do
		table.insert(strips, view:Memo(`strip-{i}`, S.version(`tracks/{i}`), function()
			return strip(i, track.name, `tracks/{i}`, track.gain_db, tint(track.colour, 1))
		end))
	end
	local master = song().master
	table.insert(strips, strip(#tracks + 1, "Master", "master", master.gain_db, "accent"))
	return S.row({ w = "full", h = MIXER, fill = "surface", px = S.theme.space[2], gap = S.theme.space[1] }, strips)
end

local function render(view: any): S.Node
	local rows: { S.Child } = {}
	for i, track in song().tracks do
		table.insert(rows, trackRow(view, i, track))
	end
	local zoom = song().view.zoom
	return S.box({ w = "full", h = "full", fill = "bg" }, {
		S.col({ w = "full", h = "full" }, {
			view:Memo("transport", S.version("transport"), transport),
			ruler(view),
			S.col({ w = "full", h = WINDOW_H - TRANSPORT - RULER - MIXER, scrollY = true }, rows),
			mixer(view),
		}),
		-- The playhead, over the ruler and the tracks.
		S.box({ pin = { left = HEADER, top = TRANSPORT, right = 0, bottom = MIXER }, clip = true }, {
			S.live({ key = "playhead-scroll", x = "scroll", pin = { left = 0, top = 0, bottom = 0 } }, {
				S.live({ key = "playhead", x = "tick", scale = zoom, add = LEAD, w = 1, pin = { left = 0, top = 0, bottom = 0 }, fill = "accent" }),
			}),
		}),
	})
end

return S.tool(render)
