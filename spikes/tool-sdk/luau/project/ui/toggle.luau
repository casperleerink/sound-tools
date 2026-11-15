--!strict
-- A small square button with a letter, lit in `color` while on.
local S = require("../sdk")

export type ToggleProps = { key: string, label: string, on: boolean, color: S.Color, onPress: (S.Event) -> () }

return function(p: ToggleProps): S.Node
	return S.box({
		key = p.key,
		role = "button",
		label = p.label,
		w = 20,
		h = 18,
		radius = S.theme.radius.sm,
		center = true,
		fill = if p.on then p.color else "border",
		hover = if p.on then nil else "surface-raised",
		onPress = p.onPress,
	}, {
		S.text(p.label, S.theme.text.xs, { color = if p.on then "on-accent" else "text-muted" }),
	})
end
