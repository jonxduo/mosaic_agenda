// Edits of the package YAML files made by the guided editors (§12.4).
// The `yaml` library keeps comments, anchors and the layout of the parts not
// touched, so the YAML view and the guided view stay the same file.
import { parseDocument, parse, stringify, YAMLSeq, type Document } from 'yaml';

export type Path = (string | number)[];
export type Js = Record<string, unknown>;

export function parseDoc(text: string): Document | null {
  const doc = parseDocument(text);
  return doc.errors.length ? null : doc;
}

export function toText(doc: Document): string {
  return doc.toString({ lineWidth: 0 });
}

/** Plain value of the document (aliases resolved), or null on syntax errors. */
export function toJs(text: string | undefined): Js | null {
  if (text === undefined) return null;
  const doc = parseDoc(text);
  if (!doc) return null;
  const v = doc.toJS();
  return v && typeof v === 'object' ? (v as Js) : {};
}

export function at(value: unknown, path: Path): unknown {
  let v = value;
  for (const k of path) {
    if (v === null || typeof v !== 'object') return undefined;
    v = (v as Record<string | number, unknown>)[k];
  }
  return v;
}

/** Sets a value; `undefined` removes the key. Collections are written inline. */
export function setAt(doc: Document, path: Path, value: unknown) {
  if (value === undefined) {
    if (doc.hasIn(path)) doc.deleteIn(path);
    return;
  }
  doc.setIn(path, value !== null && typeof value === 'object' ? doc.createNode(value, { flow: true }) : value);
}

export function addTo(doc: Document, path: Path, value: unknown) {
  const node = doc.createNode(value, { flow: true });
  if (doc.getIn(path) == null) {
    // A missing list would be created as a map by `addIn`.
    const list = new YAMLSeq();
    list.items.push(node);
    doc.setIn(path, list);
  } else {
    doc.addIn(path, node);
  }
}

/** A value as one line of YAML, for the small editors of conditions and lists. */
export function snippet(value: unknown): string {
  if (value === undefined) return '';
  return stringify(value, { collectionStyle: 'flow', lineWidth: 0 }).trim();
}

/** Parses what was typed in a small editor; empty means "remove". Throws on errors. */
export function fromSnippet(text: string): unknown {
  return text.trim() ? parse(text) : undefined;
}

// --- migrations in package.yaml (§6.6) ---

type Migration = { rename?: { from: string; to: string }; retire?: { id: string } };

function migrations(doc: Document): Migration[] {
  const v = (doc.toJS() as Js | null)?.migrations;
  return Array.isArray(v) ? (v as Migration[]) : [];
}

/** Records that `from` is now called `to` (follows earlier renames). */
export function addRename(doc: Document, from: string, to: string) {
  const list = migrations(doc);
  const i = list.findIndex((m) => m.rename?.to === from);
  if (i >= 0) {
    if (list[i].rename!.from === to) doc.deleteIn(['migrations', i]);
    else doc.setIn(['migrations', i, 'rename', 'to'], to);
    return;
  }
  addTo(doc, ['migrations'], { rename: { from, to } });
}

/** Records that `id` is retired (the original id, if it had been renamed). */
export function addRetire(doc: Document, id: string) {
  const list = migrations(doc);
  const i = list.findIndex((m) => m.rename?.to === id);
  let original = id;
  if (i >= 0) {
    original = list[i].rename!.from;
    doc.deleteIn(['migrations', i]);
  }
  if (!migrations(doc).some((m) => m.retire?.id === original)) addTo(doc, ['migrations'], { retire: { id: original } });
}

// --- texts (texts/<locale>.yaml): flat keys or nested maps joined by dots ---

export function flatTexts(js: Js | null): Record<string, string> {
  const out: Record<string, string> = {};
  const walk = (v: unknown, prefix: string) => {
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      for (const [k, w] of Object.entries(v)) walk(w, prefix ? `${prefix}.${k}` : k);
    } else if (prefix) {
      out[prefix] = v === null || v === undefined ? '' : String(v);
    }
  };
  walk(js, '');
  return out;
}

export function setText(doc: Document, key: string, value: string) {
  const nested = key.split('.');
  const path = doc.hasIn([key]) || !doc.hasIn(nested) ? [key] : nested;
  if (value === '') {
    if (doc.hasIn(path)) doc.deleteIn(path);
  } else {
    doc.setIn(path, value);
  }
}
