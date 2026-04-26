// bestiary viewer — vanilla JS, no framework, single fetch.
//
// Data: a single catalog.json fetched once on page load, kept entirely in
// memory. With ~few thousand entries this stays well under 1 MB compressed
// and a few hundred KB decompressed; client-side filter on every keystroke
// runs in <10 ms.
//
// Rendering: paginated by 100 in the unfiltered view to keep the DOM small.
// As soon as the user types into search, results render in full (a search
// hit set rarely exceeds a few hundred and renders fast).

const CATALOG_URL = './catalog.json';
const PAGE = 100;

let CATALOG = {};
let ALL = []; // sorted array of entries
let CATEGORIES = []; // sorted unique categories
let view = { route: 'list', q: '', category: null, shown: PAGE };

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

async function load() {
  try {
    const r = await fetch(CATALOG_URL, { cache: 'no-cache' });
    if (!r.ok) throw new Error(`fetch failed: ${r.status}`);
    CATALOG = await r.json();
  } catch (e) {
    main().innerHTML = `<p class="empty">Couldn't load catalog: ${escapeHtml(e.message)}</p>`;
    return;
  }
  ALL = Object.values(CATALOG).sort((a, b) => a.name.localeCompare(b.name));
  CATEGORIES = Array.from(new Set(ALL.map(e => e.category).filter(Boolean))).sort();
  bindControls();
  parseHash();
  render();
}

function bindControls() {
  $('#search').addEventListener('input', (e) => {
    view.q = e.target.value.trim();
    view.shown = PAGE;
    pushHashFromState();
    render();
  });
  $('#home-link').addEventListener('click', (e) => {
    if (e.metaKey || e.ctrlKey || e.shiftKey) return;
    e.preventDefault();
    view = { route: 'list', q: '', category: null, shown: PAGE };
    $('#search').value = '';
    pushHashFromState();
    render();
  });
  window.addEventListener('hashchange', () => {
    parseHash();
    render();
  });
}

