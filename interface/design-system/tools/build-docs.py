"""Build the NeoFlash design-system docs from the Figma export.

Inputs  (all under design-system/):
  docs/data/figma-dump.json, docs/data/outline/*.txt, exports/manifest.json   ← NeoFlash Export plugin
  docs/source/NeoFlash-Design-System.src.md                                  ← Claude reference (EN)
  docs/source/pdf.en.html, docs/source/pdf.ru.html                           ← PDF templates
  docs/source/notes.en.json, docs/source/notes.ru.json                       ← panel / dialog / screen notes
Outputs:
  docs/NeoFlash-Design-System.md
  docs/source/build/NeoFlash-Design-System-{EN,RU}.html (+ build/img/ downscaled images for the PDFs)
Placeholders: {{GEN:name}} in the sources are replaced by generated sections; {{GENERATED}}, {{FILE_NAME}}.
"""
import html
import json
import os
import re
import subprocess
from datetime import date

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
DOCS = os.path.join(ROOT, 'docs')
SRC = os.path.join(DOCS, 'source')
BUILD = os.path.join(SRC, 'build')

dump = json.load(open(os.path.join(DOCS, 'data', 'figma-dump.json')))
manifest = json.load(open(os.path.join(ROOT, 'exports', 'manifest.json')))
NF2 = next(c for c in dump['collections'] if c['name'] == 'NF2 · Tokens')
TOK = {v['name']: v for v in NF2['variables']}
COMPS = dump['components']['v2 · System & Components']

T = {  # UI strings of generated parts
    'en': dict(token='Token', dark='Dark', light='Light', name='Name', font='Font', size='Size', lh='Line height',
               ls='Letter spacing', case='Case', comp='Component', kind='Kind', props='Properties', desc='Description',
               variants='variants', item='Item', shortcut='Shortcut', notes='Notes', area='Area', capability='Capability',
               where='Where in the file', state='State', look='Look', hotspot='Hotspot', group='Group', icons='Icons',
               page='Page', id='Id', frames='Top-level frames', screen='Screen', files='Files', folder='Folder', count='Files',
               submenu='submenu ▸', checked='checked ✓', disabled='disabled', value='Value', usage='Usage',
               light_twin='Light theme', svg='SVG'),
    'ru': dict(token='Токен', dark='Тёмная', light='Светлая', name='Название', font='Шрифт', size='Размер', lh='Интерлиньяж',
               ls='Трекинг', case='Регистр', comp='Компонент', kind='Тип', props='Свойства', desc='Описание',
               variants='вариантов', item='Пункт', shortcut='Клавиши', notes='Примечание', area='Область', capability='Возможность',
               where='Где в файле', state='Состояние', look='Вид', hotspot='Хотспот', group='Группа', icons='Иконки',
               page='Страница', id='Id', frames='Фреймы верхнего уровня', screen='Экран', files='Файлы', folder='Папка', count='Файлов',
               submenu='подменю ▸', checked='отмечен ✓', disabled='неактивен', value='Значение', usage='Применение',
               light_twin='Светлая тема', svg='SVG'),
}

# ---------- helpers ----------

def esc(s):
    return html.escape(str(s), quote=False)


def inline(s, fmt):
    """Render the tiny markdown subset used in notes (**bold**, `code`) for md or html."""
    if fmt == 'md':
        return s
    s = esc(s)
    s = re.sub(r'\*\*(.+?)\*\*', r'<b>\1</b>', s)
    return re.sub(r'`(.+?)`', r'<code>\1</code>', s)


def table(headers, rows, fmt, cls=''):
    if fmt == 'md':
        out = ['| ' + ' | '.join(headers) + ' |', '|' + '---|' * len(headers)]
        for r in rows:
            out.append('| ' + ' | '.join(str(c).replace('|', '\\|').replace('\n', ' ') for c in r) + ' |')
        return '\n'.join(out) + '\n'
    head = ''.join(f'<th>{esc(h)}</th>' for h in headers)
    body = ''.join('<tr>' + ''.join(f'<td>{c}</td>' for c in r) + '</tr>' for r in rows)
    return f'<table class="{cls}"><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>\n'


def val(v, mode):
    x = v['values'][mode]
    if isinstance(x, dict):
        return '→ ' + x['alias']
    return x


def swatch(hexv, fmt):
    if fmt == 'md':
        return f'`{hexv}`'
    h = hexv if isinstance(hexv, str) and hexv.startswith('#') else '#000'
    return f'<span class="sw" style="background:{h}"></span><code>{esc(hexv)}</code>'


