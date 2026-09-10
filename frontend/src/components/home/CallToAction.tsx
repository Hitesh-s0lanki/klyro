import Link from "next/link";
import { ArrowRightIcon, SquareTerminalIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { site } from "@/lib/site";

export function CallToAction() {
  return (
    <section className="relative overflow-hidden border-t border-line-soft py-24">
      <div aria-hidden className="pointer-events-none absolute inset-0 bg-grid opacity-40" />
      <div
        aria-hidden
        className="pointer-events-none absolute left-1/2 top-1/2 h-[24rem] w-[52rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-brand/12 blur-[130px]"
      />
      <div className="relative mx-auto max-w-3xl px-5 text-center sm:px-8">
        <span className="inline-grid size-11 place-items-center rounded-xl bg-surface-2 text-brand-bright ring-1 ring-line">
          <SquareTerminalIcon className="size-5" />
        </span>
        <h2 className="mt-6 text-3xl font-semibold tracking-tight sm:text-[2.5rem] sm:leading-tight">
          Give your agent a memory it can rank
        </h2>
        <p className="mx-auto mt-4 max-w-xl text-[15px] leading-relaxed text-ink-muted">
          One container, one port, and the Redis client already in your project.
          The quickstart has you querying a hybrid index in under a minute.
        </p>
        <div className="mt-8 flex flex-col items-center justify-center gap-3 sm:flex-row">
          <Button size="lg" nativeButton={false} render={<Link href="/docs/quickstart" />}>
            Read the quickstart <ArrowRightIcon />
          </Button>
          <Button
            size="lg"
            variant="outline"
            nativeButton={false}
            render={<a href={site.repo} target="_blank" rel="noreferrer" />}
          >
            Browse the source
          </Button>
        </div>
      </div>
    </section>
  );
}
