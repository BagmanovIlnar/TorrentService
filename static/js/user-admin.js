import { api } from './api.js';

const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host { display: block; }
  h2 { font-size: 16px; margin: 0 0 12px; }
  table { width: 100%; border-collapse: collapse; max-width: 640px; }
  td, th { padding: 8px 10px; border-bottom: 1px solid var(--border); text-align: left; }
  th { font-size: 12px; color: var(--muted); font-weight: 500; }
  .role { font-size: 12px; padding: 2px 8px; border-radius: 10px; }
  .role.admin { background: #2d2440; color: #b48cff; }
  .role.user { background: var(--panel2); color: var(--muted); }
  .ad { color: var(--muted); font-size: 12px; }
  button {
    background: none; border: 1px solid var(--border); color: var(--muted);
    border-radius: var(--radius); padding: 4px 8px; cursor: pointer; font: inherit; margin-right: 4px;
  }
  button:hover { color: var(--text); }
  button.danger:hover { color: var(--err); border-color: var(--err); }
  .add { display: flex; gap: 8px; margin-top: 14px; max-width: 640px; flex-wrap: wrap; }
  .add input, .add select {
    background: var(--panel2); color: var(--text); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 8px 10px; font: inherit;
  }
  .add button {
    background: var(--accent); color: #fff; border: 0; padding: 8px 16px;
  }
  .err { color: var(--err); font-size: 13px; width: 100%; min-height: 15px; }
  .note { color: var(--muted); font-size: 13px; max-width: 640px; }
</style>
<h2>Пользователи</h2>
<table><tbody id="tbody"></tbody></table>
<div class="add">
  <input id="u" placeholder="логин">
  <input id="p" type="password" placeholder="пароль">
  <select id="r"><option value="user">user</option><option value="admin">admin</option></select>
  <button id="add">Добавить</button>
  <div class="err"></div>
</div>
<p class="note">Удаление пользователя отписывает его от торрентов; оставшиеся без подписчиков — удаляются вместе с файлами.</p>`;

class UserAdmin extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
    this.me = null;
    this.shadowRoot.getElementById('add').onclick = () => this.add();
  }

  async load() {
    this.me = await api.me().catch(() => null);
    const users = await api.users().catch(() => []);
    const tbody = this.shadowRoot.getElementById('tbody');
    tbody.innerHTML = '';
    for (const u of users) {
      const tr = document.createElement('tr');
      tr.innerHTML = `
        <td>${u.name}${u.name === this.me?.name ? ' <span class="ad">(вы)</span>' : ''}</td>
        <td><span class="role ${u.role}">${u.role}</span>${u.from_ad ? ' <span class="ad">AD</span>' : ''}</td>
        <td>
          <button class="pw">пароль</button>
          <button class="del danger">✕</button>
        </td>`;
      tr.querySelector('.pw').onclick = async () => {
        const pass = prompt(`Новый пароль для «${u.name}» (мин. 4 символа):`);
        if (!pass) return;
        try {
          await api.setPassword(u.name, pass);
        } catch (ex) { alert(ex.message); }
      };
      tr.querySelector('.del').onclick = async () => {
        if (!confirm(`Удалить «${u.name}»? Его торренты без других подписчиков будут удалены с файлами.`)) return;
        try {
          await api.deleteUser(u.name);
          this.load();
        } catch (ex) { alert(ex.message); }
      };
      tbody.appendChild(tr);
    }
  }

  async add() {
    const err = this.shadowRoot.querySelector('.err');
    err.textContent = '';
    const u = this.shadowRoot.getElementById('u');
    const p = this.shadowRoot.getElementById('p');
    const r = this.shadowRoot.getElementById('r');
    try {
      await api.addUser(u.value.trim(), p.value, r.value);
      u.value = ''; p.value = '';
      this.load();
    } catch (ex) { err.textContent = ex.message; }
  }
}
customElements.define('user-admin', UserAdmin);
