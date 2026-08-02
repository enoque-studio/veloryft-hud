#!/usr/bin/env python3
"""Generate the light and dark installer artwork from assets/brand.

Inputs:  assets/brand/veloryft_logo.png, assets/brand/veloryft_mono.png
Outputs: installer/assets/*.bmp, installer/assets/veloryft.ico, veloryft-256.png

Requires Pillow:  pip install pillow
"""
from PIL import Image, ImageDraw, ImageFilter, ImageFont
import os

HERE = os.path.dirname(os.path.abspath(__file__))
REPOSITORY = os.path.normpath(os.path.join(HERE, ".."))
# Brand source artwork (inputs) and generated installer artwork (outputs) are
# kept apart on purpose: everything under OUT is reproducible from BRAND.
BRAND = os.path.join(REPOSITORY, "assets", "brand")
OUT = os.path.join(REPOSITORY, "installer", "assets")

BLUE = (24, 112, 255)          # acento da marca, levemente vibrante p/ fundo escuro
BG_TOP = (12, 18, 30)
BG_BOT = (4, 6, 11)
LIGHT_TOP = (249, 251, 255)
LIGHT_BOT = (225, 236, 252)
INK = (10, 31, 63)

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
    mark = keyed(Image.open(os.path.join(BRAND, "veloryft_logo.png")))
    return mark.crop(mark.getbbox())


def load_wordmark():
    """Wordmark 'Veloryft' (só o texto, sem o símbolo), branco, recortado."""
    full = keyed(Image.open(os.path.join(BRAND, "veloryft_mono.png")))
    bbox = full.getbbox()
    full = full.crop(bbox)
    w, h = full.size
    # coluna de menor 'tinta' na metade esquerda = fenda entre símbolo e texto
    a = full.getchannel("A")
    col = [sum(a.crop((x, 0, x + 1, h)).get_flattened_data()) for x in range(w)]
    lo, hi = int(w * 0.18), int(w * 0.42)
    gap = min(range(lo, hi), key=lambda x: col[x])
    word = full.crop((gap, 0, w, h))
    return word.crop(word.getbbox())


def fit(img, box_w, box_h):
    w, h = img.size
    s = min(box_w / w, box_h / h)
    return img.resize((max(1, round(w * s)), max(1, round(h * s))), Image.LANCZOS)


def tint_for_light_surface(img):
    """Recolor white portions of a branded asset for a light surface."""
    src = img.convert("RGBA")
    out = Image.new("RGBA", src.size, (0, 0, 0, 0))
    sp = src.load()
    op = out.load()
    w, h = src.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = sp[x, y]
            if a == 0:
                continue
            if b > r + 35 and b > g + 25 and b > 110:
                op[x, y] = (BLUE[0], BLUE[1], BLUE[2], a)
            else:
                op[x, y] = (INK[0], INK[1], INK[2], a)
    return out


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
    out = os.path.join(OUT, "veloryft.ico")
    tile.save(out, sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)])
    print("ok:", out)
    tile.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "veloryft-256.png"))

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
    out = os.path.join(OUT, f"wizard-large{suffix}.bmp")
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
    out = os.path.join(OUT, f"wizard-small{suffix}.bmp")
    img.convert("RGB").save(out)
    print("ok:", out)


