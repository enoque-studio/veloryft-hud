#!/usr/bin/env python3
"""Gera os assets de marca do instalador a partir das artes em docs/.

Saídas (installer/assets/):
  veloryft.ico          — ícone do app (símbolo branco+azul em tile escuro)
  wizard-large.bmp      — banner lateral do wizard (164x314)
  wizard-large@2x.bmp   — idem em 2x (328x628) p/ telas HiDPI
  wizard-small.bmp      — banner de topo (55x58)
  wizard-small@2x.bmp   — idem em 2x (110x116)
"""
from PIL import Image, ImageDraw, ImageFilter, ImageFont
import os

HERE = os.path.dirname(os.path.abspath(__file__))
DOCS = os.path.normpath(os.path.join(HERE, "..", "..", "docs"))

BLUE = (24, 112, 255)          # acento da marca, levemente vibrante p/ fundo escuro
BG_TOP = (12, 18, 30)
BG_BOT = (4, 6, 11)

# --- helpers ---------------------------------------------------------------

def keyed(src, blue=BLUE, white=(255, 255, 255)):
    """Tira o fundo branco por luminância: alpha = 255 - min(canal).
    Pixel azulado -> cor de acento; o resto -> branco. Dá anti-alias limpo
    tanto contra preto quanto contra o azul, sem halo branco."""
    src = src.convert("RGB")
    px = src.load()
    w, h = src.size
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    op = out.load()
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y]
            a = 255 - min(r, g, b)
            if a <= 6:
                continue
            if b > r + 35 and b > g + 25 and b > 110:
                op[x, y] = (blue[0], blue[1], blue[2], a)
            else:
                op[x, y] = (white[0], white[1], white[2], a)
    return out


def vgradient(size, top, bot):
    w, h = size
    g = Image.new("RGB", (w, h), top)
    d = ImageDraw.Draw(g)
    for y in range(h):
        t = y / max(1, h - 1)
        d.line([(0, y), (w, y)],
               fill=tuple(round(top[i] + (bot[i] - top[i]) * t) for i in range(3)))
    return g


def blue_glow(size, center, radius, strength=90):
    w, h = size
    glow = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(glow)
    cx, cy = center
    d.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], fill=strength)
    glow = glow.filter(ImageFilter.GaussianBlur(radius * 0.55))
    layer = Image.new("RGB", (w, h), BLUE)
    return glow, layer


def rounded_mask(size, radius):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius, fill=255)
    return m


def load_mark():
    """Símbolo 'V' recolorido (branco+azul), recortado no conteúdo."""
    mark = keyed(Image.open(os.path.join(DOCS, "veloryft_logo.png")))
    return mark.crop(mark.getbbox())


def load_wordmark():
    """Wordmark 'Veloryft' (só o texto, sem o símbolo), branco, recortado."""
    full = keyed(Image.open(os.path.join(DOCS, "veloryft_mono.png")))
    bbox = full.getbbox()
    full = full.crop(bbox)
    w, h = full.size
    # coluna de menor 'tinta' na metade esquerda = fenda entre símbolo e texto
    a = full.getchannel("A")
    col = [sum(a.crop((x, 0, x + 1, h)).getdata()) for x in range(w)]
    lo, hi = int(w * 0.18), int(w * 0.42)
    gap = min(range(lo, hi), key=lambda x: col[x])
    word = full.crop((gap, 0, w, h))
    return word.crop(word.getbbox())


def fit(img, box_w, box_h):
    w, h = img.size
    s = min(box_w / w, box_h / h)
    return img.resize((max(1, round(w * s)), max(1, round(h * s))), Image.LANCZOS)


def font(name, size):
    return ImageFont.truetype(f"C:/Windows/Fonts/{name}", size)

# --- ícone -----------------------------------------------------------------

