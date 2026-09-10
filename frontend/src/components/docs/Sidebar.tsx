"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useMemo, useState } from "react";
import { SearchIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { docsNav } from "@/content/docs/navigation";
import { cn } from "@/lib/utils";

export function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const pathname = usePathname();
  const [query, setQuery] = useState("");

  const groups = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return docsNav;
    return docsNav
      .map((group) => ({
        ...group,
        links: group.links.filter((link) => link.title.toLowerCase().includes(needle)),
      }))
      .filter((group) => group.links.length > 0);
  }, [query]);

  return (
    <div className="flex h-full flex-col gap-6">
      <div className="relative">
        <SearchIcon className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-ink-faint" />
        <Input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Filter pages"
          aria-label="Filter documentation"
          className="h-9 bg-surface/60 pl-9 text-[13px]"
        />
      </div>

      <nav className="scroll-slim flex-1 overflow-y-auto pb-10">
        {groups.map((group) => (
          <div key={group.title} className="mb-7">
            <h4 className="mb-2 px-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
              {group.title}
            </h4>
            <ul className="space-y-0.5">
              {group.links.map((link) => {
                const active = pathname === link.href;
                return (
                  <li key={link.href}>
                    <Link
                      href={link.href}
                      onClick={onNavigate}
                      aria-current={active ? "page" : undefined}
                      className={cn(
                        "flex items-center justify-between gap-2 rounded-md px-2 py-1.5 text-[13.5px] transition",
                        active
                          ? "bg-brand/12 font-medium text-brand-bright"
                          : "text-ink-muted hover:bg-surface hover:text-ink",
                      )}
                    >
                      {link.title}
                      {link.tag && (
                        <Badge variant="outline" className="h-4 px-1.5 font-mono text-[10px] text-ink-faint">
                          {link.tag}
                        </Badge>
                      )}
                    </Link>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
        {groups.length === 0 && (
          <p className="px-2 text-[13px] text-ink-faint">No pages match that filter.</p>
        )}
      </nav>
    </div>
  );
}
