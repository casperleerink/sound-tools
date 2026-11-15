--!strict
-- A button with a text label. `variant` "primary" is filled with the accent colour.
local S = require("../sdk")

export type ButtonProps = { key: string, label: string, variant: ("primary" | "secondary")?, onPress: (S.Event) -> () }

return function(p: ButtonProps): S.Node
	local primary = p.variant == "primary"
	return S.box({
		key = p.key,
		role = "button",
		label = p.label,
		h = 28,
		px = S.theme.space[3],
		radius = S.theme.radius.md,
		center = true,
		fill = if primary then "accent" else "surface-raised",
		hover = if primary then nil else "border",
		onPress = p.onPress,
	}, {
		S.text(p.label, S.theme.text.sm, { color = if primary then "on-accent" else "text" }),
	})
end
