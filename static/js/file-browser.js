import { api } from './api.js';
import { fmtSize } from './torrent-row.js';

const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host { display: block; }
  .crumbs { margin-bottom: 10px; color: var(--muted); }
  .crumbs a { color: var(--accent); cursor: pointer; text-decoration: none; }
  .crumbs span.sep { margin: 0 6px; }
  table { width: 100%; border-collapse: collapse; }
  td { padding: 7px 10px; border-bottom: 1px solid var(--border); }
  .fname { cursor: pointer; color: var(--text); }
  .fname.dir { color: var(--accent); }
  .num { color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  button.del {
    background: none; border: 0; color: var(--muted); cursor: pointer; font: inherit;
  }
  button.del:hover { color: var(--err); }
  .empty { color: var(--muted); padding: 24px; text-align: center; }
</style>
<div class="crumbs" id="crumbs"></div>
<table><tbody id="tbody"></tbody></table>
<div class="empty" hidden>Каталог пуст</div>`;

class FileBrowser extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
    this.path = '';
  }

  async load(path = this.path) {
    this.path = path || '';
    // крошки
    const crumbs = this.shadowRoot.getElementById('crumbs');
    crumbs.innerHTML = `<a data-p="">downloads</a>`;
    let acc = '';
    for (const part of this.path.split('/').filter(Boolean)) {
      acc = acc ? `${acc}/${part}` : part;
      crumbs.insertAdjacentHTML('beforeend',
        `<span class="sep">/</span><a data-p="${acc}">${part}</a>`);
    }
    crumbs.querySelectorAll('a').forEach((a) => {
      a.onclick = () => this.load(a.dataset.p);
    });
    // содержимое
    const entries = await api.files(this.path).catch(() => []);
    const tbody = this.shadowRoot.getElementById('tbody');
    tbody.innerHTML = '';
    this.shadowRoot.querySelector('.empty').hidden = entries.length > 0;
    for (const e of entries) {
      const tr = document.createElement('tr');
      tr.innerHTML = `
        <td><span class="fname ${e.is_dir ? 'dir' : ''}">${e.is_dir ? '📁' : '📄'} ${e.name}</span></td>
        <td class="num">${e.is_dir ? '' : fmtSize(e.size)}</td>
        <td class="num">${new Date(e.modified * 1000).toLocaleString()}</td>
        <td><button class="del" title="Удалить">✕</button></td>`;
      tr.querySelector('.fname').onclick = () => e.is_dir && this.load(e.path);
      tr.querySelector('.del').onclick = async () => {
        if (!confirm(`Удалить «${e.name}»${e.is_dir ? ' со всем содержимым' : ''}?`)) return;
        try {
          await api.deleteFile(e.path);
          this.load();
        } catch (ex) { alert(ex.message); }
      };
      tbody.appendChild(tr);
    }
  }
}
customElements.define('file-browser', FileBrowser);
