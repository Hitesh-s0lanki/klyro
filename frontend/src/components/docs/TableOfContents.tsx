"use client";

import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";

type Heading = { id: string; text: string; level: number };

/**
 * Reads the headings out of the rendered article rather than requiring
 * every page to declare them, so a page only has to render `h2`/`h3`
 * elements that carry an id.
 */
export function TableOfContents() {
  const pathname = usePathname();
  const [headings, setHeadings] = useState<Heading[]>([]);
  const [active, setActive] = useState<string>("");

  useEffect(() => {
    const nodes = Array.from(
      document.querySelectorAll<HTMLHeadingElement>("article h2[id], article h3[id]"),
    );
    setHeadings(
      nodes.map((node) => ({
        id: node.id,
        text: node.textContent ?? "",
        level: Number(node.tagName.slice(1)),
      })),
    );
    setActive(nodes[0]?.id ?? "");

    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) setActive(visible[0].target.id);
      },
      { rootMargin: "-88px 0px -70% 0px", threshold: [0, 1] },
    );
    nodes.forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [pathname]);

  if (headings.length === 0) return null;

  return (
    <nav aria-label="On this page" className="text-[13px]">
      <p className="mb-3 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
        On this page
      </p>
      <ul className="space-y-1.5 border-l border-line-soft">
        {headings.map((heading) => (
          <li key={heading.id}>
            <a
              href={`#${heading.id}`}
              className={cn(
                "-ml-px block border-l py-0.5 transition",
                heading.level === 3 ? "pl-6" : "pl-3",
                active === heading.id
                  ? "border-brand text-brand-bright"
                  : "border-transparent text-ink-muted hover:text-ink",
              )}
            >
              {heading.text}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
