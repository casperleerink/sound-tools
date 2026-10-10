"""How much of its page a page uses: the box around what it draws, as a share of the page.

    python3 -I tooling/agent-eval/fill.py <window.png>...

Takes the `window.png` the screenshot example writes. The page is the window under its title
row; its own colour is the colour at its top left, and what differs from it is drawn.
"""

import sys

from PIL import Image

# The title row, in points, and the scale of the screenshots.
TITLE = 48
SCALE = 2


def fill(path: str) -> float:
    image = Image.open(path).convert("RGB")
    width, height = image.size
    page = image.crop((0, TITLE * SCALE, width, height))
    background = page.getpixel((4, 4))
    # Pixels that differ from the page colour by more than a little, as a mask.
    mask = page.point(lambda value: 0).convert("L")
    pixels = page.load()
    marks = mask.load()
    for y in range(0, page.height, 2):
        for x in range(0, page.width, 2):
            if sum(abs(a - b) for a, b in zip(pixels[x, y], background)) > 24:
                marks[x, y] = 255
    box = mask.getbbox()
    if not box:
        return 0.0
    left, top, right, bottom = box
    return (right - left) * (bottom - top) / (page.width * page.height)


if __name__ == "__main__":
    for path in sys.argv[1:]:
        print(f"{fill(path):.2f} {path}")
