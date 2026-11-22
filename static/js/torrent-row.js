const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host { display: table-row; }
  td { padding: 8px 10px; border-bottom: 1px solid var(--border); vertical-align: middle; }
  .name { max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name .cat { color: var(--muted); font-size: 12px; }
  .status { font-size: 12px; padding: 2px 8px; border-radius: 10px; white-space: nowrap; }
  .status.downloading { background: #24344d; color: #7cb0ff; }
  .status.seeding { background: #1d3a2a; color: var(--ok); }
  .status.paused { background: #3a3324; color: var(--warn); }
  .status.error { background: #452323; color: var(--err); }
  .status.limit { background: #2d2440; color: #b48cff; }
  .bar { width: 140px; height: 6px; background: var(--panel2); border-radius: 3px; overflow: hidden; }
  .bar > div { height: 100%; background: var(--accent); }
  .num { font-variant-numeric: tabular-nums; color: var(--muted); white-space: nowrap; }
  .subs { color: var(--muted); font-size: 12px; }
  button {
    background: none; border: 1px solid var(--border); color: var(--muted);
    border-radius: var(--radius); padding: 4px 8px; cursor: pointer; font: inherit; margin-right: 4px;
  }
  button:hover { color: var(--text); }
  button.danger:hover { color: var(--err); border-color: var(--err); }
</style>
<td class="name"></td>
<td><span class="status"></span></td>
<td><div class="bar"><div></div></div><span class="pct num"></span></td>
<td class="num dls"></td>
<td class="num uls"></td>
<td class="num peers"></td>
<td class="num size"></td>
<td class="num ratio"></td>
<td class="num eta"></td>
<td class="num added"></td>
<td class="subs"></td>
<td class="actions">
  <button class="pause">⏸</button>
  <button class="danger del">✕</button>
</td>`;

export const fmtSize = (b) => {
  if (!b) return '0 B';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0, v = b;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${u[i]}`;
};

const STATUS_RU = {
  downloading: 'скачивается',
  seeding: 'раздаётся',
  paused: 'пауза',
  error: 'ошибка',
};

class TorrentRow extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
  }

  set data(t) {
    this._t = t;
    const $ = (sel) => this.shadowRoot.querySelector(sel);
    this.shadowRoot.querySelector('.name').innerHTML =
      `${t.name}<br><span class="cat">${t.category ?? ''}${t.error ? ' — ' + t.error : ''}</span>`;
    const st = this.shadowRoot.querySelector('.status');
    if (t.seeding_paused) {
      st.textContent = t.seeding_paused === 'ratio' ? 'лимит ратио' : 'лимит времени';
      st.className = 'status limit';
    } else {
      st.textContent = (STATUS_RU[t.status] ?? t.status) + (t.user_paused ? ' (для вас)' : '');
      st.className = `status ${t.status}`;
    }
    this.shadowRoot.querySelector('.bar > div').style.width = `${(t.progress * 100).toFixed(1)}%`;
    this.shadowRoot.querySelector('.pct').textContent = `${(t.progress * 100).toFixed(1)}%`;
    this.shadowRoot.querySelector('.dls').textContent = `↓ ${fmtSize(t.down_speed)}/с`;
    this.shadowRoot.querySelector('.uls').textContent = `↑ ${fmtSize(t.up_speed)}/с`;
    this.shadowRoot.querySelector('.peers').textContent = `${t.peers} (${t.seeders})`;
    this.shadowRoot.querySelector('.size').textContent =
      `${fmtSize(t.downloaded)} / ${fmtSize(t.size)}`;
    this.shadowRoot.querySelector('.ratio').textContent = t.ratio ? t.ratio.toFixed(2) : '—';
    this.shadowRoot.querySelector('.eta').textContent =
      t.eta_sec > 0 ? `${Math.floor(t.eta_sec / 60)}м ${t.eta_sec % 60}с` : '—';
    this.shadowRoot.querySelector('.added').textContent =
      new Date(t.added_at * 1000).toLocaleDateString();
    this.shadowRoot.querySelector('.subs').textContent = t.subscribers.join(', ');
    // кнопка — про ПЕРСОНАЛЬНУЮ паузу: движение остановится, когда паузу захотят все
    const pausedForMe = t.user_paused || t.seeding_paused;
    this.shadowRoot.querySelector('.pause').textContent = pausedForMe ? '▶' : '⏸';
    this.shadowRoot.querySelector('.pause').onclick = () =>
      document.dispatchEvent(new CustomEvent('torrent-action', {
        detail: { action: pausedForMe ? 'resume' : 'pause', hash: t.hash },
      }));
    this.shadowRoot.querySelector('.del').onclick = () => {
      if (!confirm(`Удалить «${t.name}»?`)) return;
      const deleteFiles = confirm('Стереть файлы с диска?\nОК — да (если вы последний подписчик)\nОтмена — только отписаться');
      document.dispatchEvent(new CustomEvent('torrent-action', {
        detail: { action: 'delete', hash: t.hash, deleteFiles },
      }));
    };
  }

  get data() { return this._t; }
}
customElements.define('torrent-row', TorrentRow);
