import { Fragment, type ReactNode } from "react";
import { openExternal } from "../lib/desktop";

/** **bold**, _italic_ or *italic*, `code` and [links](https://…) inside one line. */
function inline(text: string, key: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)\s]+\)|_[^_]+_|\*[^*]+\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const token = m[0];
    const k = `${key}:${i++}`;
    if (token.startsWith("**")) out.push(<strong key={k}>{token.slice(2, -2)}</strong>);
    else if (token.startsWith("`")) out.push(<code key={k}>{token.slice(1, -1)}</code>);
    else if (token.startsWith("[")) {
      const [, label, url] = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(token) ?? [];
      // Only web links open, and only in the browser.
      out.push(/^https?:\/\//.test(url) ? <a key={k} href={url} onClick={(e) => { e.preventDefault(); openExternal(url).catch(() => {}); }}>{label}</a> : <span key={k}>{label}</span>);
    } else out.push(<em key={k}>{token.slice(1, -1)}</em>);
    last = m.index + token.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/** S54 · Font Info › Notes, shown as simple Markdown: headings, lists, emphasis and links. */
export function NotesPreview({ text }: { text: string }) {
  const lines = text.split(/\r?\n/);
  const blocks: ReactNode[] = [];
  let list: ReactNode[] = [];
  const flush = (key: string) => { if (list.length) { blocks.push(<ul key={`ul${key}`}>{list}</ul>); list = []; } };
  lines.forEach((line, n) => {
    const key = String(n);
    const heading = /^(#{1,3})\s+(.*)$/.exec(line);
    const item = /^\s*[-*]\s+(.*)$/.exec(line);
    if (item) { list.push(<li key={key}>{inline(item[1], key)}</li>); return; }
    flush(key);
    if (heading) {
      const level = heading[1].length;
      const content = inline(heading[2], key);
      blocks.push(level === 1 ? <h3 key={key}>{content}</h3> : level === 2 ? <h4 key={key}>{content}</h4> : <h5 key={key}>{content}</h5>);
    } else if (line.trim()) {
      blocks.push(<p key={key}>{inline(line, key)}</p>);
    }
  });
  flush("end");
  return <div className="notes-preview">{blocks.length ? blocks.map((b, i) => <Fragment key={i}>{b}</Fragment>) : null}</div>;
}
