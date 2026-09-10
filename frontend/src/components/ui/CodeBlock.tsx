import { TOKEN_CLASS, tokenize, type Language } from "@/lib/highlight";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { CopyButton } from "./CopyButton";

type CodeBlockProps = {
  code: string;
  lang?: Language;
  filename?: string;
  className?: string;
  /** Hide the header row for short, inline-ish samples. */
  copyable?: boolean;
};

export function HighlightedCode({ code, lang = "text" }: { code: string; lang?: Language }) {
  const tokens = tokenize(code.trimEnd(), lang);
  return (
    <code className="font-mono text-[12.5px] leading-[1.75]">
      {tokens.map((token, i) => (
        <span key={i} className={TOKEN_CLASS[token.type]}>
          {token.value}
        </span>
      ))}
    </code>
  );
}

export function CodeBlock({
  code,
  lang = "text",
  filename,
  className,
  copyable = true,
}: CodeBlockProps) {
  return (
    <Card className={cn("gap-0 rounded-card py-0", className)}>
      {(filename || copyable) && (
        <CardHeader className="flex grid-cols-none flex-row items-center justify-between gap-3 border-b border-line-soft bg-surface-2/60 px-4 py-2">
          <span className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint">
            {filename ?? lang}
          </span>
          {copyable && <CopyButton value={code.trimEnd()} />}
        </CardHeader>
      )}
      <CardContent className="scroll-slim overflow-x-auto px-0 py-0">
        <pre className="px-4 py-4">
          <HighlightedCode code={code} lang={lang} />
        </pre>
      </CardContent>
    </Card>
  );
}