IMG_CACHE = {}


def img(path_from_root, fmt, out_dir, caption='', width=None):
    """Image reference. For html the image is downscaled once into build/img (keeps the PDFs small)."""
    src = os.path.join(ROOT, path_from_root)
    if not os.path.exists(src):
        return f'*(missing {path_from_root})*' if fmt == 'md' else f'<p class="missing">missing {esc(path_from_root)}</p>'
    if fmt == 'md':
        rel = os.path.relpath(src, out_dir)
        return f'![{caption}]({rel})'
    if src not in IMG_CACHE:
        os.makedirs(os.path.join(BUILD, 'img'), exist_ok=True)
        dst = os.path.join(BUILD, 'img', path_from_root.replace('/', '__'))
        if not os.path.exists(dst) or os.path.getmtime(dst) < os.path.getmtime(src):
            subprocess.run(['sips', '-Z', str(width or 1800), src, '--out', dst], check=True, capture_output=True)
        IMG_CACHE[src] = os.path.relpath(dst, out_dir)
    cap = f'<figcaption>{esc(caption)}</figcaption>' if caption else ''
    return f'<figure><img src="{IMG_CACHE[src]}" alt="{esc(caption)}">{cap}</figure>'


def item(slug, group='panels', theme='dark'):
    return next((i for i in manifest if i['slug'] == slug and i['group'] == group and i['theme'] == theme), None)


def notes(lang):
    return json.load(open(os.path.join(SRC, f'notes.{lang}.json')))

# ---------- generated sections ----------

def gen_pages(fmt, lang, out_dir):
    t = T[lang]
    rows = []
    for p in dump['pages']:
        fr = ', '.join(f"{f['name']} ({f['id']})" for f in p['frames'][:14]) + (' …' if len(p['frames']) > 14 else '')
        rows.append([p['name'], p['id'], fr if fmt == 'md' else esc(fr)])
    return table([t['page'], t['id'], t['frames']], rows, fmt, 'small')


COLOR_USAGE = {
    'en': {'bg': 'Surfaces', 'border': 'Borders & focus', 'text': 'Text', 'accent': 'Accent (interactive)', 'brand': 'Brand',
           'danger': 'Danger / error', 'success': 'Success', 'warning': 'Warning', 'layer': 'Layer colours', 'timeline': 'Timeline',
           'syntax': 'Code syntax', 'other': 'Canvas aids & overlays'},
    'ru': {'bg': 'Поверхности', 'border': 'Рамки и фокус', 'text': 'Текст', 'accent': 'Акцент (интерактив)', 'brand': 'Бренд',
           'danger': 'Опасное действие / ошибка', 'success': 'Успех', 'warning': 'Предупреждение', 'layer': 'Цвета слоёв',
           'timeline': 'Таймлайн', 'syntax': 'Подсветка кода', 'other': 'Вспомогательное и оверлеи'},
}


def color_groups():
    groups = {}
    for v in NF2['variables']:
        if v['type'] != 'COLOR':
            continue
        parts = v['name'].split('/')
        key = parts[1] if len(parts) > 2 else 'other'
        groups.setdefault(key, []).append(v)
    order = ['bg', 'border', 'text', 'accent', 'brand', 'danger', 'warning', 'success', 'layer', 'timeline', 'syntax', 'other']
    return [(k, groups[k]) for k in order if k in groups]


def gen_colors(fmt, lang, out_dir):
    t = T[lang]
    out = []
    for key, vs in color_groups():
        title = COLOR_USAGE[lang].get(key, key)
        out.append(f'#### {title}\n' if fmt == 'md' else f'<h4>{esc(title)}</h4>')
        rows = [[f'`{v["name"]}`' if fmt == 'md' else f'<code>{esc(v["name"])}</code>', swatch(val(v, 'Dark'), fmt), swatch(val(v, 'Light'), fmt)] for v in vs]
        out.append(table([t['token'], t['dark'], t['light']], rows, fmt, 'tokens'))
    return '\n'.join(out)


def gen_dimensions(fmt, lang, out_dir):
    t = T[lang]
    rows = [[f'`{v["name"]}`' if fmt == 'md' else f'<code>{v["name"]}</code>', f'{val(v, "Dark")} px'] for v in NF2['variables'] if v['type'] == 'FLOAT']
    return table([t['token'], t['value']], rows, fmt, 'narrow')


