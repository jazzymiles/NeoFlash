'use strict';

(() => {
  const labels = { implemented: 'Реализовано', partial: 'Частично', planned: 'В планах' };
  const colors = { implemented: '#147b63', partial: '#b98124', total: '#426986' };
  const repository = 'https://github.com/jazzymiles/NeoFlash';
  const dateFormat = new Intl.DateTimeFormat('ru-RU', { dateStyle: 'medium', timeZone: 'UTC' });
  const shortDateFormat = new Intl.DateTimeFormat('ru-RU', { day: '2-digit', month: '2-digit', timeZone: 'UTC' });
  const byId = id => document.getElementById(id);
  const percent = counts => `${counts.total ? (100 * counts.implemented / counts.total).toFixed(1).replace('.', ',') : '0'}%`;
  const date = value => dateFormat.format(new Date(value.length === 10 ? `${value}T00:00:00Z` : value));
  const pathUrl = (revision, path) => `${repository}/blob/${encodeURIComponent(revision)}/${path.split('/').map(encodeURIComponent).join('/')}`;

  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function link(text, href, className) {
    const node = element('a', className, text);
    node.href = href;
    return node;
  }

  function svgElement(tag, attributes, text) {
    const node = document.createElementNS('http://www.w3.org/2000/svg', tag);
    Object.entries(attributes || {}).forEach(([key, value]) => node.setAttribute(key, String(value)));
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function countsFor(features) {
    const counts = { implemented: 0, partial: 0, planned: 0, total: features.length };
    features.forEach(feature => { counts[feature.status] += 1; });
    return counts;
  }

  function validateData(data) {
    if (data.schemaVersion !== 1 || !Array.isArray(data.categories) || !Array.isArray(data.features) || !Array.isArray(data.history)) {
      throw new Error('Неизвестный формат данных.');
    }
    if (!/^[a-f0-9]{7,40}$/.test(data.revision) || Number.isNaN(Date.parse(data.generatedAt))) {
      throw new Error('В снимке отсутствует корректная дата или ревизия.');
    }
    const categories = new Set(data.categories.map(category => category.id));
    data.features.forEach(feature => {
      if (!categories.has(feature.category) || !Object.hasOwn(labels, feature.status) || !Array.isArray(feature.evidence)) {
        throw new Error('Некорректная запись реестра.');
      }
    });
    return data;
  }

  function fillCategorySelect(select, categories) {
    categories.forEach(category => {
      const option = element('option', '', category.title);
      option.value = category.id;
      select.append(option);
    });
  }

  function renderMetadata(data) {
    byId('snapshot-label').textContent = `Снимок ${date(data.generatedAt)} · ${data.features.length} пунктов · методика v${data.methodology.version}`;
    const revision = byId('revision-link');
    revision.textContent = `${data.revision.slice(0, 7)} ↗`;
    revision.href = `${repository}/commit/${data.revision}`;
    revision.setAttribute('aria-label', `Коммит снимка ${data.revision.slice(0, 7)}`);
    document.querySelectorAll('[data-method]').forEach(node => { node.href = pathUrl(data.revision, 'docs/progress/README.md'); });
    document.querySelectorAll('[data-compat]').forEach(node => { node.href = pathUrl(data.revision, 'docs/COMPATIBILITY.md'); });
    document.querySelectorAll('[data-details]').forEach(node => { node.href = pathUrl(data.revision, 'docs/progress/DETAILS.md'); });
  }

  function renderCategories(data) {
    data.categories.forEach(category => {
      const counts = countsFor(data.features.filter(feature => feature.category === category.id));
      const card = element('article', 'category-card');
      const heading = element('h3');
      heading.append(link(category.title, pathUrl(data.revision, 'docs/progress/DETAILS.md')));
      card.append(heading);
      const metric = element('p', 'metric');
      metric.append(element('span', '', counts.implemented), element('span', 'denominator', `/ ${counts.total}`), element('span', 'percent', percent(counts)));
      metric.setAttribute('aria-label', `Реализовано ${counts.implemented} из ${counts.total}, ${percent(counts)}`);
      card.append(metric);
      const bar = element('div', 'progress-bar');
      bar.setAttribute('aria-hidden', 'true');
      for (const [status, className] of [['implemented', 'done'], ['partial', 'part']]) {
        const segment = element('span', className);
        segment.style.width = `${counts.total ? 100 * counts[status] / counts.total : 0}%`;
        bar.append(segment);
      }
      const breakdown = element('div', 'category-breakdown');
      breakdown.append(element('span', '', `Частично: ${counts.partial}`), element('span', '', `В планах: ${counts.planned}`));
      card.append(bar, breakdown, element('p', 'category-description', category.description));
      const features = data.features.filter(feature => feature.category === category.id);
      const working = features.filter(feature => feature.status === 'implemented').slice(0, 2);
      const remaining = features.filter(feature => feature.status === 'planned').slice(0, 2);
      if (working.length) card.append(element('p', 'category-examples', `Есть: ${working.map(feature => feature.title).join('; ')}.`));
      else if (counts.partial) card.append(element('p', 'category-examples', 'Работает ограниченный набор; полного покрытия пунктов пока нет.'));
      if (remaining.length) card.append(element('p', 'category-examples pending', `Впереди: ${remaining.map(feature => feature.title).join('; ')}.`));
      byId('categories').append(card);
    });
  }

  function renderCurrent(data) {
    renderCategories(data);
  }

  function historyReadout(point, counts) {
    return `${date(point.date)} · реализовано ${counts.implemented}/${counts.total} (${percent(counts)}) · частично ${counts.partial} · в планах ${counts.planned} · v${point.methodologyVersion}`;
  }

  function renderChart(category, history) {
    const card = element('article', 'chart-card');
    const title = element('h3', '', category.title);
    title.id = `chart-title-${category.id}`;
    card.append(title, element('p', 'chart-description', category.description));
    const points = history.filter(point => point.categories[category.id]);
    if (!points.length) {
      card.append(element('p', 'fine-print', 'Для этого направления пока нет снимков.'));
      return card;
    }
    const width = 520, height = 210;
    const left = 37, right = 18, top = 15, bottom = 34;
    const plotWidth = width - left - right, plotHeight = height - top - bottom;
    const times = points.map(point => Date.parse(`${point.date}T00:00:00Z`));
    const first = times[0], last = times[times.length - 1];
    const max = Math.max(1, ...points.map(point => point.categories[category.id].total));
    const ceiling = Math.ceil(max / 4) * 4;
    const x = time => first === last ? left + plotWidth / 2 : left + (time - first) / (last - first) * plotWidth;
    const y = count => top + plotHeight * (1 - count / ceiling);
    const svg = svgElement('svg', { viewBox: `0 0 ${width} ${height}`, role: 'group', 'aria-labelledby': title.id });
    svg.append(svgElement('title', {}, `${category.title}: реализованные, частичные и все пункты реестра по дням`));
    for (let index = 0; index <= 4; index += 1) {
      const value = ceiling * index / 4;
      svg.append(svgElement('line', { x1: left, y1: y(value), x2: width - right, y2: y(value), class: 'grid-line' }));
      svg.append(svgElement('text', { x: left - 10, y: y(value) + 3, 'text-anchor': 'end' }, value));
    }
    const tickTimes = first === last ? [first] : [first, last];
    tickTimes.forEach(time => {
      const anchor = first === last ? 'middle' : time === first ? 'start' : 'end';
      svg.append(svgElement('text', { x: x(time), y: height - 10, 'text-anchor': anchor }, shortDateFormat.format(new Date(time))));
    });
    // A changed denominator is meaningful data. A changed methodology is a break,
    // so never connect snapshots across incompatible versions.
    const groups = [];
    let group = [];
    points.forEach((point, index) => {
      if (index && point.methodologyVersion !== points[index - 1].methodologyVersion) {
        groups.push(group);
        group = [];
      }
      group.push({ point, time: times[index] });
    });
    groups.push(group);
    for (const series of ['total', 'partial', 'implemented']) {
      groups.forEach(entries => {
        if (entries.length > 1) svg.append(svgElement('polyline', {
          class: `series ${series}`,
          points: entries.map(({ point, time }) => `${x(time)},${y(point.categories[category.id][series])}`).join(' ')
        }));
      });
      points.forEach((point, index) => {
        svg.append(svgElement('circle', { cx: x(times[index]), cy: y(point.categories[category.id][series]), r: 3.5, fill: colors[series], class: 'point-marker' }));
      });
    }
    const readout = element('output', 'chart-readout');
    const latest = points[points.length - 1];
    readout.textContent = historyReadout(latest, latest.categories[category.id]);
    readout.setAttribute('aria-live', 'polite');
    points.forEach((point, index) => {
      const counts = point.categories[category.id];
      const text = historyReadout(point, counts);
      const hit = svgElement('rect', { x: x(times[index]) - 7, y: top - 5, width: 14, height: plotHeight + 10, class: 'point-hit', tabindex: 0, role: 'img', 'aria-label': text });
      hit.append(svgElement('title', {}, text));
      for (const event of ['pointerenter', 'focus', 'click']) hit.addEventListener(event, () => { readout.textContent = text; });
      svg.append(hit);
    });
    card.append(svg, readout);
    return card;
  }

  function renderHistoryRows(data, history) {
    const category = byId('history-category').value;
    const body = byId('history-rows');
    body.replaceChildren();
    [...history].reverse().forEach(point => {
      const counts = point.categories[category];
      if (!counts) return;
      const row = element('tr');
      row.append(element('td', '', date(point.date)));
      const revision = element('td');
      revision.append(link(point.revision.slice(0, 7), `${repository}/commit/${encodeURIComponent(point.revision)}`));
      row.append(revision);
      for (const key of ['implemented', 'partial', 'planned', 'total']) row.append(element('td', '', counts[key]));
      row.append(element('td', '', percent(counts)), element('td', '', `v${point.methodologyVersion}`));
      body.append(row);
    });
  }

  function renderHistory(data) {
    const history = [...data.history].sort((a, b) => a.date.localeCompare(b.date));
    fillCategorySelect(byId('history-category'), data.categories);
    byId('history-note').textContent = history.length === 1
      ? `Первый снимок: ${date(history[0].date)} Пока на графиках по одной точке; следующие дни разработки продолжат историю.`
      : history.length
        ? `${history.length} снимков · ${date(history[0].date)} — ${date(history[history.length - 1].date)}. Пропуски между датами означают отсутствие снимка, а не нулевой прогресс.`
        : 'История появится после первого снимка. Исторические значения не выдумываются.';
    data.categories.forEach(category => byId('charts').append(renderChart(category, history)));
    renderHistoryRows(data, history);
    byId('history-category').addEventListener('change', () => renderHistoryRows(data, history));
  }

  async function start() {
    try {
      const response = await fetch('./progress.json', { cache: 'no-cache' });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const data = validateData(await response.json());
      renderMetadata(data);
      if (document.body.dataset.page === 'history') renderHistory(data);
      else renderCurrent(data);
    } catch (error) {
      byId('snapshot-label').textContent = 'Снимок недоступен';
      const message = byId('load-error');
      message.hidden = false;
      message.append(document.createTextNode('Не удалось загрузить статистику. Обновите страницу или откройте '), link('исходные данные', 'progress.json'), document.createTextNode('.'));
      console.error('NeoFlash progress:', error);
    }
  }

  start();
})();
