from PIL import Image, ImageDraw, ImageFilter

S = 1024
CX0, CY0, CX1, CY1 = 112, 96, 912, 928
RADIUS = 112

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

# 1) 柔和投影
shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(shadow).rounded_rectangle(
    [CX0 + 18, CY0 + 54, CX1 + 6, CY1 + 34], radius=RADIUS, fill=(92, 62, 0, 120)
)
img = Image.alpha_composite(img, shadow.filter(ImageFilter.GaussianBlur(30)))

# 2) 便签纸主体 + 竖向渐变
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle([CX0, CY0, CX1, CY1], radius=RADIUS, fill=255)

grad = Image.new("RGBA", (S, S), (0, 0, 0, 0))
gd = ImageDraw.Draw(grad)
for y in range(CY0, CY1 + 1):
    t = (y - CY0) / (CY1 - CY0)
    r = round(255 - 10 * t)
    g = round(238 - 30 * t)
    b = round(150 - 40 * t)
    gd.line([(0, y), (S, y)], fill=(r, g, b, 255))
img.paste(grad, (0, 0), mask)

# 3) 右下角折角
fold = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(fold).polygon(
    [(CX1 - 210, CY1), (CX1, CY1 - 210), (CX1, CY1)], fill=(236, 190, 84, 255)
)
fold = Image.composite(fold, Image.new("RGBA", (S, S), (0, 0, 0, 0)), mask)
img = Image.alpha_composite(img, fold)
ImageDraw.Draw(img).line(
    [(CX1 - 210, CY1), (CX1, CY1 - 210)], fill=(214, 166, 62, 255), width=6
)

# 4) 文本行
lines = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ld = ImageDraw.Draw(lines)
for i, (left, width) in enumerate([(236, 548), (236, 470), (236, 372)]):
    y = 356 + i * 132
    ld.rounded_rectangle([left, y, left + width, y + 44], radius=22, fill=(126, 94, 26, 82))
lines = Image.composite(lines, Image.new("RGBA", (S, S), (0, 0, 0, 0)), mask)
img = Image.alpha_composite(img, lines)

img.save(r"D:\ai\rust-sticky-notes\src-tauri\icons\source.png")
print("wrote source.png", img.size)
