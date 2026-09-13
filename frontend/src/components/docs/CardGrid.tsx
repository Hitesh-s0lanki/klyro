import Link from "next/link";
import type { ReactNode } from "react";
import { ArrowRightIcon } from "lucide-react";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";

export function CardGrid({
  children,
  columns = 2,
}: {
  children: ReactNode;
  columns?: 2 | 3;
}) {
  return (
    <div
      className={cn(
        "my-8 grid gap-3",
        columns === 2 ? "sm:grid-cols-2" : "sm:grid-cols-2 lg:grid-cols-3",
      )}
    >
      {children}
    </div>
  );
}

export function LinkCard({
  href,
  title,
  children,
}: {
  href: string;
  title: string;
  children: ReactNode;
}) {
  const card = (
    <Card className="group/link h-full rounded-card bg-surface py-5 transition hover:bg-surface-2/50 hover:ring-brand/40">
      <CardHeader className="grid-cols-[1fr_auto] items-center gap-x-3">
        <CardTitle className="min-w-0 text-[14.5px] leading-snug font-semibold text-ink">{title}</CardTitle>
        <ArrowRightIcon className="size-4 shrink-0 text-ink-faint transition group-hover/link:translate-x-0.5 group-hover/link:text-brand-bright" />
        <CardDescription className="col-span-2 text-[13px] leading-relaxed">
          {children}
        </CardDescription>
      </CardHeader>
    </Card>
  );

  if (href.startsWith("http")) {
    return (
      <a href={href} target="_blank" rel="noreferrer" className="doc-card block">
        {card}
      </a>
    );
  }
  return (
    <Link href={href} className="doc-card block">
      {card}
    </Link>
  );
}