def gen_type(fmt, lang, out_dir):
    t = T[lang]
    rows = []
    for s in dump['textStyles']:
        if not s['name'].startswith('NF2/'):
            continue
        ls = s['letterSpacing'].replace('.0', '')
        rows.append([f'`{s["name"]}`' if fmt == 'md' else f'<code>{s["name"]}</code>', f'{s["family"]} {s["style"]}', s['size'], s['lineHeight'],
                     '0' if ls in ('0%', '0px') else ls, 'UPPER' if s['textCase'] == 'UPPER' else '—'])
    return table([t['name'], t['font'], t['size'], t['lh'], t['ls'], t['case']], rows, fmt)


ICON_GROUPS = [
    ('Tools (20 px)', 'Инструменты (20 px)', r'/20/(tool-|color-)'),
    ('Layers & timeline', 'Слои и таймлайн', r'/(layer|folder|folderopen|guide|motionguide|mask|masked|eye|eyeoff|lock|unlock|layerplus|folderplus|guideplus|trash|flag|anchor|keyframe-add|blankkey-add|frame-add|keyframe-remove|tween-classic|tween-shape|tween-motion|ease|onion|onionoutline|editmulti|markers|centerframe|pencil|film|layers|armature|joint|motion-editor|motion-path)$'),
    ('Playback, sound & video', 'Воспроизведение, звук и видео', r'/(first|last|stepback|stepfwd|play|pause|stop|loop|sound|waveform|sound-file|video)$'),
    ('Align & transform', 'Выравнивание и трансформация', r'/(align-.*|dist-.*|space-.*|match-.*|to-stage|rotate-.*|flip-.*|reset-transform|skew|angle|scale|link)$'),
    ('Stroke', 'Обводка', r'/(cap-.*|join-.*)$'),
    ('Text', 'Текст', r'/(talign-.*|superscript|subscript|letter-spacing|line-spacing|indent|margin-.*|selectable|html|text-border|text-orientation|embed|text|font)$'),
    ('Colour', 'Цвет', r'/(swatches|mixer|recent|nocolor|colorwheel|eyedropper16|bucket|flow-.*|bitmap-fill|lock-fill)$'),
    ('View & canvas', 'Вид и холст', r'/(ruler|guides|grid|snap-grid|snap-guides|lock-guides|origin|view-settings|hand|fit|zoomin|zoomout)$'),
    ('Library & symbols', 'Библиотека и символы', r'/(movieclip|graphic|button-symbol|bitmap|symbol|scene|package|class)$'),
    ('Code, debug & diagnostics', 'Код, отладка и диагностика', r'/(code|terminal|target|autoformat|codehint|debug|collapse|expand|comment-.*|uncomment|wand|help|sidebar|frame|pin|error|warning|breakpoint|bookmark|info|fold-.*|debug-continue|step-.*|callstack|variable|exec-arrow|file-code|snippet|find-replace|tree)$'),
    ('App, menus & publishing', 'Приложение, меню и публикация', r'/(scissors|copy|paste|redo|undo|select-all|new-file|open|publish|upload|keyboard|submenu|history|info-panel|settings|save|access|pen|globe|speed|desktop)$'),
    ('Interface & other', 'Интерфейс и прочее', r'.'),
]


def gen_icons(fmt, lang, out_dir):
    t = T[lang]
    names = sorted(c['name'] for c in COMPS if c['name'].startswith('NF2 Icon/'))
    rest = list(names)
    rows = []
    for en, ru, rx in ICON_GROUPS:
        hit = [n for n in rest if re.search(rx, n)]
        rest = [n for n in rest if n not in hit]
        if not hit:
            continue
        short = ', '.join(n.replace('NF2 Icon/', '') for n in hit)
        rows.append([en if lang == 'en' else ru, len(hit), short if fmt == 'md' else f'<span class="mono-small">{esc(short)}</span>'])
    total = f'{len(names)} icons' if lang == 'en' else f'Всего иконок: {len(names)}'
    return (f'**{total}** (16 px and 20 px).\n\n' if fmt == 'md' else f'<p><b>{total}</b> (16 и 20 px).</p>' if lang == 'ru' else f'<p><b>{total}</b> (16 and 20 px).</p>') + \
        table([t['group'], '#', t['icons']], rows, fmt, 'small')


