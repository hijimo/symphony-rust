import { useEffect, useRef } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import { Box } from '@mui/material';

let mermaidSeq = 0;

function Mermaid({ code }: { code: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let cancelled = false;
    import('mermaid')
      .then(async ({ default: mermaid }) => {
        mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' });
        const { svg } = await mermaid.render(`mmd-${++mermaidSeq}`, code);
        if (!cancelled && ref.current) ref.current.innerHTML = svg;
      })
      .catch((e: Error) => {
        if (!cancelled && ref.current) ref.current.textContent = `Mermaid 渲染失败：${e.message}`;
      });
    return () => {
      cancelled = true;
    };
  }, [code]);
  return <div ref={ref} className="mermaid" data-testid="mermaid" />;
}

/** Strip the system frontmatter from preview. */
export function stripFrontmatter(text: string): string {
  if (!text.startsWith('---\n')) return text;
  const end = text.indexOf('\n---\n', 4);
  return end >= 0 ? text.slice(end + 5) : text;
}

export default function MarkdownPreview({
  content,
  onLineClick,
}: {
  content: string;
  onLineClick?: (line: number) => void;
}) {
  const body = stripFrontmatter(content);
  const offset = content.length === body.length ? 0 : content.slice(0, content.length - body.length).split('\n').length - 1;
  return (
    <Box
      data-testid="markdown-preview"
      sx={{
        p: 2,
        overflow: 'auto',
        height: '100%',
        fontSize: 14,
        lineHeight: 1.6,
        '& table': { borderCollapse: 'collapse' },
        '& th, & td': { border: '1px solid #c3c6d7', px: 1, py: 0.5 },
        '& pre': { bgcolor: '#f3f3fe', p: 1, borderRadius: 1, overflow: 'auto' },
        '& code': { fontFamily: '"JetBrains Mono", monospace', fontSize: 12 },
      }}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          code({ className, children, ...rest }) {
            const lang = /language-(\w+)/.exec(className ?? '')?.[1];
            if (lang === 'mermaid') return <Mermaid code={String(children).trim()} />;
            return (
              <code className={className} {...rest}>
                {children}
              </code>
            );
          },
          h2({ node, children }) {
            const line = (node?.position?.start.line ?? 0) + offset;
            return (
              <h2 data-line={line} onClick={() => onLineClick?.(line)} style={{ cursor: onLineClick ? 'pointer' : undefined }}>
                {children}
              </h2>
            );
          },
        }}
      >
        {body}
      </ReactMarkdown>
    </Box>
  );
}
