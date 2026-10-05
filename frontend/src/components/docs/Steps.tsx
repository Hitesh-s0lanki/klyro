import type { ReactNode } from "react";
import { slugify } from "@/lib/utils";

export function Steps({ children }: { children: ReactNode }) {
  return (
    <ol className="doc-steps my-8 space-y-9 border-l border-line-soft pl-8 [counter-reset:step]">
      {children}
    </ol>
  );
}

export function Step({ title, children }: { title: string; children: ReactNode }) {
  return (
    <li className="relative [counter-increment:step]">
      <span
        aria-hidden
        className="absolute -left-[2.6rem] top-0.5 grid h-7 w-7 place-items-center rounded-full border border-line bg-surface-2 font-mono text-[11.5px] text-brand-bright before:content-[counter(step)]"
      />
      <h3 id={slugify(title)} className="!mt-0 text-[15.5px] font-semibold text-ink">
        {title}
      </h3>
      <div className="mt-2 text-[14px] leading-relaxed text-ink-muted [&>*+*]:mt-3">{children}</div>
    </li>
  );
}
