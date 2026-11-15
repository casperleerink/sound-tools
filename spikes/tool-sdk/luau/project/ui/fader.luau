--!strict
-- A vertical fader: a slot and a thumb at `value`, dragged to set the record at `path`.
local S = require("../sdk")

export type FaderProps = { key: string, path: string, value: number, min: number, max: number, h: number }

local THUMB = 12

return function(p: FaderProps): S.Node
	local travel = p.h - THUMB
	local fraction = math.clamp((p.value - p.min) / (p.max - p.min), 0, 1)
	return S.box({ key = p.key, w = 24, h = p.h }, {
		S.box({ pin = { left = 10, top = 0 }, w = 4, h = p.h, radius = S.theme.radius.full, fill = "well" }),
		S.drag({
			key = `{p.key}-thumb`,
			path = p.path,
			value = p.value,
			min = p.min,
			max = p.max,
			length = travel,
			pin = { left = 0, top = (1 - fraction) * travel },
			w = 24,
			h = THUMB,
			radius = S.theme.radius.sm,
			fill = "text",
			shadow = "sm",
		}),
	})
end