function parseHash() {
  const raw = location.hash.replace(/^#\/?/, '');
  if (!raw) {
    view = { route: 'list', q: '', category: null, shown: PAGE };
    $('#search').value = '';
    return;
  }
  if (raw.startsWith('?')) {
    const p = new URLSearchParams(raw.slice(1));
    view = {
      route: 'list',
      q: p.get('q') || '',
      category: p.get('category') || null,
      shown: PAGE,
    };
    $('#search').value = view.q;
    return;
  }
  // Treat as detail route: #/<name>
  view = { route: 'detail', name: decodeURIComponent(raw) };
}

function pushHashFromState() {
  if (view.route !== 'list') return;
  const params = new URLSearchParams();
  if (view.q) params.set('q', view.q);
  if (view.category) params.set('category', view.category);
  const qs = params.toString();
  const target = qs ? `#/?${qs}` : '#/';
  if (location.hash !== target) {
    history.replaceState(null, '', target);
  }
}

function render() {
  // The category nav lives in the header and is visible on every view,
  // so render it on every render — including detail — to keep its
  // active state honest.
  renderCategories();
  if (view.route === 'detail') {
    // Hide the count on detail pages; it only describes the list.
    $('#count').textContent = '';
    renderDetail();
  } else {
    renderList();
  }
}

function renderList() {
  const filtered = filterAll();
  const total = ALL.length;
  // Only show the count when something is filtering — otherwise "659 of
  // 659" is just noise.
  const filtering = view.q || view.category;
  $('#count').textContent = filtering && total
    ? `${filtered.length} of ${total}`
    : '';
  const slice = view.q
    ? filtered                          // search hits: show all
    : filtered.slice(0, view.shown);    // unfiltered/category-only: paginate

  const m = main();
  if (filtered.length === 0) {
    m.innerHTML = '<p class="empty">No matches.</p>';
    return;
  }

  const tpl = $('#card-template');
  const frag = document.createDocumentFragment();
  for (const e of slice) {
    const node = tpl.content.firstElementChild.cloneNode(true);
    node.dataset.name = e.name;
    node.href = `#/${encodeURIComponent(e.name)}`;
    $('.name', node).textContent = e.name;
    $('.display', node).textContent = e.display_name && e.display_name !== e.name ? e.display_name : '';
    $('.cat', node).textContent = e.category || '';
    const flavors = Object.keys(e.locations || {}).sort();
    $('.flavors', node).innerHTML = flavors
      .map(f => `<span class="flavor f-${f}">${f}</span>`)
      .join('');
    frag.appendChild(node);
  }
  m.innerHTML = '';
  m.appendChild(frag);

  if (!view.q && filtered.length > view.shown) {
    const btn = document.createElement('button');
    btn.id = 'load-more';
    btn.textContent = `Show ${Math.min(PAGE, filtered.length - view.shown)} more (of ${filtered.length - view.shown} remaining)`;
    btn.addEventListener('click', () => {
      view.shown += PAGE;
      render();
    });
    m.appendChild(btn);
  }
}

function renderCategories() {
  const nav = $('#categories');
  if (!CATEGORIES.length) {
    nav.innerHTML = '';
    return;
  }
  const chips = [
    { label: 'all', value: null, active: !view.category },
    ...CATEGORIES.map(c => ({ label: c, value: c, active: view.category === c })),
  ];
  nav.innerHTML = chips
    .map(c => `<button class="chip${c.active ? ' active' : ''}" data-cat="${escapeAttr(c.value || '')}">${escapeHtml(c.label)}</button>`)
    .join('');
  $$('button.chip', nav).forEach(b => {
    b.addEventListener('click', () => {
      // From any view (list or detail), clicking a category chip jumps
      // back to the list filtered by that category. Search resets so
      // the user sees the full category, not whatever they were typing.
      view = { route: 'list', q: '', category: b.dataset.cat || null, shown: PAGE };
      $('#search').value = '';
      pushHashFromState();
      render();
    });
  });
}

function filterAll() {
  const q = view.q.toLowerCase();
  return ALL.filter(e => {
    if (view.category && e.category !== view.category) return false;
    if (!q) return true;
    if (e.name.toLowerCase().includes(q)) return true;
    if ((e.display_name || '').toLowerCase().includes(q)) return true;
    if ((e.category || '').toLowerCase().includes(q)) return true;
    if ((e.tags || []).some(t => t.toLowerCase().includes(q))) return true;
    return false;
  });
}

function renderDetail() {
  const entry = CATALOG[view.name];
  const m = main();
  if (!entry) {
    m.innerHTML = `
      <div class="detail">
        <a class="back" href="#/">← back to catalog</a>
        <p class="empty">No entry for <code>${escapeHtml(view.name)}</code>.</p>
      </div>`;
    return;
  }

  const display = entry.display_name && entry.display_name !== entry.name ? entry.display_name : '';
  const info = [];
  if (entry.category) {
    const cat = escapeHtml(entry.category);
    const catUrl = `#/?category=${encodeURIComponent(entry.category)}`;
    info.push(`<span>category: <a class="filter-link" href="${catUrl}">${cat}</a></span>`);
  }
  if (entry.homepage) info.push(`<span>homepage: <a href="${escapeAttr(entry.homepage)}" rel="noopener">${escapeHtml(entry.homepage)}</a></span>`);
  if (entry.tags && entry.tags.length) {
    const tagLinks = entry.tags.map(t => {
      const url = `#/?q=${encodeURIComponent(t)}`;
      return `<a class="filter-link" href="${url}">${escapeHtml(t)}</a>`;
    }).join(', ');
    info.push(`<span>tags: ${tagLinks}</span>`);
  }

  const flavorBlocks = Object.entries(entry.locations || {}).map(([flavor, loc]) => {
    const id = loc.flatpak_id ? ` <span class="id">${escapeHtml(loc.flatpak_id)}</span>`
      : loc.snap_name ? ` <span class="id">${escapeHtml(loc.snap_name)}</span>`
      : '';
    const rows = ['config', 'data', 'cache', 'state']
      .filter(k => loc[k])
      .map(k => `<dt>${k}</dt><dd>${escapeHtml(loc[k])}</dd>`)
      .join('');
    return `
      <div class="flavor-block">
        <h3><span class="flavor f-${flavor}">${flavor}</span>${id}</h3>
        <dl>${rows}</dl>
      </div>`;
  }).join('');

  const exclude = entry.backup_exclude && entry.backup_exclude.length
    ? `<p class="exclude">backup excludes: ${entry.backup_exclude.map(g => `<code>${escapeHtml(g)}</code>`).join(' ')}</p>`
    : '';

  m.innerHTML = `
    <div class="detail">
      <a class="back" href="#/">← back to catalog</a>
      <h2>
        <span class="name">${escapeHtml(entry.name)}</span>
        ${display ? `<span class="display">${escapeHtml(display)}</span>` : ''}
      </h2>
      <div class="info">${info.join('')}</div>
      ${flavorBlocks}
      ${exclude}
      <p class="meta"><a href="./catalog.json">raw JSON</a></p>
    </div>`;
}

function main() { return $('#main'); }

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
function escapeAttr(s) { return escapeHtml(s); }

document.addEventListener('DOMContentLoaded', load);
