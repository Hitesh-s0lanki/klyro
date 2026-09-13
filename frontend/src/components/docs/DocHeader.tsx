import type { ReactNode } from "react";

export function DocHeader({
  eyebrow,
  title,
  lead,
}: {
  eyebrow?: string;
  title: string;
  lead?: ReactNode;
}) {
  return (
    <header className="mb-10 border-b border-line-soft pb-8">
      {eyebrow && (
        <p className="mb-2 font-mono text-[11px] uppercase tracking-[0.18em] text-brand-bright">
          {eyebrow}
        </p>
      )}
      <h1 className="text-balance text-[2.1rem] font-semibold leading-tight tracking-tight text-ink">
        {title}
      </h1>
      {lead && (
        <p className="mt-4 max-w-2xl text-pretty text-[16px] leading-relaxed text-ink-muted">{lead}</p>
      )}
    </header>
  );
}
