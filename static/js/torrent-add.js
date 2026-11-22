import { api } from './api.js';

const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host { display: block; margin-bottom: 16px; }
  .box {
    background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius);
    padding: 12px; display: flex; gap: 8px; flex-wrap: wrap; align-items: center;
  }
  input#magnet {
    flex: 1; min-width: 260px; background: var(--panel2); color: var(--text);
    border: 1px solid var(--border); border-radius: var(--radius); padding: 9px 12px; font: inherit;
  }
  select {
    background: var(--panel2); color: var(--text); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 9px; font: inherit;
  }
  button {
    background: var(--accent); color: #fff; border: 0; border-radius: var(--radius);
    padding: 9px 16px; cursor: pointer; font: inherit;
  }
  label.file {
    background: var(--panel2); color: var(--muted); border: 1px dashed var(--border);
    border-radius: var(--radius); padding: 9px 12px; cursor: pointer; font-size: 13px;
  }
  label.file:hover { color: var(--text); }
  input[type=file] { display: none; }
  .err { color: var(--err); width: 100%; font-size: 13px; min-height: 15px; }
</style>
<div class="box">
  <input id="magnet" placeholder="magnet:?xt=urn:btih:...">
  <select id="cat"><option value="">— без категории —</option></select>
  <label class="file">.torrent файл<input type="file" id="file" accept=".torrent"></label>
  <button id="go">Добавить</button>
  <div class="err"></div>
</div>`;

class TorrentAdd extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
    this.shadowRoot.getElementById('go').onclick = () => this.addMagnet();
    this.shadowRoot.getElementById('magnet').onkeydown = (e) => e.key === 'Enter' && this.addMagnet();
    this.shadowRoot.getElementById('file').onchange = (e) => this.addFile(e.target.files[0]);
    document.addEventListener('categories-changed', (e) => this.refreshCategories(e.detail));
  }

  refreshCategories(cats) {
    const sel = this.shadowRoot.getElementById('cat');
    const current = sel.value;
    sel.innerHTML = '<option value="">— без категории —</option>' +
      cats.map((c) => `<option value="${c}">${c}</option>`).join('');
    if (cats.includes(current)) sel.value = current;
  }

  async addMagnet() {
    const input = this.shadowRoot.getElementById('magnet');
    const err = this.shadowRoot.querySelector('.err');
    const magnet = input.value.trim();
    if (!magnet) return;
    err.textContent = '';
    try {
      await api.addTorrent(magnet, this.shadowRoot.getElementById('cat').value);
      input.value = '';
      document.dispatchEvent(new CustomEvent('torrents-changed'));
    } catch (ex) {
      err.textContent = ex.message;
    }
  }

  async addFile(file) {
    if (!file) return;
    const err = this.shadowRoot.querySelector('.err');
    err.textContent = '';
    try {
      await api.addTorrentFile(file, this.shadowRoot.getElementById('cat').value);
      this.shadowRoot.getElementById('file').value = '';
      document.dispatchEvent(new CustomEvent('torrents-changed'));
    } catch (ex) {
      err.textContent = ex.message;
    }
  }
}
customElements.define('torrent-add', TorrentAdd);
