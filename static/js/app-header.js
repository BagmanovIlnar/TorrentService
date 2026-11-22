import { api } from './api.js';

const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host {
    display: flex; align-items: center; gap: 16px;
    background: var(--panel); border-bottom: 1px solid var(--border);
    padding: 10px 16px;
  }
  .title { font-weight: 600; color: var(--accent); }
  .speeds { color: var(--muted); font-variant-numeric: tabular-nums; }
  .speeds b { color: var(--text); font-weight: 500; }
  nav { display: flex; gap: 4px; margin-left: auto; }
  nav button {
    background: none; color: var(--muted); border: 0; padding: 6px 12px;
    cursor: pointer; border-radius: var(--radius); font: inherit;
  }
  nav button.active { background: var(--panel2); color: var(--text); }
  .user { color: var(--muted); }
  .user b { color: var(--text); font-weight: 500; }
  button.logout {
    background: none; border: 1px solid var(--border); color: var(--muted);
    border-radius: var(--radius); padding: 6px 12px; cursor: pointer; font: inherit;
  }
  button.logout:hover { color: var(--text); }
</style>
<span class="title">TorrentService</span>
<span class="speeds">↓ <b id="down">0</b> / с &nbsp; ↑ <b id="up">0</b> / с</span>
<nav>
  <button data-view="torrents" class="active">Торренты</button>
  <button data-view="files">Файлы</button>
</nav>
<span class="user"></span>
<button class="logout">Выйти</button>`;

const fmt = (bps) => {
  if (!bps) return '0 B';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0, v = bps;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${u[i]}`;
};

class AppHeader extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
    this.shadowRoot.querySelector('button.logout').onclick = async () => {
      await api.logout().catch(() => {});
      document.dispatchEvent(new CustomEvent('logout'));
    };
    this.shadowRoot.querySelectorAll('nav button').forEach((b) => {
      b.onclick = () => {
        this.shadowRoot.querySelectorAll('nav button').forEach((x) => x.classList.remove('active'));
        b.classList.add('active');
        document.dispatchEvent(new CustomEvent('nav', { detail: { view: b.dataset.view } }));
      };
    });
  }

  // список торрентов приходит из torrent-list (единственный источник данных)
  updateSpeeds(list) {
    const down = list.reduce((s, t) => s + t.down_speed, 0);
    const up = list.reduce((s, t) => s + t.up_speed, 0);
    this.shadowRoot.getElementById('down').textContent = fmt(down) ;
    this.shadowRoot.getElementById('up').textContent = fmt(up);
  }
}
customElements.define('app-header', AppHeader);
