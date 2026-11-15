--!strict
-- A vertical level meter: a well that a bar fills from the bottom as `level` moves.
local S = require("../sdk")

export type MeterProps = { key: string, level: string, w: number, h: number, color: S.Color }

return function(p: MeterProps): S.Node
	return S.box({ w = p.w, h = p.h, radius = p.w / 2, fill = "well", clip = true }, {
		S.live({ key = p.key, level = p.level, w = p.w, h = p.h, fill = p.color }),
	})
end