def gen_cursors(fmt, lang, out_dir):
    t = T[lang]
    rows = []
    for c in sorted((c for c in COMPS if c['name'].startswith('NF2 Cursor/')), key=lambda c: c['name']):
        m = re.search(r'x\s*(\d+),\s*y\s*(\d+)', c['description'])
        rows.append([c['name'].replace('NF2 Cursor/', ''), f'{m.group(1)}, {m.group(2)}' if m else '—'])
    half = (len(rows) + 1) // 2
    merged = [r + (rows[half + i] if half + i < len(rows) else ['', '']) for i, r in enumerate(rows[:half])]
    return table([t['name'], t['hotspot'] + ' (x, y)', t['name'], t['hotspot'] + ' (x, y)'], merged, fmt, 'small')


TL_STATES = [
    ('Empty frames', 'Пустые кадры', 'timeline/empty', 'grid, every 5th frame shaded (`timeline/empty-5th`)', 'сетка, каждый 5-й кадр затенён (`timeline/empty-5th`)'),
    ('Static frames', 'Статичные кадры', 'timeline/static', 'frames holding content after a keyframe ●, ends with ▯', 'кадры с содержимым после ключевого ●, конец ▯'),
    ('Blank keyframe', 'Пустой ключевой кадр', 'timeline/empty', 'hollow ○; following empty frames stay empty', 'полый ○; следующие кадры остаются пустыми'),
    ('Classic tween', 'Классический твин', 'timeline/classic-tween', 'violet span with a solid arrow between keyframes', 'фиолетовый диапазон со сплошной стрелкой между ключами'),
    ('Shape tween', 'Shape-твин', 'timeline/shape-tween', 'green span with a solid arrow', 'зелёный диапазон со сплошной стрелкой'),
    ('Motion tween', 'Motion-твин', 'timeline/motion-tween', 'blue span, property keyframes ◆, no arrow', 'синий диапазон, ключи свойств ◆, без стрелки'),
    ('Broken tween', 'Сломанный твин', 'timeline/classic-tween', 'dashed arrow: end keyframe missing', 'пунктирная стрелка: нет конечного ключа'),
    ('Selected frames', 'Выделенные кадры', 'timeline/selected', 'markers switch to Tone = On Selection', 'маркеры переключаются в Tone = On Selection'),
    ('Sound', 'Звук', 'timeline/static', 'waveform drawn in the span (`success/default`)', 'внутри диапазона волна звука (`success/default`)'),
    ('Video', 'Видео', 'timeline/static', 'filmstrip of the clip in the span', 'внутри диапазона раскадровка видео'),
    ('Frame label / comment', 'Метка / комментарий', 'timeline/label', 'red flag label, green // comment (`timeline/comment`), ⚓ anchor', 'красный флаг-метка, зелёный // комментарий (`timeline/comment`), ⚓ якорь'),
    ('Playhead', 'Плейхед', 'timeline/playhead', 'line through all rows + frame chip + time chip', 'линия через все слои + номер кадра + время'),
    ('Onion skin range', 'Луковая кожа', 'timeline/onion', 'handles on the ruler around the playhead', 'ручки на линейке вокруг плейхеда'),
]


def gen_timeline_states(fmt, lang, out_dir):
    t = T[lang]
    rows = []
    for en, ru, tok, den, dru in TL_STATES:
        v = TOK['color/' + tok]
        rows.append([en if lang == 'en' else ru, inline(f'`{tok}`', fmt), swatch(val(v, 'Dark'), fmt), swatch(val(v, 'Light'), fmt), inline(den if lang == 'en' else dru, fmt)])
    return table([t['state'], t['token'], t['dark'], t['light'], t['look']], rows, fmt)


