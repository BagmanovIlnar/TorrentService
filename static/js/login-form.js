import { api } from './api.js';

const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host { display: flex; justify-content: center; padding-top: 18vh; }
  form {
    background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius);
    padding: 32px; width: 320px; display: flex; flex-direction: column; gap: 12px;
  }
  h1 { font-size: 18px; margin: 0 0 8px; color: var(--text); }
  input {
    background: var(--panel2); color: var(--text); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 10px; font: inherit; outline: none;
  }
  input:focus { border-color: var(--accent); }
  button {
    background: var(--accent); color: #fff; border: 0; border-radius: var(--radius);
    padding: 10px; font: inherit; cursor: pointer;
  }
  button:disabled { opacity: .6; }
  .err { color: var(--err); font-size: 13px; min-height: 16px; }
</style>
<form>
  <h1>TorrentService</h1>
  <input name="user" placeholder="Пользователь" autocomplete="username" required>
  <input name="pass" type="password" placeholder="Пароль" autocomplete="current-password" required>
  <div class="err"></div>
  <button>Войти</button>
</form>`;

class LoginForm extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
    this.shadowRoot.querySelector('form').onsubmit = async (e) => {
      e.preventDefault();
      const f = e.target;
      const err = this.shadowRoot.querySelector('.err');
      const btn = f.querySelector('button');
      btn.disabled = true;
      err.textContent = '';
      try {
        await api.login(f.user.value.trim(), f.pass.value);
        document.dispatchEvent(new CustomEvent('auth-ok'));
      } catch (ex) {
        err.textContent = ex.message === 'unauthorized' ? 'Неверный логин или пароль' : ex.message;
      } finally {
        btn.disabled = false;
      }
    };
  }
}
customElements.define('login-form', LoginForm);