def make_wizard_large_light(mark, word, scale):
    W, H = 164 * scale, 314 * scale
    img = vgradient((W, H), LIGHT_TOP, LIGHT_BOT).convert("RGBA")

    glow, layer = blue_glow((W, H), (W // 2, int(H * 0.25)), int(W * 0.58), 46)
    img = Image.alpha_composite(
        img,
        Image.composite(layer.convert("RGBA"), Image.new("RGBA", (W, H), (0, 0, 0, 0)), glow),
    )

    m = fit(tint_for_light_surface(mark), int(W * 0.60), int(H * 0.30))
    img.paste(m, ((W - m.size[0]) // 2, int(H * 0.16)), m)

    wm = fit(tint_for_light_surface(word), int(W * 0.74), int(H * 0.12))
    img.paste(wm, ((W - wm.size[0]) // 2, int(H * 0.52)), wm)

    d = ImageDraw.Draw(img)
    f_sub = font("bahnschrift.ttf", 11 * scale)
    sub = "TELEMETRY OVERLAY"
    tw = d.textlength(sub, font=f_sub)
    d.text(((W - tw) / 2, int(H * 0.63)), sub, font=f_sub, fill=(54, 83, 125))

    f_beta = font("bahnschrift.ttf", 12 * scale)
    label = "BETA"
    lw = d.textlength(label, font=f_beta)
    pad = 8 * scale
    bx0 = (W - (lw + pad * 2)) / 2
    by0 = int(H * 0.70)
    d.rounded_rectangle([bx0, by0, bx0 + lw + pad * 2, by0 + 18 * scale],
                        radius=9 * scale, fill=BLUE)
    d.text((bx0 + pad, by0 + 3 * scale), label, font=f_beta, fill=(255, 255, 255))

    f_foot = font("segoeui.ttf", 9 * scale)
    foot = "veloryft.com"
    fw = d.textlength(foot, font=f_foot)
    d.text(((W - fw) / 2, H - 22 * scale), foot, font=f_foot, fill=(78, 101, 137))

    suffix = "" if scale == 1 else "@2x"
    out = os.path.join(OUT, f"wizard-large-light{suffix}.bmp")
    img.convert("RGB").save(out)
    print("ok:", out)


def make_wizard_small_light(mark, scale):
    W, H = 55 * scale, 58 * scale
    img = vgradient((W, H), LIGHT_TOP, LIGHT_BOT).convert("RGBA")
    glow, layer = blue_glow((W, H), (W // 2, H // 2), int(W * 0.72), 42)
    img = Image.alpha_composite(
        img,
        Image.composite(layer.convert("RGBA"), Image.new("RGBA", (W, H), (0, 0, 0, 0)), glow),
    )
    m = fit(tint_for_light_surface(mark), int(W * 0.72), int(H * 0.72))
    img.paste(m, ((W - m.size[0]) // 2, (H - m.size[1]) // 2), m)
    suffix = "" if scale == 1 else "@2x"
    out = os.path.join(OUT, f"wizard-small-light{suffix}.bmp")
    img.convert("RGB").save(out)
    print("ok:", out)


def make_theme_page_bg(mark, word, scale):
    """Background para a pagina custom de tema (layout estilo marketing)."""
    W, H = 920 * scale, 520 * scale
    img = vgradient((W, H), (6, 14, 30), (3, 8, 18)).convert("RGBA")

    # brilho de fundo, puxando o foco para o centro-direita
    glow, layer = blue_glow((W, H), (int(W * 0.68), int(H * 0.22)), int(W * 0.45), 85)
    img = Image.alpha_composite(
        img,
        Image.composite(layer.convert("RGBA"), Image.new("RGBA", (W, H), (0, 0, 0, 0)), glow),
    )

    d = ImageDraw.Draw(img)

    # container principal
    pad = 12 * scale
    d.rounded_rectangle(
        [pad, pad, W - pad, H - pad],
        radius=18 * scale,
        outline=(42, 73, 120),
        width=max(1, 2 * scale),
        fill=(4, 14, 32, 105),
    )

    # divisoria entre coluna de opcoes e hero
    split_x = int(W * 0.53)
    d.line(
        [(split_x, int(H * 0.09)), (split_x, int(H * 0.91))],
        fill=(36, 68, 112),
        width=max(1, 2 * scale),
    )

    # pequenas barras de acento no titulo
    d.rounded_rectangle(
        [int(W * 0.05), int(H * 0.14), int(W * 0.09), int(H * 0.147)],
        radius=4 * scale,
        fill=BLUE,
    )

    # cards de opcao (guias visuais para os radios sobrepostos no setup)
    card_left = int(W * 0.05)
    card_right = int(W * 0.46)
    card_h = int(H * 0.12)
    first_top = int(H * 0.40)
    gap = int(H * 0.03)
    for i in range(3):
        y0 = first_top + i * (card_h + gap)
        stroke = (20, 105, 220) if i == 0 else (29, 57, 94)
        fill = (4, 18, 40, 155) if i == 0 else (4, 14, 32, 115)
        d.rounded_rectangle(
            [card_left, y0, card_right, y0 + card_h],
            radius=12 * scale,
            outline=stroke,
            width=max(1, 2 * scale),
            fill=fill,
        )

    # painel da arte hero
    hero_x0 = int(W * 0.56)
    hero_y0 = int(H * 0.10)
    hero_x1 = int(W * 0.94)
    hero_y1 = int(H * 0.90)
    d.rounded_rectangle(
        [hero_x0, hero_y0, hero_x1, hero_y1],
        radius=18 * scale,
        outline=(68, 104, 155),
        width=max(1, 2 * scale),
        fill=(2, 10, 24, 170),
    )

    # arte interna do hero com marca
    hero_w = hero_x1 - hero_x0 - 16 * scale
    hero_h = hero_y1 - hero_y0 - 16 * scale
    hero = vgradient((hero_w, hero_h), (6, 16, 38), (2, 8, 20)).convert("RGBA")
    hglow, hlayer = blue_glow((hero_w, hero_h), (hero_w // 2, int(hero_h * 0.55)), int(hero_w * 0.55), 90)
    hero = Image.alpha_composite(
        hero,
        Image.composite(hlayer.convert("RGBA"), Image.new("RGBA", (hero_w, hero_h), (0, 0, 0, 0)), hglow),
    )

    m = fit(mark, int(hero_w * 0.52), int(hero_h * 0.34))
    hero.paste(m, ((hero_w - m.size[0]) // 2, int(hero_h * 0.23)), m)
    wm = fit(word, int(hero_w * 0.74), int(hero_h * 0.20))
    hero.paste(wm, ((hero_w - wm.size[0]) // 2, int(hero_h * 0.68)), wm)

    f_sub = font("bahnschrift.ttf", 12 * scale)
    sub = "TELEMETRY OVERLAY"
    tw = ImageDraw.Draw(hero).textlength(sub, font=f_sub)
    ImageDraw.Draw(hero).text(((hero_w - tw) / 2, int(hero_h * 0.84)), sub, font=f_sub, fill=(46, 154, 255))

    img.paste(hero, (hero_x0 + 8 * scale, hero_y0 + 8 * scale), hero)

    suffix = "" if scale == 1 else "@2x"
    out = os.path.join(OUT, f"theme-page-bg{suffix}.bmp")
    img.convert("RGB").save(out)
    print("ok:", out)


if __name__ == "__main__":
    mark = load_mark()
    word = load_wordmark()
    make_icon(mark)
    for s in (1, 2):
        make_wizard_large(mark, word, s)
        make_wizard_small(mark, s)
        make_wizard_large_light(mark, word, s)
        make_wizard_small_light(mark, s)
    print("done")
