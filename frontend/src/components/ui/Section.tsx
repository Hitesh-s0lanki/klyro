import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export function Section({
  id,
  children,
  className,
}: {
  id?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section id={id} className={cn("border-t border-line-soft py-20 sm:py-28", className)}>
      <div className="mx-auto w-full max-w-6xl px-5 sm:px-8">{children}</div>
    </section>
  );
}

export function SectionHeading({
  eyebrow,
  title,
  description,
  align = "left",
}: {
  eyebrow?: string;
  title: ReactNode;
  description?: ReactNode;
  align?: "left" | "center";
}) {
  return (
    <div className={cn("max-w-3xl", align === "center" && "mx-auto text-center")}>
      {eyebrow && (
        <p className="mb-3 font-mono text-[11px] uppercase tracking-[0.2em] text-brand-bright">
          {eyebrow}
        </p>
      )}
      <h2 className="text-3xl font-semibold tracking-tight text-ink sm:text-[2.6rem] sm:leading-[1.1]">
        {title}
      </h2>
      {description && (
        <p className="mt-4 text-[15px] leading-relaxed text-ink-muted sm:text-base">
          {description}
        </p>
      )}
    </div>
  );
}