def make_icon(mark):
    S = 1024
    tile = vgradient((S, S), BG_TOP, BG_BOT)
    glow, layer = blue_glow((S, S), (S // 2, int(S * 0.46)), int(S * 0.34), 120)
    tile = Image.composite(layer, tile, glow)
    tile = tile.convert("RGBA")

    m = fit(mark, int(S * 0.60), int(S * 0.60))
    mx = (S - m.size[0]) // 2
    my = int(S * 0.46) - m.size[1] // 2
    # sombra sutil sob o símbolo
    sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sh.paste((0, 0, 0, 160), (mx, my + int(S * 0.02)), m)
    sh = sh.filter(ImageFilter.GaussianBlur(14))
    tile = Image.alpha_composite(tile, sh)
    tile.paste(m, (mx, my), m)

    tile.putalpha(rounded_mask((S, S), int(S * 0.18)))
    out = os.path.join(HERE, "veloryft.ico")
    tile.save(out, sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)])
    print("ok:", out)
    tile.resize((256, 256), Image.LANCZOS).save(os.path.join(HERE, "veloryft-256.png"))

# --- banners do wizard -----------------------------------------------------

def make_wizard_large(mark, word, scale):
    W, H = 164 * scale, 314 * scale
    img = vgradient((W, H), BG_TOP, BG_BOT).convert("RGBA")

    # faixa diagonal azul bem sutil (movimento)
    streak = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    ds = ImageDraw.Draw(streak)
    ds.polygon([(0, int(H * 0.30)), (W, int(H * 0.10)), (W, int(H * 0.16)), (0, int(H * 0.38))],
               fill=(BLUE[0], BLUE[1], BLUE[2], 26))
    img = Image.alpha_composite(img, streak.filter(ImageFilter.GaussianBlur(2 * scale)))

    glow, layer = blue_glow((W, H), (W // 2, int(H * 0.30)), int(W * 0.55), 70)
    img = Image.alpha_composite(img, Image.composite(layer.convert("RGBA"),
                                                     Image.new("RGBA", (W, H), (0, 0, 0, 0)), glow))

    m = fit(mark, int(W * 0.60), int(H * 0.30))
    img.paste(m, ((W - m.size[0]) // 2, int(H * 0.16)), m)

    wm = fit(word, int(W * 0.74), int(H * 0.12))
    img.paste(wm, ((W - wm.size[0]) // 2, int(H * 0.52)), wm)

    d = ImageDraw.Draw(img)
    f_sub = font("bahnschrift.ttf", 11 * scale)
    sub = "TELEMETRY OVERLAY"
    tw = d.textlength(sub, font=f_sub)
    d.text(((W - tw) / 2, int(H * 0.63)), sub, font=f_sub, fill=(150, 165, 190))

    # tag BETA
    f_beta = font("bahnschrift.ttf", 12 * scale)
    label = "BETA"
    lw = d.textlength(label, font=f_beta)
    pad = 8 * scale
    bx0 = (W - (lw + pad * 2)) / 2
    by0 = int(H * 0.70)
    d.rounded_rectangle([bx0, by0, bx0 + lw + pad * 2, by0 + 18 * scale],
                        radius=9 * scale, fill=BLUE)
    d.text((bx0 + pad, by0 + 3 * scale), label, font=f_beta, fill=(255, 255, 255))

    # rodapé
    f_foot = font("segoeui.ttf", 9 * scale)
    foot = "veloryft.com"
    fw = d.textlength(foot, font=f_foot)
    d.text(((W - fw) / 2, H - 22 * scale), foot, font=f_foot, fill=(90, 105, 130))

    suffix = "" if scale == 1 else "@2x"
    out = os.path.join(HERE, f"wizard-large{suffix}.bmp")
    img.convert("RGB").save(out)
    print("ok:", out)


def make_wizard_small(mark, scale):
    W, H = 55 * scale, 58 * scale
    img = vgradient((W, H), BG_TOP, BG_BOT).convert("RGBA")
    glow, layer = blue_glow((W, H), (W // 2, H // 2), int(W * 0.7), 60)
    img = Image.alpha_composite(img, Image.composite(layer.convert("RGBA"),
                                                     Image.new("RGBA", (W, H), (0, 0, 0, 0)), glow))
    m = fit(mark, int(W * 0.72), int(H * 0.72))
    img.paste(m, ((W - m.size[0]) // 2, (H - m.size[1]) // 2), m)
    suffix = "" if scale == 1 else "@2x"
    out = os.path.join(HERE, f"wizard-small{suffix}.bmp")
    img.convert("RGB").save(out)
    print("ok:", out)


if __name__ == "__main__":
    mark = load_mark()
    word = load_wordmark()
    make_icon(mark)
    for s in (1, 2):
        make_wizard_large(mark, word, s)
        make_wizard_small(mark, s)
    print("done")
