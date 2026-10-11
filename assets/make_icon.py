# 生成程序图标（原创设计：深色圆角方块 + 白色双八分音符 + 焦点色弧线）。
# 用法: python assets/make_icon.py   → assets/icon_256.png, assets/app.ico
from PIL import Image, ImageDraw
import os

S = 1024  # 先画大图再缩小，得到平滑边缘
here = os.path.dirname(os.path.abspath(__file__))


def draw():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # 背景：深色圆角方块，略带渐变
    pad = 40
    for i in range(60):
        t = i / 59
        c = (int(46 - 18 * t), int(50 - 18 * t), int(62 - 20 * t), 255)
        r = 210 - i
        d.rounded_rectangle([pad + i * 2, pad + i * 2, S - pad - i * 2, S - pad - i * 2], radius=max(r, 120), fill=c)
    d.rounded_rectangle([pad, pad, S - pad, S - pad], radius=210, outline=(90, 96, 112, 255), width=6)
    accent = (56, 196, 160, 255)
    # 弧线
    d.arc([200, 150, 824, 640], start=200, end=340, fill=accent, width=46)
    white = (245, 247, 250, 255)
    # 两个符头（倾斜椭圆）
    for cx, cy in [(372, 740), (668, 690)]:
        head = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        hd = ImageDraw.Draw(head)
        hd.ellipse([cx - 112, cy - 80, cx + 112, cy + 80], fill=white)
        head = head.rotate(22, center=(cx, cy), resample=Image.BICUBIC)
        img.alpha_composite(head)
    # 符干与符杠
    d.rounded_rectangle([452, 380, 502, 742], radius=25, fill=white)
    d.rounded_rectangle([748, 330, 798, 692], radius=25, fill=white)
    d.polygon([(452, 380), (798, 330), (798, 430), (452, 480)], fill=white)
    return img


def main():
    big = draw()
    big.resize((256, 256), Image.LANCZOS).save(os.path.join(here, "icon_256.png"))
    sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    big.resize((256, 256), Image.LANCZOS).save(os.path.join(here, "app.ico"), sizes=sizes)
    print("ok")


main()