def gen_notes(cat, fmt, lang, out_dir, level=3):
    t = T[lang]
    out = []
    for slug, n in notes(lang).items():
        if n['cat'] != cat:
            continue
        it = item(slug)
        lt = item(slug, theme='light')
        if fmt == 'md':
            out.append(f'{"#" * level} {n["title"]}\n')
            if it:
                out.append(img(it['png'], fmt, out_dir, n['title']) + '\n')
            out.append(n['text'] + '\n')
            if it:
                links = [f'[{t["dark"]} PNG]({os.path.relpath(os.path.join(ROOT, it["png"]), out_dir)})', f'[SVG]({os.path.relpath(os.path.join(ROOT, it["svg"]), out_dir)})']
                if lt:
                    links += [f'[{t["light"]} PNG]({os.path.relpath(os.path.join(ROOT, lt["png"]), out_dir)})', f'[SVG]({os.path.relpath(os.path.join(ROOT, lt["svg"]), out_dir)})']
                out.append('Files: ' + ' · '.join(links) + f' · Figma node `{it["id"]}`\n')
        else:
            wide = it and (it['w'] >= 500 or it['h'] >= 1200)  # boards of several dialogs: text on top, picture full width
            pic = img(it['png'], fmt, out_dir, '', width=2000 if wide else 1400) if it else ''
            if wide:
                out.append(f'<section class="panel wide"><h3>{esc(n["title"])}</h3><p>{inline(n["text"], fmt)}</p>{pic}</section>')
            else:
                out.append(f'<section class="panel"><h3>{esc(n["title"])}</h3><div class="panel-row">'
                           f'<div class="panel-pic">{pic}</div><div class="panel-text"><p>{inline(n["text"], fmt)}</p></div></div></section>')
    return '\n'.join(out)


def gen_panels(fmt, lang, out_dir):
    return gen_notes('panels', fmt, lang, out_dir)


def gen_dialogs(fmt, lang, out_dir):
    return gen_notes('dialogs', fmt, lang, out_dir) + '\n' + gen_notes('prefs', fmt, lang, out_dir)


def gen_interface_gallery(fmt, lang, out_dir):
    t = T[lang]
    n = notes(lang)
    if fmt == 'md':
        rows = []
        for it in manifest:
            if it['group'] != 'interface' or it['theme'] != 'dark':
                continue
            lt = item(it['slug'], 'interface', 'light')
            r = lambda p: os.path.relpath(os.path.join(ROOT, p), out_dir)
            rows.append([it['name'].replace('NF2 / ', ''), f'[PNG]({r(it["png"])}) · [SVG]({r(it["svg"])})', f'[PNG]({r(lt["png"])}) · [SVG]({r(lt["svg"])})', f'`{it["id"]}` / `{lt["id"]}`'])
        return table([t['screen'], t['dark'], t['light'], 'Figma'], rows, fmt)
    out = []
    for it in manifest:
        if it['group'] != 'interface' or it['theme'] != 'dark' or it['slug'] in ('editor-main', 'timeline-states-legend'):
            continue
        note = n.get(it['slug'], {'title': it['name'], 'text': ''})
        out.append(f'<section class="screen"><h3>{esc(note["title"])}</h3>{img(it["png"], fmt, out_dir, "", 1800)}<p>{inline(note["text"], fmt)}</p></section>')
    return '\n'.join(out)


def parse_menus():
    menus, cur = [], None
    for line in open(os.path.join(DOCS, 'data', 'outline', 'panels-menus.txt')):
        s = line.strip()
        m = re.match(r'F Menu / (.+?) \d+×\d+', s)
        if m:
            cur = (m.group(1), [])
            menus.append(cur)
            continue
        if cur and s.startswith('I NF2/Menu Item'):
            texts = re.findall(r'"([^"]*)"', s)
            props = dict(re.findall(r'(Submenu|State)=([^,}]+)', s))
            label = texts[0] if texts else ''
            sc = texts[1].strip() if len(texts) > 1 else ''
            cur[1].append((label, sc, props.get('Submenu') == 'true', props.get('State', 'Default')))
    return menus


def gen_menus(fmt, lang, out_dir):
    t = T[lang]
    out = []
    for name, items in parse_menus():
        out.append(f'#### {name}\n' if fmt == 'md' else f'<div class="menu"><h4>{esc(name)}</h4>')
        rows = []
        for label, sc, sub, state in items:
            note = ', '.join(x for x in [t['submenu'] if sub else '', t['checked'] if state == 'Checked' else '', t['disabled'] if state == 'Disabled' else ''] if x)
            rows.append([label if fmt == 'md' else esc(label), (f'`{sc}`' if sc else '') if fmt == 'md' else (f'<kbd>{esc(sc)}</kbd>' if sc else ''), note])
        out.append(table([t['item'], t['shortcut'], t['notes']], rows, fmt, 'menu-table'))
        if fmt == 'html':
            out.append('</div>')
    return '\n'.join(out)


