import { api } from './api.js';

const tpl = document.createElement('template');
tpl.innerHTML = `
<style>
  :host { display: block; padding: 16px; }
  h2 { font-size: 13px; text-transform: uppercase; letter-spacing: .5px; color: var(--muted); margin: 0 0 10px; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { display: flex; align-items: center; }
  .node {
    flex: 1; padding: 6px 10px; cursor: pointer; border-radius: var(--radius);
    color: var(--text); border: 0; background: none; font: inherit; text-align: left;
    display: flex; align-items: center; gap: 8px; min-width: 0;
  }
  .node:hover { background: var(--panel2); }
  .node.active { background: var(--panel2); color: var(--accent); }
  .node .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .node .cnt { color: var(--muted); font-size: 12px; margin-left: auto; }
  .del {
    background: none; border: 0; color: var(--muted); cursor: pointer; padding: 4px 8px;
    visibility: hidden; font: inherit;
  }
  li:hover .del { visibility: visible; }
  .del:hover { color: var(--err); }
  .add { display: flex; gap: 6px; margin-top: 12px; }
  .add input {
    flex: 1; min-width: 0; background: var(--panel2); color: var(--text);
    border: 1px solid var(--border); border-radius: var(--radius); padding: 6px 8px; font: inherit;
  }
  .add button {
    background: var(--accent); color: #fff; border: 0; border-radius: var(--radius);
    padding: 6px 10px; cursor: pointer; font: inherit;
  }
</style>
<h2>Категории</h2>
<ul id="list"></ul>
<div class="add">
  <input id="new" placeholder="movies/action">
  <button id="addBtn">+</button>
</div>`;

// плоский список ["movies", "movies/action"] -> дерево
function buildTree(names) {
  const root = { children: new Map() };
  for (const n of names) {
    let node = root;
    for (const part of n.split('/')) {
      if (!node.children.has(part)) node.children.set(part, { children: new Map() });
      node = node.children.get(part);
    }
    node.full = n;
  }
  return root;
}

class CategoryTree extends HTMLElement {
  constructor() {
    super();
    this.attachShadow({ mode: 'open' }).appendChild(tpl.content.cloneNode(true));
    this.selected = null;
    this.shadowRoot.getElementById('addBtn').onclick = () => this.addNew();
    this.shadowRoot.getElementById('new').onkeydown = (e) => e.key === 'Enter' && this.addNew();
  }

  async load() {
    const cats = await api.categories().catch(() => []);
    const ul = this.shadowRoot.getElementById('list');
    ul.innerHTML = '';
    const root = buildTree(cats);

    // рекурсивный рендер дерева
    const addNode = (node, depth, parentUl) => {
      for (const [name, child] of node.children) {
        const li = document.createElement('li');
        li.innerHTML = `
          <button class="node" style="padding-left:${10 + depth * 16}px">
            <span class="name">${name}</span>
            <span class="cnt"></span>
          </button>
          <button class="del" title="Удалить">×</button>`;
        const btn = li.querySelector('.node');
        btn.onclick = () => this.select(child.full, btn);
        li.querySelector('.del').onclick = async () => {
          if (!confirm(`Удалить категорию «${child.full}»? Торренты останутся.`)) return;
          await api.deleteCategory(child.full);
          this.load();
          if (this.selected === child.full) this.select(null, null);
        };
        parentUl.appendChild(li);
        if (child.children.size) {
          const sub = document.createElement('ul');
          parentUl.appendChild(sub);
          addNode(child, depth + 1, sub);
        }
      }
    };
    addNode(root, 0, ul);
    document.dispatchEvent(new CustomEvent('categories-changed', { detail: cats }));
  }

  select(name, btn) {
    this.shadowRoot.querySelectorAll('.node').forEach((n) => n.classList.remove('active'));
    if (btn) btn.classList.add('active');
    this.selected = name;
    document.dispatchEvent(new CustomEvent('category-selected', { detail: { name }, bubbles: true, composed: true }));
  }

  async addNew() {
    const input = this.shadowRoot.getElementById('new');
    const name = input.value.trim();
    if (!name) return;
    await api.addCategory(name);
    input.value = '';
    await this.load();
  }
}
customElements.define('category-tree', CategoryTree);
