'use strict';

(() => {
  const labels = { implemented: 'Implemented', partial: 'Partial', planned: 'Planned' };
  const colors = { implemented: '#147b63', partial: '#b98124', needs_review: '#83629d', total: '#426986' };
  const catalogueStatuses = [['implemented_subset', 'Implemented subset'], ['partial', 'Partial'], ['missing', 'Missing'], ['needs_review', 'Needs review']];
  const repository = 'https://github.com/jazzymiles/NeoFlash';
  const dateFormat = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeZone: 'UTC' });
  const shortDateFormat = new Intl.DateTimeFormat('en-US', { day: '2-digit', month: '2-digit', timeZone: 'UTC' });
  const byId = id => document.getElementById(id);
  const number = value => Number(value).toLocaleString('en-US');
  const isActionScript = id => id === 'as2' || id === 'as3';
  const percent = counts => `${counts.total ? (100 * counts.implemented / counts.total).toFixed(1) : '0'}%`;
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
    if (![1, 2].includes(data.schemaVersion) || !Array.isArray(data.categories) || !Array.isArray(data.features) || !Array.isArray(data.history)) {
      throw new Error('Unknown data format.');
    }
    if (!/^[a-f0-9]{7,40}$/.test(data.revision) || Number.isNaN(Date.parse(data.generatedAt))) {
      throw new Error('The snapshot has no valid date or revision.');
    }
    const categories = new Set(data.categories.map(category => category.id));
    data.features.forEach(feature => {
      if (!categories.has(feature.category) || !Object.hasOwn(labels, feature.status) || !Array.isArray(feature.evidence)) {
        throw new Error('Invalid register entry.');
      }
    });
    if (data.actionscript) {
      const languages = data.actionscript.counts.byLanguage || data.actionscript.counts;
      for (const language of ['AS2', 'AS3']) {
        const counts = languages[language];
        const keys = ['implemented_subset', 'partial', 'missing', 'needs_review'];
        if (!counts || !keys.every(key => Number.isInteger(counts[key]) && counts[key] >= 0) || keys.reduce((sum, key) => sum + counts[key], 0) !== counts.total) {
          throw new Error('Invalid ActionScript catalogue counts.');
        }
      }
      const priorities = data.actionscript.priorities;
      if (priorities) {
        const validCounts = counts => counts && catalogueStatuses.every(([key]) => Number.isInteger(counts[key]) && counts[key] >= 0)
          && catalogueStatuses.reduce((sum, [key]) => sum + counts[key], 0) === counts.total;
        const validRows = rows => Array.isArray(rows) && rows.every(row => typeof row.id === 'string' && typeof row.title === 'string' && typeof row.purpose === 'string' && validCounts(row.counts));
        if (priorities.schemaVersion !== 1 || !validCounts(priorities.total) || !validRows(priorities.groups) || !validRows(priorities.stages) || !validRows(priorities.buckets)
          || !Array.isArray(priorities.limitations) || !priorities.limitations.every(value => typeof value === 'string')) {
          throw new Error('Invalid AS3 priority classification.');
        }
        const player = priorities.buckets.find(bucket => bucket.id === 'player');
        const sameCounts = (left, right) => ['total', ...catalogueStatuses.map(([key]) => key)].every(key => left[key] === right[key]);
        const sumCounts = rows => Object.fromEntries(['total', ...catalogueStatuses.map(([key]) => key)].map(key => [key, rows.reduce((sum, row) => sum + row.counts[key], 0)]));
        if (!player || !sameCounts(priorities.total, languages.AS3) || !sameCounts(sumCounts(priorities.groups), priorities.total)
          || !sameCounts(sumCounts(priorities.buckets), priorities.total) || !sameCounts(sumCounts(priorities.stages), player.counts)
          || priorities.stages.some((stage, index) => stage.order !== index + 1)) {
          throw new Error('AS3 priority totals or stage order do not match the catalogue.');
        }
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

  function renderActionScript(data) {
    const audit = data.actionscript;
    if (!audit) {
      byId('actionscript-note').textContent = 'The full ActionScript catalogue is unavailable in this snapshot. Feature-family counts are not used as its denominator.';
      return;
    }
    const languages = audit.counts.byLanguage || audit.counts;
    const scopes = audit.byScope?.byLanguageScope || audit.byScope;
    for (const language of ['AS2', 'AS3']) {
      const counts = languages[language];
      const card = element('article', 'category-card actionscript-card');
      const heading = element('h3');
      heading.append(link(language === 'AS2' ? 'ActionScript 2' : 'ActionScript 3', pathUrl(data.revision, 'docs/progress/ACTIONSCRIPT.md')));
      card.append(heading, element('p', 'catalogue-total-label', 'Documented entries'));
      const metric = element('p', 'metric catalogue-total', number(counts.total));
      metric.setAttribute('aria-label', `${number(counts.total)} documented entries`);
      card.append(metric);
      const statuses = element('dl', 'catalogue-statuses');
      for (const [key, label] of catalogueStatuses) {
        const item = element('div', `catalogue-status ${key}`);
        item.append(element('dt', '', label), element('dd', '', number(counts[key])));
        statuses.append(item);
      }
      card.append(statuses);
      const languageScopes = scopes?.[language];
      if (languageScopes) {
        const core = ['language', 'core', 'runtime'].reduce((sum, scope) => sum + (languageScopes[scope]?.total || 0), 0);
        card.append(element('p', 'catalogue-scope', `Language + core + runtime: ${number(core)} entries. The full catalogue also includes framework, component and platform-extension entries, including AIR/Flex where documented.`));
      }
      card.append(link('Catalogue breakdown and audit →', pathUrl(data.revision, 'docs/progress/ACTIONSCRIPT.md'), 'category-link'));
      byId('actionscript-categories').append(card);
    }
    const note = byId('actionscript-note');
    note.replaceChildren();
    const auditDate = audit.auditedAt && !Number.isNaN(Date.parse(audit.auditedAt)) ? date(audit.auditedAt) : String(audit.auditedAt || 'date not recorded');
    note.append(document.createTextNode(`Dated audit: ${auditDate}. “Implemented subset” does not mean full support. This is not a fresh Flash conformance result. `));
    if (audit.sourceSnapshot?.implementationMatches === true) note.append(document.createTextNode('Audited source matches this published implementation. '));
    else if (audit.sourceSnapshot?.implementationMatches === false) note.append(document.createTextNode('Implementation changed since this audit; counts are a dated snapshot. '));
    if (audit.sourceRevision) {
      note.append(document.createTextNode('Audited baseline: '), element('code', '', audit.sourceRevision.slice(0, 7)), document.createTextNode('. '));
    }
    note.append(link('Audit evidence', pathUrl(data.revision, 'docs/progress/actionscript-audit.json')));
  }

  function priorityTable(rows, caption, ordered = false) {
    const wrapper = element('div', 'table-scroll priority-scroll');
    wrapper.tabIndex = 0;
    wrapper.setAttribute('role', 'region');
    wrapper.setAttribute('aria-label', caption);
    const table = element('table', 'priority-table');
    table.append(element('caption', '', caption));
    const head = element('thead'), heading = element('tr');
    for (const label of [ordered ? 'Adaptation order and purpose' : 'Scope and purpose', 'Total', ...catalogueStatuses.map(([key, label]) => key === 'missing' ? 'Audit missing' : label)]) {
      const cell = element('th', '', label);
      cell.scope = 'col';
      heading.append(cell);
    }
    head.append(heading);
    const body = element('tbody');
    for (const row of rows) {
      const line = element('tr', `priority-${row.group || row.id}`);
      const title = element('th', 'priority-topic');
      title.scope = 'row';
      title.append(element('span', 'priority-title', `${ordered ? `${row.order}. ` : ''}${row.title}`), element('span', 'priority-purpose', row.purpose));
      line.append(title);
      for (const key of ['total', ...catalogueStatuses.map(([key]) => key)]) line.append(element('td', `priority-count ${key}`, number(row.counts[key])));
      body.append(line);
    }
    table.append(head, body);
    wrapper.append(table);
    return wrapper;
  }

  function renderAS3Priorities(data) {
    const priorities = data.actionscript?.priorities;
    const section = byId('as3-priorities');
    if (!priorities || !section) return;
    section.hidden = false;
    byId('as3-priorities-description').textContent = priorities.description;
    const content = byId('as3-priorities-content');
    content.replaceChildren();
    content.append(priorityTable(priorities.groups, 'Every AS3 catalogue entry belongs to one scope below. AIR-only APIs and shipped libraries are separate from the Flash Player adaptation queue.'));
    const stages = element('details', 'priority-stages');
    stages.open = true;
    stages.append(element('summary', '', 'Flash Player adaptation order: foundations first'));
    stages.append(element('p', 'fine-print', 'These stages divide the same Flash Player entries shown above; they are not additional entries. “Audit missing” is a dated decision, not a fresh source check. Adaptation targets remaining behavior; existing methods are not scheduled again. “Needs review” requires evidence before being treated as missing.'));
    stages.append(priorityTable(priorities.stages, 'Ordered by dependency and usefulness for classic Flash content. Cameras, networking and external services follow the core.', true));
    content.append(stages);
    const note = element('p', 'fine-print priority-boundary', 'Classification describes documented Flash Player availability, including APIs shared with AIR. It does not promise equivalent HTML5 browser access or establish new implementation evidence. ');
    note.append(link('Per-entry classification, exclusions and evidence →', pathUrl(data.revision, 'docs/progress/ACTIONSCRIPT.md')));
    content.append(note);
    if (priorities.limitations.length) {
      const details = element('details', 'priority-limitations');
      details.append(element('summary', '', 'Classification limits'));
      const list = element('ul');
      priorities.limitations.forEach(value => list.append(element('li', '', value)));
      details.append(list);
      content.append(details);
    }
  }

  function renderCategories(data) {
    data.categories.filter(category => !isActionScript(category.id)).forEach(category => {
      const counts = countsFor(data.features.filter(feature => feature.category === category.id));
      const card = element('article', 'category-card');
      const heading = element('h3');
      heading.append(link(category.title, pathUrl(data.revision, 'docs/progress/DETAILS.md')));
      card.append(heading);
      const metric = element('p', 'metric');
      metric.append(element('span', '', counts.implemented), element('span', 'denominator', `/ ${counts.total}`), element('span', 'percent', percent(counts)));
      metric.setAttribute('aria-label', `Implemented ${counts.implemented} of ${counts.total}, ${percent(counts)}`);
      card.append(metric);
      const bar = element('div', 'progress-bar');
      bar.setAttribute('aria-hidden', 'true');
      for (const [status, className] of [['implemented', 'done'], ['partial', 'part']]) {
        const segment = element('span', className);
        segment.style.width = `${counts.total ? 100 * counts[status] / counts.total : 0}%`;
        bar.append(segment);
      }
      const breakdown = element('div', 'category-breakdown');
      breakdown.append(element('span', '', `Partial: ${counts.partial}`), element('span', '', `Planned: ${counts.planned}`));
      card.append(bar, breakdown, element('p', 'category-description', category.description));
      const features = data.features.filter(feature => feature.category === category.id);
      const working = features.filter(feature => feature.status === 'implemented').slice(0, 2);
      const remaining = features.filter(feature => feature.status === 'planned').slice(0, 2);
      if (working.length) card.append(element('p', 'category-examples', `Available: ${working.map(feature => feature.title).join('; ')}.`));
      else if (counts.partial) card.append(element('p', 'category-examples', 'A limited subset works; no tracked item is complete yet.'));
      if (remaining.length) card.append(element('p', 'category-examples pending', `Planned: ${remaining.map(feature => feature.title).join('; ')}.`));
      byId('categories').append(card);
    });
  }

  function renderCurrent(data) {
    renderActionScript(data);
    renderAS3Priorities(data);
    renderCategories(data);
  }

  function methodologyFor(point, category) {
    return point.categories[category]?.methodologyVersion || point.methodologyVersion;
  }

  function historyFor(category, history) {
    return history.filter(point => point.categories[category] && (!isActionScript(category) || (methodologyFor(point, category) >= 2 && (!point.categories[category].metric || point.categories[category].metric === 'catalogue'))));
  }

  function historyReadout(point, counts, category) {
    if (isActionScript(category)) return `${date(point.date)} · subset ${number(counts.implemented)} · partial ${number(counts.partial)} · missing ${number(counts.planned)} · needs review ${number(counts.needs_review || 0)} · total ${number(counts.total)} · v${methodologyFor(point, category)}`;
    return `${date(point.date)} · implemented ${counts.implemented}/${counts.total} (${percent(counts)}) · partial ${counts.partial} · planned ${counts.planned} · v${methodologyFor(point, category)}`;
  }

  function renderChart(category, history) {
    const card = element('article', 'chart-card');
    const title = element('h3', '', category.title);
    title.id = `chart-title-${category.id}`;
    card.append(title, element('p', 'chart-description', isActionScript(category.id) ? 'Full documented catalogue. Green counts implemented subsets, not complete support. Dated audit values are carried forward until reviewed.' : category.description));
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
    svg.append(svgElement('title', {}, isActionScript(category.id) ? `${category.title}: implemented subsets, partial support, needs review and total documented entries by day` : `${category.title}: implemented, partial and total tracked milestones by day`));
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
    byId('history-implemented-label').textContent = catalogue ? 'Implemented subset' : 'Implemented';
    byId('history-remaining-label').textContent = catalogue ? 'Missing' : 'Planned';
    byId('history-review-label').hidden = !catalogue;
    byId('history-share-label').hidden = catalogue;
    byId('history-table-note').textContent = catalogue
      ? 'Full documented catalogue, including framework, component and platform-extension entries. “Implemented subset” does not mean full support. Needs-review entries remain in the denominator. Values reflect a dated audit, not fresh conformance verification.'
      : 'Share = implemented milestones / all tracked milestones in this product area. Partial support does not count as complete. These milestones are independent of the ActionScript catalogue.';
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
      if (!catalogue) row.append(element('td', '', percent(counts)));
      row.append(element('td', '', `v${methodologyFor(point, category)}`));
      body.append(row);
    });
  }

  function renderHistory(data) {
    const history = [...data.history].sort((a, b) => a.date.localeCompare(b.date));
    fillCategorySelect(byId('history-category'), data.categories);
    byId('history-note').textContent = history.length === 1
      ? `One recorded day: ${date(history[0].date)}. AS charts include only full-catalogue snapshots from methodology v2; later development days will extend the history.`
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