def parse_coverage():
    texts = [re.match(r'\s*T "(.*)"$', l).group(1) for l in open(os.path.join(DOCS, 'data', 'outline', 'docs-functional-coverage-v2.txt')) if re.match(r'\s*T "', l)]
    i = texts.index('WHERE IN THE FILE') + 1
    rows, area, prev_where = [], '', None
    seq = texts[i:]
    for k, x in enumerate(seq):
        if x == 'Covered':
            cap = seq[k - 1]
            before = seq[k - 2] if k >= 2 else None
            if before is not None and before != prev_where and before != 'Covered':
                area = before
            where = seq[k + 1] if k + 1 < len(seq) else ''
            rows.append((area, cap, where))
            prev_where = where
    return rows


def gen_coverage(fmt, lang, out_dir):
    t = T[lang]
    rows = [[a, c if fmt == 'md' else esc(c), w if fmt == 'md' else esc(w)] for a, c, w in parse_coverage()]
    return table([t['area'], t['capability'], t['where']], rows, fmt, 'small')


def fmt_prop(p):
    if p['type'] == 'VARIANT':
        return f"{p['name']} = {' / '.join(p['options'])}"
    if p['type'] == 'TEXT':
        return f"{p['name']}: text (\"{str(p['default'])[:24]}\")"
    if p['type'] == 'BOOLEAN':
        return f"{p['name']}: bool ({'on' if p['default'] else 'off'})"
    return f"{p['name']}: {p['type'].lower()}"


def gen_components(fmt, lang, out_dir):
    t = T[lang]
    shelves = {}
    for c in COMPS:
        if c['name'].startswith(('NF2 Icon/', 'NF2 Cursor/')):
            continue
        shelves.setdefault(c['shelf'].replace('Shelf / ', '') or '—', []).append(c)
    out = []
    for shelf, cs in shelves.items():
        out.append(f'#### {shelf}\n' if fmt == 'md' else f'<h4>{esc(shelf)}</h4>')
        rows = []
        for c in cs:
            kind = f"set · {c['variants']} {t['variants']}" if c['kind'] == 'set' else 'component'
            props = '; '.join(fmt_prop(p) for p in c['properties']) or '—'
            row = [f'`{c["name"]}`' if fmt == 'md' else f'<code>{esc(c["name"])}</code>', kind, f"{c['w']}×{c['h']}", props if fmt == 'md' else esc(props)]
            if lang == 'en':
                row.append(c['description'] if fmt == 'md' else esc(c['description']))
            rows.append(row)
        hdr = [t['comp'], t['kind'], t['size'], t['props']] + ([t['desc']] if lang == 'en' else [])
        out.append(table(hdr, rows, fmt, 'components'))
    return '\n'.join(out)


SHELF_CAPTIONS = {
    'en': {'button': 'Button', 'icon-button': 'Icon button', 'segmented-control': 'Segmented control', 'field-search-select': 'Field · Search · Select',
           'checkbox-slider-swatch-badge': 'Checkbox · Slider · Swatch · Badge', 'dock-tab-section-header-panel-header': 'Dock tab · Section header · Panel header',
           'tree-item': 'Tree item', 'kbd-tooltip-dialog-header-zoom-control-logo': 'Kbd · Tooltip · Dialog header · Zoom control · Logo',
           'code-editor': 'Code editor', 'timeline-layers': 'Timeline — layers', 'timeline-frames': 'Timeline — frames',
           'panels-tools-icons': 'Panels & Tools — icons', 'panels-tools-components': 'Panels & Tools — components',
           'rulers-guides-grid': 'Rulers, guides, grid', 'quick-colours-easing': 'Quick colours & easing', 'inputs-menus-breadcrumb': 'Inputs, menus, breadcrumb',
           'canvas-editing-chrome': 'Canvas editing chrome', 'cursors': 'Cursors', 'technical': 'Technical', 'icons-16-and-20-px': 'Icons 16 / 20 px'},
    'ru': {'button': 'Кнопка', 'icon-button': 'Кнопка-иконка', 'segmented-control': 'Сегментированный переключатель', 'field-search-select': 'Поле · Поиск · Выпадающий список',
           'checkbox-slider-swatch-badge': 'Чекбокс · Слайдер · Образец цвета · Бейдж', 'dock-tab-section-header-panel-header': 'Вкладка дока · Заголовок секции · Заголовок панели',
           'tree-item': 'Элемент дерева', 'kbd-tooltip-dialog-header-zoom-control-logo': 'Клавиша · Тултип · Заголовок диалога · Зум · Логотип',
           'code-editor': 'Редактор кода', 'timeline-layers': 'Таймлайн — слои', 'timeline-frames': 'Таймлайн — кадры',
           'panels-tools-icons': 'Панели и инструменты — иконки', 'panels-tools-components': 'Панели и инструменты — компоненты',
           'rulers-guides-grid': 'Линейки, направляющие, сетка', 'quick-colours-easing': 'Быстрые цвета и изинг', 'inputs-menus-breadcrumb': 'Поля, меню, хлебные крошки',
           'canvas-editing-chrome': 'Элементы редактирования на холсте', 'cursors': 'Курсоры', 'technical': 'Технические элементы', 'icons-16-and-20-px': 'Иконки 16 / 20 px'},
}


