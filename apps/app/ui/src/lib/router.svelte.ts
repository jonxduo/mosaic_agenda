// Hash routing: #/view?key=value&...
export type Route = { view: string; params: Record<string, string> };

function parse(): Route {
  const h = typeof location === 'undefined' ? '' : location.hash.replace(/^#\/?/, '');
  const [view, query] = h.split('?');
  const params: Record<string, string> = {};
  new URLSearchParams(query ?? '').forEach((v, k) => (params[k] = v));
  return { view: view || 'home', params };
}

export const route = $state<Route>(parse());

if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => Object.assign(route, parse()));
}

export function go(view: string, params: Record<string, string | null | undefined> = {}) {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v != null && v !== '') q.set(k, v);
  const s = q.toString();
  location.hash = `#/${view}${s ? '?' + s : ''}`;
}
