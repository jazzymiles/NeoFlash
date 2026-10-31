"""Build site/: two Russian gallery pages (dark.html, light.html) with every exported screen and panel.

Self-contained: images are copied to site/img/<theme>/, all links are relative (no leading "/").
Run after the NeoFlash Export plugin:  python3 tools/build-gallery.py
"""
import html
import json
import os
import re
import shutil

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
SITE = os.path.join(ROOT, 'site')
manifest = json.load(open(os.path.join(ROOT, 'exports', 'manifest.json')))
notes = json.load(open(os.path.join(ROOT, 'docs', 'source', 'notes.ru.json')))
dump = json.load(open(os.path.join(ROOT, 'docs', 'data', 'figma-dump.json')))

EXTRA = {  # frames without a note in notes.ru.json
    'menus': ('menus', 'Главное и контекстные меню', 'Выпадающие меню File, Edit, View, Insert, Modify, Control, Window с горячими клавишами и подменю; контекстные меню кадра, слоя и объекта на сцене.'),
    'timeline-frame-views': ('timeline', 'Режимы отображения кадров', 'Меню ⋯ таймлайна: расположение панели и режимы Tiny, Small, Normal, Medium, Large, Short, Preview, Preview in context, без подкраски твинов.'),
    'timeline-button-symbol-cta-btn': ('timeline', 'Таймлайн символа-кнопки', 'Четыре кадра Up · Over · Down · Hit и превью состояний; кадр Hit задаёт только зону клика.'),
}

SECTIONS = [
    ('screens', 'Экраны', 'Полноразмерные экраны редактора 1920 × 1080 в разных состояниях.'),
    ('panels', 'Панели', 'Отсоединяемые панели: инструменты, свойства, цвет, изинг, библиотека, скрипты и другие.'),
    ('timeline', 'Таймлайн', 'Режимы отображения кадров и таймлайн символа-кнопки.'),
    ('menus', 'Меню', 'Главное меню и контекстные меню с горячими клавишами.'),
    ('dialogs', 'Диалоги', 'Диалоги импорта, экспорта, публикации, документа и символов.'),
    ('prefs', 'Настройки приложения', 'Все страницы окна настроек.'),
]

THEMES = {
    'dark': dict(name='Тёмная тема', other='light', other_name='Светлая тема', bg='#0d1016', panel='#12151c', raised='#171b24',
                 border='#232938', text='#e7eaf2', text2='#9aa3b6', text3='#8790a4', accent='#6c93ff', scheme='dark'),
    'light': dict(name='Светлая тема', other='dark', other_name='Тёмная тема', bg='#eef0f5', panel='#f6f7fa', raised='#ffffff',
                  border='#e1e4ec', text='#171b26', text2='#5b6477', text3='#667086', accent='#3c6bf0', scheme='light'),
}


def esc(s):
    return html.escape(str(s), quote=True)


def inline(s):
    s = esc(s)
    s = re.sub(r'\*\*(.+?)\*\*', r'<b>\1</b>', s)
    return re.sub(r'`(.+?)`', r'<code>\1</code>', s)


