"use client";

// Read-only markdown renderer. Uses react-markdown + GFM + rehype-sanitize
// so user input can't inject HTML. Code blocks get monospace; lists and
// headings get sensible spacing. No syntax highlighting in v1 — adding
// shiki/highlight.js is a M9 polish task.

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSanitize from "rehype-sanitize";

// ─── @mention highlighting ──────────────────────────────────────────────────
// A tiny remark plugin: split text nodes on @handle and wrap each mention in
// a link node with a `#mention-` fragment URL (which survives rehype-sanitize,
// unlike custom elements or classes). The `a` component below renders those
// as highlighted spans instead of anchors. Handle rules mirror the server's
// notification parser: 3–32 chars of [A-Za-z0-9_], not glued to a word char
// (so emails don't light up). Code spans/blocks are separate node types and
// links are skipped, so neither gets rewritten.

const MENTION_PREFIX = "#mention-";
const MENTION = /(^|[^A-Za-z0-9_])@([A-Za-z0-9_]{3,32})(?![A-Za-z0-9_])/g;

type MdNode = {
  type: string;
  value?: string;
  children?: MdNode[];
};

function splitMentions(node: MdNode): MdNode[] | null {
  const text = node.value ?? "";
  MENTION.lastIndex = 0;
  if (!MENTION.test(text)) return null;
  MENTION.lastIndex = 0;
  const out: MdNode[] = [];
  let last = 0;
  for (let m = MENTION.exec(text); m; m = MENTION.exec(text)) {
    const at = m.index + (m[1]?.length ?? 0); // position of the "@"
    const handle = m[2] ?? "";
    if (at > last) out.push({ type: "text", value: text.slice(last, at) });
    out.push({
      type: "link",
      // Lowercased so the renderer check is case-insensitive-stable.
      url: `${MENTION_PREFIX}${handle.toLowerCase()}`,
      children: [{ type: "text", value: `@${handle}` }],
    } as MdNode);
    last = at + 1 + handle.length;
  }
  if (last < text.length) out.push({ type: "text", value: text.slice(last) });
  return out;
}

function remarkMentions() {
  return (tree: MdNode) => {
    const walk = (parent: MdNode) => {
      if (!parent.children) return;
      // Never rewrite inside links (incl. autolinks) — nested links are
      // invalid and the text is already someone else's URL/label.
      if (parent.type === "link" || parent.type === "linkReference") return;
      const next: MdNode[] = [];
      for (const child of parent.children) {
        const split = child.type === "text" ? splitMentions(child) : null;
        if (split) {
          next.push(...split);
        } else {
          walk(child);
          next.push(child);
        }
      }
      parent.children = next;
    };
    walk(tree);
  };
}

export function Markdown({ children }: { children: string }) {
  return (
    <div className="markdown text-sm leading-relaxed text-chrome">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMentions]}
        rehypePlugins={[rehypeSanitize]}
        components={{
          h1: (p) => <h1 className="mb-2 mt-4 text-xl font-semibold" {...p} />,
          h2: (p) => <h2 className="mb-2 mt-4 text-lg font-semibold" {...p} />,
          h3: (p) => <h3 className="mb-1 mt-3 text-base font-semibold" {...p} />,
          p: (p) => <p className="mb-3 last:mb-0" {...p} />,
          ul: (p) => <ul className="mb-3 ml-5 list-disc space-y-1" {...p} />,
          ol: (p) => <ol className="mb-3 ml-5 list-decimal space-y-1" {...p} />,
          li: (p) => <li className="leading-relaxed" {...p} />,
          a: ({ href, children, ...rest }) => {
            if (href?.startsWith(MENTION_PREFIX)) {
              return (
                <span
                  data-mention={href.slice(MENTION_PREFIX.length)}
                  className="mono rounded bg-accent/15 px-1 py-0.5 text-[12px] text-accent"
                >
                  {children}
                </span>
              );
            }
            return (
              <a
                className="text-accent underline hover:opacity-80"
                target="_blank"
                rel="noreferrer noopener"
                href={href}
                {...rest}
              >
                {children}
              </a>
            );
          },
          code: ({ className, children, ...rest }) => {
            const isBlock = (className ?? "").includes("language-");
            if (isBlock) {
              return (
                <code className={`mono ${className ?? ""}`} {...rest}>
                  {children}
                </code>
              );
            }
            return (
              <code
                className="mono rounded bg-ink-muted px-1 py-0.5 text-[12px]"
                {...rest}
              >
                {children}
              </code>
            );
          },
          pre: (p) => (
            <pre
              className="mono mb-3 overflow-x-auto rounded border border-white/10 bg-ink p-3 text-[12px]"
              {...p}
            />
          ),
          blockquote: (p) => (
            <blockquote
              className="mb-3 border-l-2 border-white/20 pl-3 text-chrome-dim"
              {...p}
            />
          ),
          table: (p) => (
            <div className="mb-3 overflow-x-auto">
              <table className="mono w-full border-collapse text-[12px]" {...p} />
            </div>
          ),
          th: (p) => (
            <th
              className="border-b border-white/10 px-2 py-1 text-left font-medium text-chrome-dim"
              {...p}
            />
          ),
          td: (p) => <td className="border-b border-white/5 px-2 py-1" {...p} />,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
