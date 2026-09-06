/**
 * Client-side documentation search — zero dependencies, zero network.
 *
 * A compact TF-based scoring index over the documentation chunks
 * (web/docs-data.js). Matching is case-insensitive, prefix-friendly
 * (typing "prob" finds "probability"), and highlights the snippets that
 * matched.
 */

import { docsChunks } from "./docs-data.js";

// ── Tokenisation ────────────────────────────────────────────────────────────

function tokenize(text) {
  return String(text)
    .toLowerCase()
    .split(/[^a-z0-9_]+/)
    .filter((t) => t.length >= 2 && !STOPWORDS.has(t));
}

const STOPWORDS = new Set([
  "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "is",
  "are", "be", "it", "its", "this", "that", "with", "as", "by", "at",
  "from", "you", "your", "can", "not", "no", "so", "if", "then", "than",
  "into", "was", "were", "has", "have", "had", "will", "would", "there",
]);

// ── Index ───────────────────────────────────────────────────────────────────

class SearchIndex {
  constructor() {
    this.chunks = docsChunks();
    // term → [{chunkIdx, tf}]
    this.postings = new Map();
    // term → Set of chunk indices (for prefix matching)
    this.prefixes = new Map();
    this.docNorm = this.chunks.map(() => 0);

    this.chunks.forEach((chunk, i) => {
      const tokens = [
        ...tokenize(chunk.title),
        ...tokenize(chunk.keywords.join(" ")),
        ...tokenize(chunk.text),
      ];
      // Title tokens count double, keywords triple (intent-rich).
      const weighted = [
        ...tokens,
        ...tokenize(chunk.title),
        ...tokenize(chunk.keywords.join(" ")),
      ];
      const counts = new Map();
      for (const t of weighted) counts.set(t, (counts.get(t) || 0) + 1);
      let sq = 0;
      for (const [t, c] of counts) {
        if (!this.postings.has(t)) this.postings.set(t, []);
        this.postings.get(t).push({ chunkIdx: i, tf: c });
        sq += c * c;
      }
      this.docNorm[i] = Math.sqrt(sq) || 1;
      // Prefix map.
      for (const t of new Set(tokens)) {
        for (let p = 2; p <= t.length; p++) {
          const pre = t.slice(0, p);
          if (!this.prefixes.has(pre)) this.prefixes.set(pre, new Set());
          this.prefixes.get(pre).add(i);
        }
      }
    });
    this.N = this.chunks.length;
  }

  /** Expand a query term into exact + prefix-matched posting lists. */
  expandTerm(term) {
    const out = { exact: [], prefix: new Set() };
    if (this.postings.has(term)) {
      out.exact = this.postings.get(term);
    }
    if (this.prefixes.has(term)) {
      for (const idx of this.prefixes.get(term)) out.prefix.add(idx);
    }
    return out;
  }

  search(query) {
    const terms = tokenize(query);
    if (terms.length === 0) return [];
    // Score accumulator per chunk.
    const scores = new Map();
    for (const term of terms) {
      const { exact, prefix } = this.expandTerm(term);
      const dfExact = exact.length;
      const dfPrefix = prefix.size || 1;
      const idfExact = Math.log((this.N + 1) / (dfExact + 0.5));
      const idfPrefix = Math.log((this.N + 1) / (dfPrefix + 0.5)) * 0.6;
      for (const { chunkIdx, tf } of exact) {
        const s = (tf / this.docNorm[chunkIdx]) * idfExact;
        scores.set(chunkIdx, (scores.get(chunkIdx) || 0) + s);
      }
      // Prefix matches score at 60% (they are less certain).
      for (const idx of prefix) {
        if (exact.some((e) => e.chunkIdx === idx)) continue; // already exact
        const s = 0.25 * idfPrefix / this.docNorm[idx];
        scores.set(idx, (scores.get(idx) || 0) + s);
      }
    }
    // Phrase bonus: consecutive query words appearing verbatim.
    const q = query.toLowerCase();
    for (const [idx, _] of scores) {
      const hay = (this.chunks[idx].title + " " + this.chunks[idx].text).toLowerCase();
      if (q.length >= 4 && hay.includes(q)) {
        scores.set(idx, (scores.get(idx) || 0) + 1.5);
      }
    }
    return [...scores.entries()]
      .map(([idx, score]) => ({ chunk: this.chunks[idx], score }))
      .filter((r) => r.score > 0.01)
      .sort((a, b) => b.score - a.score)
      .slice(0, 12);
  }
}

let index = null;

export function getSearchIndex() {
  if (!index) index = new SearchIndex();
  return index;
}

/** Search the docs. Returns [{chunk, score, snippet}] with highlights. */
export function searchDocs(query) {
  const idx = getSearchIndex();
  const results = idx.search(query);
  const terms = tokenize(query);
  return results.map(({ chunk, score }) => ({
    id: chunk.id,
    title: chunk.title,
    score,
    snippet: makeSnippet(chunk.text, terms),
    terms,
  }));
}

/** Produce a ~240-char snippet centred on the first term occurrence. */
function makeSnippet(text, terms) {
  const lower = text.toLowerCase();
  let pos = -1;
  for (const t of terms) {
    const p = lower.indexOf(t);
    if (p >= 0 && (pos < 0 || p < pos)) pos = p;
  }
  let start = pos < 0 ? 0 : Math.max(0, pos - 100);
  let end = Math.min(text.length, start + 280);
  // Snap to word boundaries.
  while (start > 0 && text[start - 1] !== " ") start--;
  while (end < text.length && text[end - 1] !== " ") end++;
  const raw = (start > 0 ? "… " : "") + text.slice(start, end) + (end < text.length ? " …" : "");
  return raw;
}