def entries(theme):
    out = {k: [] for k, _, _ in SECTIONS}
    for it in manifest:
        if it['theme'] != theme or it['group'] not in ('interface', 'panels'):
            continue
        slug = it['slug']
        if slug in notes:
            n = notes[slug]
            cat, title, text = n['cat'], n['title'], n['text']
        elif slug in EXTRA:
            cat, title, text = EXTRA[slug]
        else:
            cat, title, text = ('screens' if it['group'] == 'interface' else 'panels'), it['name'], ''
        if it['group'] == 'interface':
            cat = 'screens'
        src = os.path.join(ROOT, it['png'])
        rel = f'img/{theme}/{it["group"]}/{slug}.png'
        dst = os.path.join(SITE, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        if not os.path.exists(dst) or os.path.getmtime(dst) < os.path.getmtime(src):
            shutil.copy2(src, dst)
        # real pixel size from the PNG header (exports include shadow padding), halved for @2x display
        with open(dst, 'rb') as f:
            head = f.read(24)
        w, h = int.from_bytes(head[16:20], 'big') // 2, int.from_bytes(head[20:24], 'big') // 2
        out[cat].append(dict(title=title, text=text, img=rel, w=w, h=h, id=slug))
    return out


def page(theme):
    t = THEMES[theme]
    data = entries(theme)
    nav = ''.join(f'<a href="#{k}">{esc(name)}<span>{len(data[k])}</span></a>' for k, name, _ in SECTIONS if data[k])
    body = []
    for key, name, lead in SECTIONS:
        items = data[key]
        if not items:
            continue
        cards = []
        for e in items:
            wide = key == 'screens' or e['w'] >= 900
            desc = f'<p>{inline(e["text"])}</p>' if e['text'] else ''
            cards.append(
                f'<article class="card{" wide" if wide else ""}" id="{e["id"]}">'
                f'<a class="pic" href="{e["img"]}" target="_blank" rel="noopener"><img src="{e["img"]}" alt="{esc(e["title"])}" loading="lazy" width="{e["w"]}" height="{e["h"]}"></a>'
                f'<div class="meta"><h3>{esc(e["title"])}</h3>{desc}'
                f'<a class="open" href="{e["img"]}" target="_blank" rel="noopener">Открыть PNG ↗</a></div></article>')
        body.append(f'<section id="{key}"><header class="sec"><h2>{esc(name)}</h2><p>{esc(lead)}</p></header>'
                    f'<div class="{"screens" if key == "screens" else "grid"}">{"".join(cards)}</div></section>')
    css = f"""
:root {{ color-scheme: {t['scheme']}; --bg:{t['bg']}; --panel:{t['panel']}; --raised:{t['raised']}; --border:{t['border']};
  --text:{t['text']}; --text2:{t['text2']}; --text3:{t['text3']}; --accent:{t['accent']}; }}
* {{ box-sizing: border-box; }}
html {{ scroll-padding-top: 76px; }}
body {{ margin: 0; background: var(--bg); color: var(--text); font: 14px/1.55 Inter, -apple-system, "Segoe UI", Roboto, Arial, sans-serif; }}
code {{ font: 12px "Roboto Mono", Menlo, Consolas, monospace; background: var(--raised); border: 1px solid var(--border); border-radius: 4px; padding: 0 4px; }}
.top {{ position: sticky; top: 0; z-index: 5; background: color-mix(in srgb, var(--bg) 88%, transparent); backdrop-filter: blur(10px);
  border-bottom: 1px solid var(--border); }}
.top .in {{ max-width: 1440px; margin: 0 auto; padding: 12px 24px; display: flex; gap: 16px; align-items: center; flex-wrap: wrap; }}
.brand {{ display: flex; gap: 10px; align-items: center; font-weight: 600; font-size: 15px; margin-right: 8px; }}
.logo {{ width: 26px; height: 26px; border-radius: 7px; background: var(--accent); display: grid; place-items: center; }}
.logo i {{ width: 10px; height: 10px; border-radius: 50%; border: 3px solid var(--bg); }}
nav {{ display: flex; gap: 4px; flex-wrap: wrap; }}
nav a {{ color: var(--text2); text-decoration: none; padding: 5px 10px; border-radius: 6px; font-size: 13px; }}
nav a:hover {{ background: var(--raised); color: var(--text); }}
nav a span {{ color: var(--text3); margin-left: 6px; font-size: 12px; }}
.switch {{ margin-left: auto; color: var(--text); text-decoration: none; border: 1px solid var(--border); background: var(--raised);
  padding: 6px 12px; border-radius: 6px; font-size: 13px; }}
.switch:hover {{ border-color: var(--accent); }}
main {{ max-width: 1440px; margin: 0 auto; padding: 32px 24px 80px; }}
.hero h1 {{ font-size: 32px; line-height: 1.2; margin: 0 0 8px; letter-spacing: -0.01em; }}
.hero p {{ color: var(--text2); max-width: 760px; margin: 0; font-size: 15px; }}
section {{ margin-top: 56px; }}
.sec {{ border-bottom: 1px solid var(--border); padding-bottom: 12px; margin-bottom: 20px; }}
.sec h2 {{ margin: 0 0 4px; font-size: 22px; }}
.sec p {{ margin: 0; color: var(--text2); }}
.screens {{ display: grid; gap: 28px; }}
.grid {{ columns: 3 340px; column-gap: 20px; }}
.card {{ break-inside: avoid; background: var(--panel); border: 1px solid var(--border); border-radius: 12px; overflow: hidden; margin: 0 0 20px; }}
.screens .card {{ margin: 0; }}
.pic {{ display: block; background: var(--raised); padding: 16px; text-align: center; border-bottom: 1px solid var(--border); }}
.pic img {{ max-width: 100%; height: auto; display: inline-block; vertical-align: top; }}
.screens .pic {{ padding: 0; }}
.meta {{ padding: 14px 16px 16px; }}
.meta h3 {{ margin: 0 0 6px; font-size: 15px; }}
.meta p {{ margin: 0 0 10px; color: var(--text2); font-size: 13px; }}
.open {{ color: var(--accent); text-decoration: none; font-size: 13px; }}
.open:hover {{ text-decoration: underline; }}
footer {{ max-width: 1440px; margin: 0 auto; padding: 0 24px 40px; color: var(--text3); font-size: 12px; }}
@media (max-width: 1100px) {{ html {{ scroll-padding-top: 160px; }} }}
@media (max-width: 640px) {{ .top .in, main, footer {{ padding-left: 16px; padding-right: 16px; }} .hero h1 {{ font-size: 24px; }} .grid {{ columns: 1; }} }}
"""
    total = sum(len(v) for v in data.values())
    return f"""<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>NeoFlash — {esc(t['name'])}</title>
<style>{css}</style>
</head>
<body>
<div class="top"><div class="in">
  <div class="brand"><span class="logo"><i></i></span>NeoFlash 2.0</div>
  <nav>{nav}</nav>
  <a class="switch" href="{t['other']}.html">{esc(t['other_name'])} →</a>
</div></div>
<main>
  <div class="hero">
    <h1>Интерфейс и панели · {esc(t['name'].lower())}</h1>
    <p>Все экраны, панели, меню, диалоги и настройки редактора NeoFlash (дизайн-система NF2), всего макетов: {total}. Клик по картинке открывает её в полном размере (PNG @2x). Подписи в самом интерфейсе — на английском.</p>
  </div>
  {''.join(body)}
</main>
<footer>Сгенерировано из файла Figma «{esc(dump['file'])}» · {dump['generated'][:10]}</footer>
</body>
</html>
"""


def main():
    for theme in THEMES:
        out = page(theme)
        open(os.path.join(SITE, f'{theme}.html'), 'w').write(out)
        print(f'site/{theme}.html', len(out) // 1024, 'kB')


if __name__ == '__main__':
    os.makedirs(SITE, exist_ok=True)
    main()
