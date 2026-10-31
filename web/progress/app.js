'use strict';

(() => {
  const labels = { implemented: 'Closed', partial: 'Partial', planned: 'Planned' };
  const itemLabels = { working: 'Working', partial: 'Partial', planned: 'Planned' };
  const colors = { implemented: '#147b63', partial: '#b98124', needs_review: '#83629d', total: '#426986' };
  const targetStatuses = [['working', 'Working'], ['partial', 'Partial'], ['missing', 'Missing'], ['needs_review', 'Needs review']];
  const repository = 'https://github.com/jazzymiles/NeoFlash';
  const dateFormat = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeZone: 'UTC' });
  const shortDateFormat = new Intl.DateTimeFormat('en-US', { day: '2-digit', month: '2-digit', timeZone: 'UTC' });
  const byId = id => document.getElementById(id);
  const number = value => Number(value).toLocaleString('en-US');
  const isActionScript = id => id === 'as2' || id === 'as3';
  const share = (counts, ...keys) => `${counts.total ? (100 * keys.reduce((sum, key) => sum + counts[key], 0) / counts.total).toFixed(1) : '0.0'}%`;
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

  function validCounts(counts, keys) {
    return counts && keys.every(key => Number.isInteger(counts[key]) && counts[key] >= 0)
      && keys.reduce((sum, key) => sum + counts[key], 0) === counts.total;
  }

  function validateData(data) {
    if (data.schemaVersion !== 3 || !Array.isArray(data.categories) || !Array.isArray(data.features) || !Array.isArray(data.history)) {
      throw new Error('Unknown data format.');
    }
    if (!/^[a-f0-9]{7,40}$/.test(data.revision) || Number.isNaN(Date.parse(data.generatedAt))) {
      throw new Error('The snapshot has no valid date or revision.');
    }
    const categories = new Set(data.categories.map(category => category.id));
    data.features.forEach(feature => {
      if (!categories.has(feature.category) || !Object.hasOwn(labels, feature.status) || !Array.isArray(feature.evidence)
        || !Array.isArray(feature.items) || !feature.items.every(item => Object.hasOwn(itemLabels, item.status) && typeof item.title === 'string')) {
        throw new Error('Invalid register entry.');
      }
    });
    const keys = targetStatuses.map(([key]) => key);
    for (const view of Object.values(data.actionscript?.targets || {})) {
      const rows = [...view.stages.map(stage => stage.counts), view.now, view.target, view.total, ...view.outside.map(item => item.counts)];
      if (!rows.every(counts => validCounts(counts, keys)) || view.stages.some((stage, index) => stage.order !== index + 1)) {
        throw new Error('Invalid ActionScript target counts.');
      }
    }
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
    byId('snapshot-label').textContent = `Snapshot ${date(data.generatedAt)} (UTC) · methodology v${data.methodology.version}`;
    const revision = byId('revision-link');
    revision.textContent = `${data.revision.slice(0, 7)} ↗`;
    revision.href = `${repository}/commit/${data.revision}`;
    revision.setAttribute('aria-label', `Snapshot commit ${data.revision.slice(0, 7)}`);
    document.querySelectorAll('[data-method]').forEach(node => { node.href = pathUrl(data.revision, 'docs/progress/README.md'); });
    document.querySelectorAll('[data-compat]').forEach(node => { node.href = pathUrl(data.revision, 'docs/COMPATIBILITY.md'); });
    document.querySelectorAll('[data-details]').forEach(node => { node.href = pathUrl(data.revision, 'docs/progress/DETAILS.md'); });
  }

  function targetTable(view) {
    const wrapper = element('div', 'table-scroll priority-scroll');
    wrapper.tabIndex = 0;
    wrapper.setAttribute('role', 'region');
    wrapper.setAttribute('aria-label', `${view.title} by Flash Player area`);
    const table = element('table', 'priority-table target-table');
    const head = element('thead'), heading = element('tr');
    for (const label of ['Flash Player area', 'Entries', ...targetStatuses.map(([, label]) => label), 'Working or partial']) {
      const cell = element('th', '', label);
      cell.scope = 'col';
      heading.append(cell);
    }
    head.append(heading);
    const body = element('tbody');
    const row = (title, purpose, counts, className) => {
      const line = element('tr', className);
      const cell = element('th', 'priority-topic');
      cell.scope = 'row';
      cell.append(element('span', 'priority-title', title));
      if (purpose) cell.append(element('span', 'priority-purpose', purpose));
      const bar = element('div', 'progress-bar');
      bar.setAttribute('aria-hidden', 'true');
      for (const [key, name] of [['working', 'done'], ['partial', 'part']]) {
        const segment = element('span', name);
        segment.style.width = `${counts.total ? 100 * counts[key] / counts.total : 0}%`;
        bar.append(segment);
      }
      cell.append(bar);
      line.append(cell, element('td', 'priority-count total', number(counts.total)));
      for (const [key] of targetStatuses) line.append(element('td', `priority-count ${key}`, `${number(counts[key])}`));
      line.append(element('td', 'priority-count share', share(counts, 'working', 'partial')));
      body.append(line);
    };
    view.stages.forEach(stage => row(`${stage.order}. ${stage.title}`, stage.purpose, stage.counts, stage.now ? 'priority-foundation' : ''));
    const lastNow = Math.max(...view.stages.filter(stage => stage.now).map(stage => stage.order));
    row(`Now: areas 1–${lastNow}`, '', view.now, 'target-subtotal');
    row('All Flash Player areas', '', view.target, 'target-subtotal');
    table.append(head, body);
    wrapper.append(table);
    return wrapper;
  }

  function renderActionScript(data) {
    const audit = data.actionscript;
    for (const language of ['AS3', 'AS2']) {
      const view = audit.targets[language];
      if (!view) continue;
      const card = element('article', 'category-card actionscript-card');
      const heading = element('h3');
      heading.append(link(view.title, pathUrl(data.revision, 'docs/progress/ACTIONSCRIPT.md')));
      card.append(heading);
      const statuses = element('dl', 'catalogue-statuses');
      const headline = [['working', 'Working now (areas 1–6)', share(view.now, 'working')], ['partial', 'Working or partial (areas 1–6)', share(view.now, 'working', 'partial')],
        ['missing', 'Missing (areas 1–6)', number(view.now.missing)], ['needs_review', 'Needs review (areas 1–6)', number(view.now.needs_review)]];
      for (const [key, label, value] of headline) {
        const item = element('div', `catalogue-status ${key}`);
        item.append(element('dt', '', label), element('dd', '', value));
        statuses.append(item);
      }
      card.append(statuses, targetTable(view));
      const outside = view.outside.map(item => `${item.title} ${number(item.counts.total)}`).join(' · ');
      card.append(element('p', 'catalogue-scope', `Outside the target, never in a percentage: ${outside}.`));
      byId('actionscript-categories').append(card);
    }
    const note = byId('actionscript-note');
    note.replaceChildren(document.createTextNode(`${audit.definitions.replace(/\*\*/g, '')} Last status review: ${date(audit.lastReview)}. `),
      link('Per-declaration statuses and evidence', pathUrl(data.revision, 'docs/progress/ACTIONSCRIPT.md')));
  }

  function itemCounts(features) {
    const counts = { working: 0, partial: 0, planned: 0, total: 0 };
    features.forEach(feature => feature.items.forEach(item => { counts[item.status] += 1; counts.total += 1; }));
    return counts;
  }

  function renderCategories(data) {
    data.categories.filter(category => !isActionScript(category.id)).forEach(category => {
      const features = data.features.filter(feature => feature.category === category.id);
      const groups = countsFor(features);
      const items = itemCounts(features);
      const card = element('article', 'category-card');
      const heading = element('h3');
      heading.append(link(category.title, pathUrl(data.revision, 'docs/progress/DETAILS.md')));
      card.append(heading);
      const statuses = element('dl', 'product-statuses');
      for (const [status, title] of [['working', 'Working'], ['partial', 'Partial'], ['planned', 'Planned']]) {
        const item = element('div', `product-status ${status === 'working' ? 'implemented' : status}`);
        item.append(element('dt', '', `${title} functions`), element('dd', '', number(items[status])));
        statuses.append(item);
      }
      card.append(statuses, element('p', 'product-closed-share', `${items.working} of ${items.total} functions work · groups: ${groups.implemented} closed, ${groups.partial} partial, ${groups.planned} planned`));
      const bar = element('div', 'progress-bar');
      bar.setAttribute('aria-hidden', 'true');
      for (const [status, className] of [['working', 'done'], ['partial', 'part']]) {
        const segment = element('span', className);
        segment.style.width = `${items.total ? 100 * items[status] / items.total : 0}%`;
        bar.append(segment);
      }
      card.append(bar, element('p', 'category-description', category.description));
      const details = element('details', 'product-partial-details');
      details.append(element('summary', '', `Functions by group (${features.length} groups)`));
      const list = element('ul', 'product-partial-list');
      for (const feature of features) {
        const entry = element('li');
        entry.append(element('strong', 'product-partial-title', `${feature.title} · ${labels[feature.status]}`));
        for (const item of feature.items) {
          const line = element('p', `product-item ${item.status}`);
          line.append(element('span', 'product-detail-label', `${itemLabels[item.status]}: `), document.createTextNode(item.title));
          entry.append(line);
        }
        list.append(entry);
      }
      details.append(list, link('Full scopes, limits and evidence →', pathUrl(data.revision, 'docs/progress/DETAILS.md'), 'text-link'));
      card.append(details);
      byId('categories').append(card);
    });
  }

  function renderCurrent(data) {
    renderActionScript(data);
    renderCategories(data);
  }

  function methodologyFor(point, category) {
    return point.categories[category]?.methodologyVersion || point.methodologyVersion;
  }

  function historyFor(category, history) {
    return history.filter(point => point.categories[category] && (!isActionScript(category) || point.categories[category].metric === 'flash-player-target'));
  }

  function historyReadout(point, counts, category) {
    if (isActionScript(category)) return `${date(point.date)} · working ${number(counts.implemented)} · partial ${number(counts.partial)} · missing ${number(counts.planned)} · needs review ${number(counts.needs_review || 0)} · Flash Player entries ${number(counts.total)}`;
    if (counts.metric === 'functions') return `${date(point.date)} · working functions ${counts.implemented} · partial ${counts.partial} · planned ${counts.planned} · of ${counts.total} · v${methodologyFor(point, category)}`;
    return `${date(point.date)} · closed groups ${counts.implemented} · partial ${counts.partial} · planned ${counts.planned} · of ${counts.total} groups · v${methodologyFor(point, category)}`;
  }

  function renderChart(category, history) {
    const card = element('article', 'chart-card');
    const title = element('h3', '', category.title);
    title.id = `chart-title-${category.id}`;
    card.append(title, element('p', 'chart-description', isActionScript(category.id) ? 'Declarations in the eight Flash Player areas, recounted from each day\'s catalogue. Green is working, amber partial.' : `${category.description} Groups until methodology v3, functions from v3.`));
    const points = historyFor(category.id, history);
    if (!points.length) {
      card.append(element('p', 'fine-print', 'No snapshots for this area yet.'));
      return card;
    }
    const width = 520, height = 210;
    const left = 60, right = 18, top = 15, bottom = 34;
    const plotWidth = width - left - right, plotHeight = height - top - bottom;
    const times = points.map(point => Date.parse(`${point.date}T00:00:00Z`));
    const first = times[0], last = times[times.length - 1];
    const max = Math.max(1, ...points.map(point => point.categories[category.id].total));
    const ceiling = Math.ceil(max / 4) * 4;
    const x = time => first === last ? left + plotWidth / 2 : left + (time - first) / (last - first) * plotWidth;
    const y = count => top + plotHeight * (1 - count / ceiling);
    const svg = svgElement('svg', { viewBox: `0 0 ${width} ${height}`, role: 'group', 'aria-labelledby': title.id });
    svg.append(svgElement('title', {}, isActionScript(category.id) ? `${category.title}: working, partial, needs review and total Flash Player entries by day` : `${category.title}: working and partial functions (capability groups before v3) by day`));
    for (let index = 0; index <= 4; index += 1) {
      const value = ceiling * index / 4;
      svg.append(svgElement('line', { x1: left, y1: y(value), x2: width - right, y2: y(value), class: 'grid-line' }));
      svg.append(svgElement('text', { x: left - 10, y: y(value) + 3, 'text-anchor': 'end' }, number(value)));
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
      if (index && methodologyFor(point, category.id) !== methodologyFor(points[index - 1], category.id)) {
        groups.push(group);
        group = [];
      }
      group.push({ point, time: times[index] });
    });
    groups.push(group);
    for (const series of isActionScript(category.id) ? ['total', 'needs_review', 'partial', 'implemented'] : ['total', 'partial', 'implemented']) {
      groups.forEach(entries => {
        if (entries.length > 1) svg.append(svgElement('polyline', {
          class: `series ${series}`,
          points: entries.map(({ point, time }) => `${x(time)},${y(point.categories[category.id][series] || 0)}`).join(' ')
        }));
      });
      points.forEach((point, index) => {
        svg.append(svgElement('circle', { cx: x(times[index]), cy: y(point.categories[category.id][series] || 0), r: 3.5, fill: colors[series], class: 'point-marker' }));
      });
    }
    const readout = element('output', 'chart-readout');
    const latest = points[points.length - 1];
    readout.textContent = historyReadout(latest, latest.categories[category.id], category.id);
    readout.setAttribute('aria-live', 'polite');
    points.forEach((point, index) => {
      const counts = point.categories[category.id];
      const text = historyReadout(point, counts, category.id);
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
    const catalogue = isActionScript(category);
    byId('history-implemented-label').textContent = catalogue ? 'Working' : 'Working / closed';
    byId('history-remaining-label').textContent = catalogue ? 'Missing' : 'Planned';
    byId('history-review-label').hidden = !catalogue;
    byId('history-share-label').textContent = catalogue ? 'Working or partial' : 'Working share';
    byId('history-table-note').textContent = catalogue
      ? 'Declarations in the eight Flash Player areas; AIR, Flex and components are outside the target. Every day is recounted from that day\'s committed catalogue with the current rules.'
      : 'From methodology v3 the counts are user-visible functions inside each capability group; earlier rows count whole groups (closed / partial / planned).';
    const body = byId('history-rows');
    body.replaceChildren();
    [...historyFor(category, history)].reverse().forEach(point => {
      const counts = point.categories[category];
      if (!counts) return;
      const row = element('tr');
      row.append(element('td', '', date(point.date)));
      const revision = element('td');
      revision.append(link(point.revision.slice(0, 7), `${repository}/commit/${encodeURIComponent(point.revision)}`));
      row.append(revision);
      for (const key of ['implemented', 'partial', 'planned']) row.append(element('td', '', number(counts[key])));
      if (catalogue) row.append(element('td', '', number(counts.needs_review || 0)));
      row.append(element('td', '', number(counts.total)));
      row.append(element('td', '', catalogue ? share(counts, 'implemented', 'partial') : share(counts, 'implemented')));
      row.append(element('td', '', `v${methodologyFor(point, category)}`));
      body.append(row);
    });
  }

  function renderHistory(data) {
    const history = [...data.history].sort((a, b) => a.date.localeCompare(b.date));
    fillCategorySelect(byId('history-category'), data.categories);
    byId('history-note').textContent = history.length === 1
      ? `One recorded day: ${date(history[0].date)}. Later development days will extend the history.`
      : history.length
        ? `${history.length} snapshots · ${date(history[0].date)} — ${date(history[history.length - 1].date)}. Gaps mean no snapshot was recorded, not zero progress.`
        : 'History begins with the first recorded snapshot. Earlier values are not estimated.';
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
      byId('snapshot-label').textContent = 'Snapshot unavailable';
      const message = byId('load-error');
      message.hidden = false;
      message.append(document.createTextNode('Could not load progress data. Refresh the page or open the '), link('raw data', 'progress.json'), document.createTextNode('.'));
      console.error('NeoFlash progress:', error);
    }
  }

  start();
})();