def gen_shelf_gallery(fmt, lang, out_dir, only=None):
    out = []
    for slug, cap in SHELF_CAPTIONS[lang].items():
        if only and slug not in only:
            continue
        out.append(img(f'docs/illustrations/{slug}.png', fmt, out_dir, cap, 1600))
    return '\n\n'.join(out)


def gen_exports(fmt, lang, out_dir):
    t = T[lang]
    counts = {}
    for d, _, fs in os.walk(os.path.join(ROOT, 'exports')):
        n = len([f for f in fs if f.endswith(('.png', '.svg'))])
        if n:
            counts[os.path.relpath(d, ROOT)] = n
    rows = [[f'`{k}/`' if fmt == 'md' else f'<code>{k}/</code>', v] for k, v in sorted(counts.items())]
    return table([t['folder'], t['count']], rows, fmt, 'narrow')


def gen_outlines(fmt, lang, out_dir):
    sys_path = os.path.join(os.path.dirname(os.path.abspath(__file__)))
    out = []
    files = sorted(f for f in os.listdir(os.path.join(DOCS, 'data', 'outline')) if f.startswith(('interface-', 'panels-')))
    for f in files:
        out.append(f'### {f[:-4]}\n\n```\n{condense(os.path.join(DOCS, "data", "outline", f))}\n```\n')
    return '\n'.join(out)


def condense(path):
    out, seen = [], set()
    for line in open(path):
        s = line.strip()
        if s.startswith('T '):
            tok = s[2:]
        elif s.startswith('I '):
            m = re.match(r'I (\S+(?: \S+)*?)(?: \{(.*?)\})? \d+×\d+(?: (".*"))?$', s)
            if not m:
                tok = s[2:]
            else:
                name = m.group(1).replace('NF2/', '').replace('NF2 Icon/', 'ic:')
                if name.startswith('ic:') or name in ('Divider', 'Menu Separator', 'TL Grid', 'TL Grid Lines'):
                    continue
                tok = '[' + name + (' ' + m.group(3) if m.group(3) else '') + ']'
        elif s.startswith('F ') and re.match(r'F (Row|Body|Content|Col|Column|Group|Frame|Text|Icon|Grid|Spacer|Left|Right|gap|indent|Indent|g)\b', s):
            continue
        elif s.startswith('F '):
            tok = '§' + re.sub(r' \d+×\d+.*', '', s[2:])
        else:
            continue
        if tok in seen and not tok.startswith('§'):
            continue
        seen.add(tok)
        out.append(tok)
    return ' · '.join(out)


GENS = {k[4:].replace('_', '-'): v for k, v in globals().items() if k.startswith('gen_')}


