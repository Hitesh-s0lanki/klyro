import Link from "next/link";
import { cn } from "@/lib/utils";

export function Logo({ className, href = "/" }: { className?: string; href?: string }) {
  return (
    <Link href={href} className={cn("group inline-flex items-center gap-2.5", className)}>
      <span className="relative grid h-8 w-8 place-items-center overflow-hidden rounded-[10px] border border-line bg-surface-2">
        <span className="absolute inset-0 bg-gradient-to-br from-brand/70 via-brand-dim/40 to-cyan/40 opacity-80 transition group-hover:opacity-100" />
        <svg viewBox="0 0 24 24" className="relative h-4 w-4 text-white" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M8 4v16M8 12l8-8M8 12l8 8" />
        </svg>
      </span>
      <span className="text-[15px] font-semibold tracking-tight text-ink">Klyro</span>
    </Link>
  );
}
