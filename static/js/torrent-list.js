import { api } from './api.js';
import './torrent-row.js';

const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host { display: block; }
  .toolbar { display: flex; gap: 10px; margin-bottom: 10px; align-items: center; }
  input#search {
    background: var(--panel2); color: var(--text); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 8px 12px; font: inherit; width: 260px; outline: none;
  }
  .cnt { color: var(--muted); font-size: 13px; }
  table { width: 100%; border-collapse: collapse; }
  thead th {
    text-align: left; font-size: 12px; color: var(--muted); font-weight: 500;
    padding: 6px 10px; border-bottom: 1px solid var(--border); cursor: pointer; user-select: none;
    white-space: nowrap;
  }
  thead th:hover { color: var(--text); }
  thead th .arrow { font-size: 10px; }
  .empty { color: var(--muted); padding: 32px; text-align: center; }
</style>
<div class="toolbar">
  <input id="search" placeholder="Поиск по имени…">
  <span class="cnt"></span>
</div>
<table>
  <thead>
    <tr>
      <th data-sort="name">Имя</th>
      <th data-sort="status">Статус</th>
      <th data-sort="progress">Прогресс</th>
      <th data-sort="down_speed">↓</th>
      <th data-sort="up_speed">↑</th>
      <th data-sort="peers">Пиры</th>
      <th data-sort="size">Размер</th>
      <th data-sort="ratio">Ратио</th>
      <th data-sort="eta_sec">ETA</th>
      <th data-sort="added_at">Добавлен</th>
      <th>Подписчики</th>
      <th></th>
    </tr>
  </thead>
  <tbody></tbody>
</table>
<div class="empty" hidden>Торрентов нет. Добавьте magnet или .torrent выше.</div>`;

class TorrentList extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
    this.category = null;
    this.sortKey = 'added_at';
    this.sortDir = -1;
    this.timer = null;
    this.lastList = [];

    this.shadowRoot.getElementById('search').oninput = () => this.render();
    this.shadowRoot.querySelectorAll('thead th[data-sort]').forEach((th) => {
      th.onclick = () => {
        const key = th.dataset.sort;
        if (this.sortKey === key) this.sortDir *= -1;
        else { this.sortKey = key; this.sortDir = 1; }
        this.render();
      };
    });
    document.addEventListener('torrent-action', (e) => this.action(e.detail));
    document.addEventListener('torrents-changed', () => this.refresh());
  }

  setCategory(name) {
    this.category = name;
    this.refresh();
  }

  start() {
    this.stop();
    this.refresh();
    this.timer = setInterval(() => this.refresh(), 2000);
  }

  stop() {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  disconnectedCallback() { this.stop(); }

  async refresh() {
    try {
      const list = await api.torrents(this.category);
      this.lastList = list;
      document.querySelector('app-header').updateSpeeds(list);
      this.render();
    } catch { /* 401 обработан в api.js */ }
  }

  async action({ action, hash, deleteFiles }) {
    try {
      if (action === 'pause') await api.pause(hash);
      if (action === 'resume') await api.resume(hash);
      if (action === 'delete') await api.deleteTorrent(hash, deleteFiles);
      this.refresh();
    } catch (ex) { alert(ex.message); }
  }

  render() {
    const q = this.shadowRoot.getElementById('search').value.trim().toLowerCase();
    const rows = this.lastList
      .filter((t) => !q || t.name.toLowerCase().includes(q))
      .sort((a, b) => {
        const va = a[this.sortKey], vb = b[this.sortKey];
        return ((va > vb) - (va < vb)) * this.sortDir;
      });
    this.shadowRoot.querySelector('.cnt').textContent = `${rows.length} шт.`;
    this.shadowRoot.querySelector('.empty').hidden = rows.length > 0;
    // индикатор сортировки
    this.shadowRoot.querySelectorAll('thead th').forEach((th) => {
      const arrow = th.querySelector('.arrow');
      if (arrow) arrow.remove();
      if (th.dataset.sort === this.sortKey) {
        const s = document.createElement('span');
        s.className = 'arrow';
        s.textContent = this.sortDir > 0 ? ' ▲' : ' ▼';
        th.appendChild(s);
      }
    });
    const tbody = this.shadowRoot.querySelector('tbody');
    const seen = new Set();
    for (const t of rows) {
      seen.add(t.hash);
      let row = tbody.querySelector(`torrent-row[hash="${t.hash}"]`);
      if (!row) {
        row = document.createElement('torrent-row');
        row.setAttribute('hash', t.hash);
        tbody.appendChild(row);
      }
      row.data = t;
    }
    tbody.querySelectorAll('torrent-row').forEach((r) => {
      if (!seen.has(r.getAttribute('hash'))) r.remove();
    });
  }
}
customElements.define('torrent-list', TorrentList);
