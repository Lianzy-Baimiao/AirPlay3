# 生成安装向导的图片（用程序自己的图标 assets/icon_256.png，深色底，与播放器的深色界面一致）。
# 用法: python installer/make_images.py   → installer/WizardImage.png（欢迎页 / 完成页左边）, installer/WizardSmallImage.png（右上角）
import os

from PIL import Image, ImageDraw, ImageFilter, ImageFont

here = os.path.dirname(os.path.abspath(__file__))
icon = Image.open(os.path.join(here, "..", "assets", "icon_256.png")).convert("RGBA")
FONTS = os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts")


def font(name, size):
    return ImageFont.truetype(os.path.join(FONTS, name), size)


def spaced(d, text, f, cx, y, fill, gap):
    """字间距加宽、水平居中的一行字；返回字的下边"""
    widths = [d.textlength(ch, font=f) for ch in text]
    x = cx - (sum(widths) + gap * (len(text) - 1)) / 2
    bottom = y
    for ch, w in zip(text, widths):
        d.text((x, y), ch, font=f, fill=fill)
        bottom = max(bottom, d.textbbox((x, y), ch, font=f)[3])
        x += w + gap
    return bottom


def big():
    # 202x386 的 2 倍：高 DPI 下也清楚（宽高比 164:314）
    W, H = 404, 772
    img = Image.new("RGBA", (W, H))
    px = img.load()
    for y in range(H):
        t = y / (H - 1)
        c = (int(40 - 18 * t), int(43 - 18 * t), int(52 - 22 * t), 255)
        for x in range(W):
            px[x, y] = c
    # 图标后面一圈很淡的焦点色光
    glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([W / 2 - 150, 120, W / 2 + 150, 420], fill=(56, 196, 160, 40))
    img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(60)))
    ic = icon.resize((200, 200), Image.LANCZOS)
    img.alpha_composite(ic, ((W - 200) // 2, 170))
    d = ImageDraw.Draw(img)
    y = spaced(d, "AIRPLAY", font("segoeuil.ttf", 46), W / 2, 412, (236, 238, 242, 255), 9) + 20
    d.line([(W / 2 - 34, y), (W / 2 + 34, y)], fill=(56, 196, 160, 200), width=2)
    spaced(d, "乐声飞扬 聆听彼此", font("msyhl.ttc", 21), W / 2, y + 14, (150, 156, 168, 255), 4)
    return img.convert("RGB")


def small():
    # 右上角：只有图标，透明底（58x58 的 2.5 倍）
    return icon.resize((147, 147), Image.LANCZOS)


big().save(os.path.join(here, "WizardImage.png"))
small().save(os.path.join(here, "WizardSmallImage.png"))
print("ok")