def write_tokens():
    """tokens/tokens.css (Dark on :root, Light on [data-theme=light]), typography.css, tokens.json (W3C format)."""
    out = os.path.join(ROOT, 'tokens')
    os.makedirs(out, exist_ok=True)
    css_name = lambda n: '--nf-' + n.replace('/', '-')

    def css_val(v, mode):
        x = v['values'][mode]
        if isinstance(x, dict):
            return f'var({css_name(x["alias"])})'
        return f'{x}px' if v['type'] == 'FLOAT' else x

    lines = ['/* Generated by tools/build-docs.py from Figma "NF2 · Tokens". Do not edit. */', ':root {']
    lines += [f'  {css_name(v["name"])}: {css_val(v, "Dark")};' for v in NF2['variables']]
    lines += ['}', '', '[data-theme="light"] {']
    lines += [f'  {css_name(v["name"])}: {css_val(v, "Light")};' for v in NF2['variables'] if v['type'] == 'COLOR']
    lines.append('}')
    open(os.path.join(out, 'tokens.css'), 'w').write('\n'.join(lines) + '\n')

    weights = {'Regular': 400, 'Medium': 500, 'Semi Bold': 600, 'Bold': 700}
    ty = ['/* Generated by tools/build-docs.py from Figma NF2 text styles. Do not edit. */']
    js_type = {}
    for s in dump['textStyles']:
        if not s['name'].startswith('NF2/'):
            continue
        cls = 'nf-text-' + re.sub(r'[^a-z0-9]+', '-', s['name'][4:].lower()).strip('-')
        fam = f'"{s["family"]}", ' + ('ui-monospace, monospace' if 'Mono' in s['family'] else 'system-ui, sans-serif')
        ls = s['letterSpacing']
        ls_css = '0' if ls.startswith('0') else (f'{float(ls[:-1]) / 100:.2f}em' if ls.endswith('%') else ls)
        ty.append(f'.{cls} {{ font-family: {fam}; font-weight: {weights.get(s["style"], 400)}; font-size: {s["size"]}px; '
                  f'line-height: {s["lineHeight"]}; letter-spacing: {ls_css};' + (' text-transform: uppercase;' if s['textCase'] == 'UPPER' else '') + ' }')
        js_type[s['name'][4:]] = {'$type': 'typography', '$value': {'fontFamily': s['family'], 'fontWeight': weights.get(s['style'], 400),
                                  'fontSize': f'{s["size"]}px', 'lineHeight': s['lineHeight'], 'letterSpacing': ls_css,
                                  'textCase': s['textCase']}}
    open(os.path.join(out, 'typography.css'), 'w').write('\n'.join(ty) + '\n')

    def tree(mode):
        root = {}
        for v in NF2['variables']:
            node = root
            parts = v['name'].split('/')
            for p in parts[:-1]:
                node = node.setdefault(p, {})
            x = v['values'][mode]
            node[parts[-1]] = {'$type': 'color' if v['type'] == 'COLOR' else 'dimension',
                               '$value': ('{' + x['alias'].replace('/', '.') + '}') if isinstance(x, dict) else (f'{x}px' if v['type'] == 'FLOAT' else x)}
        return root
    json.dump({'dark': tree('Dark'), 'light': tree('Light'), 'typography': js_type}, open(os.path.join(out, 'tokens.json'), 'w'), indent=1, ensure_ascii=False)
    print('tokens/: tokens.css, typography.css, tokens.json')


def audience(src, who):
    """Keep <!--IF:who--> blocks, drop blocks for other audiences."""
    return re.sub(r'<!--IF:(\w+)-->\n?(.*?)<!--END-->\n?', lambda m: m.group(2) if m.group(1) == who else '', src, flags=re.S)


def render(src, fmt, lang, out_dir):
    def rep(m):
        name, _, arg = m.group(1).partition(':')
        if name == 'shelf-gallery' and arg:
            return gen_shelf_gallery(fmt, lang, out_dir, arg.split(','))
        if name == 'img':
            path, _, cap = arg.partition('|')
            return img(path, fmt, out_dir, cap, 1800)
        return GENS[name](fmt, lang, out_dir)
    s = re.sub(r'\{\{GEN:([^}]+)\}\}', rep, src)
    return s.replace('{{GENERATED}}', dump['generated'][:10]).replace('{{FILE_NAME}}', dump['file']).replace('{{TODAY}}', date.today().isoformat())


def main():
    write_tokens()
    src = open(os.path.join(SRC, 'NeoFlash-Design-System.src.md')).read()
    for who, name in (('claude', 'NeoFlash-Design-System.md'), ('codex', 'NeoFlash-Design-System.codex.md')):
        md = render(audience(src, who), 'md', 'en', DOCS)
        open(os.path.join(DOCS, name), 'w').write(md)
        print('docs/' + name, len(md) // 1024, 'kB')
    agents = render(open(os.path.join(SRC, 'AGENTS.src.md')).read(), 'md', 'en', ROOT)
    open(os.path.join(ROOT, 'AGENTS.md'), 'w').write(agents)
    print('AGENTS.md', len(agents) // 1024, 'kB')
    os.makedirs(BUILD, exist_ok=True)
    for lang in ('en', 'ru'):
        tpl = os.path.join(SRC, f'pdf.{lang}.html')
        if not os.path.exists(tpl):
            continue
        out = render(open(tpl).read(), 'html', lang, BUILD)
        dst = os.path.join(BUILD, f'NeoFlash-Design-System-{lang.upper()}.html')
        open(dst, 'w').write(out)
        print(os.path.relpath(dst, ROOT), len(out) // 1024, 'kB')


if __name__ == '__main__':
    main()
