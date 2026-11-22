// Обёртки над REST. 401 -> событие 'auth-required' (показываем логин).

async function req(method, url, body, raw = false) {
  const opts = { method, headers: {} };
  if (body !== undefined) {
    if (body instanceof FormData || body instanceof Blob) opts.body = body;
    else {
      opts.headers['Content-Type'] = 'application/json';
      opts.body = JSON.stringify(body);
    }
  }
  const r = await fetch(url, opts);
  if (r.status === 401) {
    document.dispatchEvent(new CustomEvent('auth-required'));
    throw new Error('unauthorized');
  }
  if (!r.ok) {
    const text = await r.text();
    throw new Error(text || r.status);
  }
  return raw ? r : r.json();
}

export const api = {
  login: (user, pass) => req('POST', '/login', { user, pass }),
  logout: () => req('POST', '/logout'),
  torrents: (category) => req('GET', '/torrents' + (category ? `?category=${encodeURIComponent(category)}` : '')),
  addTorrent: (magnet, category) => req('POST', '/torrents', { magnet, category: category || null }),
  addTorrentFile: (file, category) => {
    const fd = new FormData();
    fd.append('file', file);
    if (category) fd.append('category', category);
    return req('POST', '/torrents/file', fd);
  },
  deleteTorrent: (hash, deleteFiles) => req('DELETE', `/torrents/${hash}?delete_files=${!!deleteFiles}`),
  pause: (hash) => req('POST', `/torrents/${hash}/pause`),
  resume: (hash) => req('POST', `/torrents/${hash}/resume`),
  setCategory: (hash, category) => req('POST', `/torrents/${hash}/category`, { category }),
  categories: () => req('GET', '/categories'),
  addCategory: (name) => req('POST', '/categories', { name }),
  deleteCategory: (name) => req('DELETE', `/categories/${encodeURIComponent(name)}`),
  files: (path) => req('GET', '/files' + (path ? `?path=${encodeURIComponent(path)}` : '')),
  deleteFile: (path) => req('DELETE', `/files?path=${encodeURIComponent(path)}`),
};
