#!/usr/bin/env python
"""生成随仓库分发的中文字体子集 (OFL 授权的 Noto Sans SC / Noto Serif SC).

Why: egui 要把整份字体常驻内存, 系统 msyh.ttc/simsun.ttc 各 ~19MB, 解析后各占 ~60MB;
裁到 GB2312 一二级字后每份 ~2MB, 进程内存从 ~160MB 降到 ~85MB.
选 Noto 而不是裁系统字体: 仓库是 public, 微软字体的派生子集不能分发, Noto 是 SIL OFL 1.1.

Noto 的 VF 默认 master 是 Thin/ExtraLight, ab_glyph 不支持变量实例化,
所以必须先 instancer 到 wght=400 再裁, 否则界面是极细字.

用法: python scripts/build-fonts.py
"""
import io
import os
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(ROOT, "assets", "fonts")
WEIGHT_AXIS, WEIGHT_VALUE = "wght", 400
UNICODES = "U+0020-007E,U+00A0-00FF,U+2000-206F,U+3000-303F,U+FE10-FE1F,U+FF00-FFEF"

TARGETS = [
    ("clowder-cjk-sans.ttf", r"C:\Windows\Fonts\NotoSansSC-VF.ttf"),
    ("clowder-cjk-serif.ttf", r"C:\Windows\Fonts\NotoSerifSC-VF.ttf"),
]


def unicode_ranges(spec):
    """'U+0020-007E,U+2000-206F' -> [int, ...]; populate() 要的是码点不是字符串."""
    out = []
    for part in spec.split(","):
        part = part.strip().upper().replace("U+", "")
        if not part:
            continue
        lo, _, hi = part.partition("-")
        end = int(hi, 16) if hi else int(lo, 16)
        out.extend(range(int(lo, 16), end + 1))
    return out


def gb2312_hanzi():
    """一级 (B0A1-D7F9) + 二级 (D8A1-F7FE), 共 6763 字."""
    out = []
    for hi in range(0xB0, 0xF8):
        for lo in range(0xA1, 0xFF):
            try:
                out.append(bytes([hi, lo]).decode("gb2312"))
            except Exception:
                pass
    return sorted(set(out))


def source_chars():
    """代码里出现过的中文 + 当前 config 里的供应商名, 保证界面不出豆腐块."""
    import glob
    import io

    chars = set()
    paths = glob.glob(os.path.join(ROOT, "src", "**", "*.rs"), recursive=True)
    paths += glob.glob(os.path.join(ROOT, "crates", "**", "*.rs"), recursive=True)
    paths.append(
        os.path.expandvars(
            r"%LOCALAPPDATA%\clowder\clowder\data\config.toml"
        )
    )
    for p in paths:
        if not os.path.exists(p):
            continue
        s = io.open(p, encoding="utf-8", errors="ignore").read()
        for ch in s:
            o = ord(ch)
            if 0x4E00 <= o <= 0x9FFF or 0x3000 <= o <= 0x303F or 0xFF00 <= o <= 0xFFEF:
                chars.add(ch)
    return sorted(chars)


def build(out_name, src):
    if not os.path.exists(src):
        print("SKIP %s: 找不到 %s (装 Noto Sans/Serif SC 或从 fonts.google.com 下载)" % (out_name, src))
        return
    font = TTFont(src)
    font = instancer.instantiateVariableFont(
        font, {WEIGHT_AXIS: WEIGHT_VALUE}, inplace=False
    )
    options = subset.Options()
    options.ignore_missing_unicodes = True
    options.hinting = False
    options.desubroutinize = True
    options.layout_features = ["kern", "liga", "ccmp", "locl", "palt", "halt", "vert", "vrt2"]
    # OFL 要求保留版权声明和许可证, name 表别裁
    options.name_IDs = ["*"]
    options.drop_tables += ["DSIG", "BASE", "VORG", "fvar", "STAT", "gvar", "HVAR", "MVAR", "vhea", "vmtx"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(text="".join(gb2312_hanzi()) + "".join(source_chars()), unicodes=unicode_ranges(UNICODES))
    subsetter.subset(font)
    out = os.path.join(OUT_DIR, out_name)
    font.save(out)
    print("  -> %s  %.2f MB" % (out, os.path.getsize(out) / 1048576.0))


def check_ofl_license():
    """OFL 要求随字体分发许可证全文; 字体内嵌的 name#13 只是一段指向说明, 不够."""
    dst = os.path.join(OUT_DIR, "OFL.txt")
    if os.path.exists(dst):
        print("  OFL.txt ok (%.1f KB)" % (os.path.getsize(dst) / 1024.0))
    else:
        print("  ! 缺 %s — 分发前必须补上 SIL OFL 1.1 全文" % dst)


def write_license_notice():
    """OFL 要求版权声明随字体分发且不删除: 把字体内嵌的版权/许可声明原样抄一份到仓库."""
    lines = ["本目录下的 clowder-cjk-*.ttf 是下列字体的子集 (instanced 到 wght=400 后裁剪), 授权: SIL OFL 1.1.", ""]
    for out_name, src in TARGETS:
        if not os.path.exists(src):
            continue
        rec = {}
        for name_id in (0, 13):
            r = TTFont(src, fontNumber=0, lazy=True)["name"].getName(name_id, 3, 1)
            if r:
                rec[name_id] = str(r)
        lines.append("源字体: %s" % os.path.basename(src))
        lines.append("  版权: %s" % rec.get(0, ""))
        lines.append("  许可: %s" % rec.get(13, ""))
        lines.append("")
    lines.append("完整 OFL 1.1 正文请放在同目录 OFL.txt (https://openfontlicense.org).")
    io.open(os.path.join(OUT_DIR, "LICENSE-Noto.txt"), "w", encoding="utf-8").write(
        "\n".join(lines) + "\n"
    )
    print("  -> LICENSE-Noto.txt")


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    chars = set(gb2312_hanzi()) | set(source_chars())
    print("字符集: %d 个" % len(chars))
    for out_name, src in TARGETS:
        print("subset %s" % os.path.basename(src))
        build(out_name, src)
    write_license_notice()
    check_ofl_license()


if __name__ == "__main__":
    main()
