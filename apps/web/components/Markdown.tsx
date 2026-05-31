"use client";

// Read-only markdown renderer. Uses react-markdown + GFM + rehype-sanitize
// so user input can't inject HTML. Code blocks get monospace; lists and
// headings get sensible spacing. No syntax highlighting in v1 — adding
// shiki/highlight.js is a M9 polish task.

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSanitize from "rehype-sanitize";

export function Markdown({ children }: { children: string }) {
  return (
    <div className="markdown text-sm leading-relaxed text-chrome">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSanitize]}
        components={{
          h1: (p) => <h1 className="mb-2 mt-4 text-xl font-semibold" {...p} />,
          h2: (p) => <h2 className="mb-2 mt-4 text-lg font-semibold" {...p} />,
          h3: (p) => <h3 className="mb-1 mt-3 text-base font-semibold" {...p} />,
          p: (p) => <p className="mb-3 last:mb-0" {...p} />,
          ul: (p) => <ul className="mb-3 ml-5 list-disc space-y-1" {...p} />,
          ol: (p) => <ol className="mb-3 ml-5 list-decimal space-y-1" {...p} />,
          li: (p) => <li className="leading-relaxed" {...p} />,
          a: (p) => (
            <a
              className="text-accent underline hover:opacity-80"
              target="_blank"
              rel="noreferrer noopener"
              {...p}
            />
          ),
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
