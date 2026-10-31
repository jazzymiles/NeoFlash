// ============================================================================
// UX/UI audit page — plain documentation frames (neutral colours, not part of v1 or v2 systems)
// ============================================================================

const AUDIT = [
  // n, severity, area, problem, decision(v2), marker css-coords on v1 window (or null)
  [1, 'High', 'Иерархия действий', '«Test Movie» — мелкая ссылка в углу, «Publish HTML5…» спрятан в самом низу панели Properties. Главного действия на экране нет.', 'В верхней панели: «Test Movie» (вторичная) и «Publish» (главная, акцентная). Название проекта и признак несохранённых изменений — по центру.', [2400, 14]],
  [2, 'High', 'Таймлайн: управление', 'Сцена, FPS и длительность — вверху, Play и Onion Skin — в нижней строке, зум — в третьей. Транспорт (play) прижат к кнопкам «добавить/удалить слой».', 'Одна панель управления над сеткой: ⏮ ▶ ⏭ + таймкод + сцена / FPS / длительность + зум справа.', [700, 52]],
  [3, 'Med', 'Колонка слоёв', 'Заголовки колонок — три иконки без подписей и тултипов; под тремя слоями ~60% пустого места; кнопки «+ / корзина» оторваны от списка.', 'Кнопки «добавить / удалить слой» — в шапке колонки слоёв; строки 32px, цветная точка слоя, lock/eye с тултипами; ghost-строка «Add layer».', [150, 109]],
  [4, 'High', 'Нижняя строка таймлайна', '«Onion Skin ☐ Outlines ☐ Before [3] After [3] ■ Anchor ──○── Opacity» — чекбоксы, поля и слайдер в одной линии без группировки; порядок «Anchor [слайдер] Opacity» читается двусмысленно.', 'Onion skin — одна кнопка-переключатель с поповером: Before / After (степперы), Outlines, Anchor, Opacity (слайдер с подписью и значением).', [1000, 305]],
  [5, 'High', 'Палитра инструментов', '20 иконок 28px подряд, без подписей и горячих клавиш; названия групп (Select / Draw / View) — мелкий серый текст; tooltip не предусмотрен.', 'Рейл 56px: группы разделены линиями, активный инструмент — акцентный, tooltip «Название + клавиша». Zoom/Hand — в плавающей панели над сценой.', [68, 520]],
  [6, 'Med', 'Типографика и контраст', 'Базовый текст 14px, вторичный 11–12px. Неактивный текст #9CA2B0 на #EAECF1 даёт контраст 2.16:1 (норма AA — 4.5:1); «muted» #666D80 — 4.58:1, на грани.', 'Шкала 11/12/13/14/16/18, минимум 12px для текста. Токены text/primary, secondary, tertiary, disabled — все ≥ 4.5:1 (кроме disabled, он исключён нормой).', null],
  [7, 'Med', 'Поверхности и рамки', 'Каждая кнопка, поле, вкладка и панель обведены рамкой 1px #6D7690. Панель, поле и кнопка выглядят одинаково — нет уровней глубины, много визуального шума.', 'Уровни поверхностей bg/app → panel → raised → field; рамки только как разделители; радиусы 6 / 8 / 12 из токенов.', null],
  [8, 'Med', 'Поля свойств', 'FPS / Frames / Alpha выглядят как кнопки (серо-синий фон), Name / W / H — белые поля. Нет единиц измерения, степперов, scrub по лейблу. Подписи не выровнены.', 'Единый Field: префикс-лейбл внутри (W, H, FPS), единицы в значении, состояния hover / focus / disabled. Размер — парой W × H со значком связи.', [2420, 1180]],
  [9, 'Med', 'Библиотека', 'Поиск + Clear + Sort + A–Z занимают две строки; дерево — 4 строки; остальные ~500px пустые. Нет пустого состояния.', 'Поиск со встроенным clear, сортировка — иконка-меню в шапке; футер с действиями; место под превью символа и empty state.', [2420, 520]],
  [10, 'Low', 'Нижний док', 'Actions / Accessibility / Output — отдельная панель 186px, постоянно открытая, при этом показывает одну строку текста.', 'Один док с вкладками: Timeline · Actions · Output · Accessibility. Output открывается по требованию.', [1200, 1330]],
  [11, 'High', 'Диалог «Unsaved changes»', 'Save / Discard / Cancel равнозначны: нет главной кнопки, деструктивное «Discard» не выделено; заголовок серый по центру, в другом диалоге — белый слева (два стиля одного компонента).', 'Единый Dialog: Save — primary, Discard — danger-ghost слева, Cancel — secondary. Подпись последствий, Esc / Enter.', null],
  [12, 'Med', 'Диалог «Convert to Symbol»', 'Восемь полей в одну колонку; ActionScript linkage раскрыт по умолчанию; disabled-поля (Identifier, Class) читаются как активные; Registration 3×3 без пояснения; Cancel и Convert одного веса.', 'Основное сверху (Name, Type — сегменты, Folder), linkage свёрнут как Advanced, Registration с подписью, Convert — primary.', null],
  [13, 'Low', 'Статус-строка', 'Горячие клавиши и подсказки в одной серой строке 12px; в конце — битый глиф «⌘□ Test Movie».', 'Статус-бар 24px: подсказки состояния слева, горячие клавиши — чипами (F5 / F6 / F7) справа.', [900, 1228]],
  [14, 'Med', 'Доступность и темы', 'Выбор показан цветом и рамкой 1px, фокус едва заметен, зоны клика 24–28px, тёмной темы нет.', 'Focus-ring 2px, hit-area ≥ 32px, две темы (Dark / Light) на переменных одной коллекции.', null],
];

