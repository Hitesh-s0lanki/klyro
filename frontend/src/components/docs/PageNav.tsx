"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { ArrowLeftIcon, ArrowRightIcon } from "lucide-react";
import { Card } from "@/components/ui/card";
import { docsFlat } from "@/content/docs/navigation";

export function PageNav() {
  const pathname = usePathname();
  const index = docsFlat.findIndex((link) => link.href === pathname);
  if (index === -1) return null;

  const previous = docsFlat[index - 1];
  const next = docsFlat[index + 1];

  return (
    <nav className="mt-16 grid gap-3 border-t border-line-soft pt-8 sm:grid-cols-2">
      {previous ? (
        <Link href={previous.href} className="doc-card block">
          <Card className="h-full rounded-card bg-surface py-4 transition hover:ring-brand/40">
            <div className="px-(--card-spacing)">
              <span className="flex items-center gap-1.5 text-[11.5px] uppercase tracking-[0.14em] text-ink-faint">
                <ArrowLeftIcon className="size-3" /> Previous
              </span>
              <span className="mt-1 block text-[14.5px] font-medium text-ink">
                {previous.title}
              </span>
            </div>
          </Card>
        </Link>
      ) : (
        <span />
      )}
      {next && (
        <Link href={next.href} className="doc-card block sm:col-start-2">
          <Card className="h-full rounded-card bg-surface py-4 text-right transition hover:ring-brand/40">
            <div className="px-(--card-spacing)">
              <span className="flex items-center justify-end gap-1.5 text-[11.5px] uppercase tracking-[0.14em] text-ink-faint">
                Next <ArrowRightIcon className="size-3" />
              </span>
              <span className="mt-1 block text-[14.5px] font-medium text-ink">{next.title}</span>
            </div>
          </Card>
        </Link>
      )}
    </nav>
  );
}