function auditBuild(page, v1Main) {
  CTX = null;
  const INK = '#15171C', MUTED = '#5E6573', LINE = '#E1E4EA', BG = '#FFFFFF';
  const sevColor = { High: ['#FDE8E8', '#B42318'], Med: ['#FEF3E2', '#B54708'], Low: ['#E8F0FE', '#2B50C4'] };
  const T = (s, size, color, o) => mkText(s, Object.assign({ font: 'i400', size, color }, o || {}));

  const root = mkFrame('Audit / v1 → v2', { dir: 'v', gap: 40, pad: 64, w: 2560, fill: '#F6F7F9' });
  page.appendChild(root);

  add(root, T('Аудит интерфейса NeoFlash v1', 44, INK, { font: 'i600' }));
  add(root, T('Что мешает работе в текущем интерфейсе и как это решается в версии 2. Номера на скриншоте совпадают с номерами в таблице.', 16, MUTED, { w: 1400 }));

  // summary strip
  const sum = mkFrame('Summary', { dir: 'h', gap: 20 });
  add(root, sum);
  const stat = (n, label, col) => {
    const c = mkFrame('Stat', { dir: 'v', gap: 4, pad: [16, 24], fill: BG, stroke: LINE, r: 12, w: 260 });
    add(c, T(String(n), 36, col, { font: 'i600' }));
    add(c, T(label, 13, MUTED));
    add(sum, c);
  };
  const cnt = k => AUDIT.filter(a => a[1] === k).length;
  stat(AUDIT.length, 'находок всего', INK);
  stat(cnt('High'), 'критичных (High)', '#B42318');
  stat(cnt('Med'), 'средних (Med)', '#B54708');
  stat(cnt('Low'), 'низких (Low)', '#2B50C4');
  stat('2.16:1', 'контраст disabled-текста (норма 4.5:1)', '#B42318');
  stat('20', 'кнопок-инструментов без подписей и tooltip', INK);

  // body: annotated screenshot + table
  const body = mkFrame('Body', { dir: 'h', gap: 48, cross: 'MIN' });
  add(root, body);

  // annotated v1 (vector clone, scaled down)
  const left = mkFrame('Annotated v1', { dir: 'v', gap: 16 });
  add(body, left);
  add(left, T('v1 — карта проблем', 20, INK, { font: 'i600' }));
  const holder = mkFrame('v1 preview', { w: 1280, h: 720, fill: '#FFFFFF', stroke: LINE, r: 6, clip: true });
  add(left, holder);
  const clone = v1Main.clone();
  clone.name = 'v1 preview (clone, 50%)';
  holder.appendChild(clone);
  clone.rescale(0.5);
  clone.x = 0; clone.y = 0;
  AUDIT.forEach(a => {
    if (!a[5]) return;
    const m = mkFrame('Marker ' + a[0], { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 28, h: 28, fill: '#E5484D', stroke: '#FFFFFF', sw: 2, r: 14 });
    add(m, T(String(a[0]), 13, '#FFFFFF', { font: 'i600' }));
    add(holder, m, { x: a[5][0] / 2 - 14, y: a[5][1] / 2 - 14 });
  });
  add(left, T('Без маркера — проблема «сквозная» (типографика, рамки, диалоги, доступность).', 13, MUTED, { w: 1280 }));

  // findings table
  const right = mkFrame('Findings', { dir: 'v', gap: 10, w: 1100 });
  add(body, right);
  add(right, T('Находки и решения', 20, INK, { font: 'i600' }));
  AUDIT.forEach(a => {
    const row = mkFrame('Finding ' + a[0], { dir: 'h', gap: 14, pad: 14, w: 1100, fill: BG, stroke: LINE, r: 10 });
    add(right, row);
    const badge = mkFrame('Badge', { dir: 'h', main: 'CENTER', cross: 'CENTER', w: 28, h: 28, fill: '#E5484D', r: 14 });
    add(badge, T(String(a[0]), 13, '#FFFFFF', { font: 'i600' }));
    add(row, badge);
    const col = mkFrame('Text', { dir: 'v', gap: 6, w: 1010 });
    add(row, col, { grow: true });
    const head = mkFrame('Head', { dir: 'h', gap: 10, cross: 'CENTER' });
    add(col, head);
    add(head, T(a[2], 15, INK, { font: 'i600' }));
    const chip = mkFrame('Severity', { dir: 'h', pad: [2, 8], fill: sevColor[a[1]][0], r: 10 });
    add(chip, T(a[1], 11, sevColor[a[1]][1], { font: 'i600' }));
    add(head, chip);
    add(col, T('Проблема: ' + a[3], 13, MUTED, { w: 1000, lh: 19 }), {});
    add(col, T('v2: ' + a[4], 13, INK, { w: 1000, lh: 19 }), {});
  });

  // principles
  const pr = mkFrame('Principles', { dir: 'v', gap: 12, pad: 28, w: 2432, fill: BG, stroke: LINE, r: 12 });
  add(root, pr);
  add(pr, T('Принципы v2', 24, INK, { font: 'i600' }));
  const grid = mkFrame('Principles grid', { dir: 'h', gap: 24 });
  add(pr, grid);
  [
    ['Один главный путь', 'На экране всегда ясно главное действие: Publish, Play, Save. Остальное — вторичное и не конкурирует по весу.'],
    ['Группировка по задаче', 'Транспорт, onion skin, зум, слои — собраны в логические группы, а не разбросаны по строкам.'],
    ['Поверхности вместо рамок', 'Глубину задают уровни фона, а не рамка у каждого контрола.'],
    ['Системность', 'Свои переменные (цвет, отступ, радиус), текстовые стили и компоненты NF2; две темы на одной коллекции.'],
    ['Доступность', 'Контраст ≥ 4.5:1, focus-ring 2px, зоны клика ≥ 32px, подсказки с горячими клавишами.'],
    ['Изоляция от v1', 'Ни одна переменная, стиль или компонент v2 не ссылается на v1 — изменения в одной версии не затрагивают другую.'],
  ].forEach(p => {
    const c = mkFrame('Principle', { dir: 'v', gap: 6, w: 380 });
    add(c, T(p[0], 15, INK, { font: 'i600' }));
    add(c, T(p[1], 13, MUTED, { w: 380, lh: 19 }));
    add(grid, c);
  });
  return root;
}
