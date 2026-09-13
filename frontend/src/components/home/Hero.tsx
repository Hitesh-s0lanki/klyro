import Link from "next/link";
import { ArrowRightIcon, SparklesIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { CodeTabs } from "@/components/ui/CodeTabs";
import { CopyButton } from "@/components/ui/CopyButton";
import { GithubIcon } from "@/components/ui/brand-icons";
import { heroTabs } from "@/content/home";
import { site, stats } from "@/lib/site";

const INSTALL = `docker run -d -p 7171:7171 ${site.docker}`;

export function Hero() {
  return (
    <section className="relative overflow-hidden">
      <div aria-hidden className="pointer-events-none absolute inset-0 bg-grid bg-radial-fade opacity-60" />
      <div
        aria-hidden
        className="pointer-events-none absolute left-1/2 top-[-18rem] h-[36rem] w-[70rem] -translate-x-1/2 rounded-full bg-brand/8 blur-[140px]"
      />

      <div className="relative mx-auto w-full max-w-6xl px-5 pb-20 pt-16 sm:px-8 sm:pb-28 sm:pt-24">
        <div className="mx-auto max-w-5xl text-center">
          <Badge
            variant="outline"
            className="animate-rise h-auto max-w-full justify-center gap-2 whitespace-normal bg-surface-2/60 px-3 py-1 text-center text-[11.5px] leading-relaxed font-medium tracking-wide text-ink-muted"
          >
            <SparklesIcon className="text-brand-bright" />
            v0.1.1 — typed clients for TypeScript, Python, and Go
          </Badge>

          <h1 className="animate-rise mx-auto mt-6 text-balance text-4xl font-semibold leading-[1.08] tracking-tight sm:text-5xl lg:text-6xl">
            <span className="block lg:whitespace-nowrap">A practical in-memory database</span>
            <span className="mt-1 block text-gradient lg:whitespace-nowrap">
              with a familiar Redis interface
            </span>
          </h1>

          <p className="animate-rise mx-auto mt-6 max-w-2xl text-pretty text-[15.5px] leading-relaxed text-ink-muted sm:text-[17px]">
            Keep application state, collections, queues, and live events in RAM.
            Connect with an existing Redis client, persist snapshots to disk, and
            add text or vector indexes only where you need ranked retrieval.
          </p>

          <div className="animate-rise mt-9 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <Button size="lg" nativeButton={false} render={<Link href="/docs/quickstart" />}>
              Start in 60 seconds <ArrowRightIcon />
            </Button>
            <Button
              size="lg"
              variant="outline"
              nativeButton={false}
              render={<a href={site.repo} target="_blank" rel="noreferrer" />}
            >
              <GithubIcon className="size-4" /> View on GitHub
            </Button>
          </div>

          <Card className="animate-rise mx-auto mt-7 w-full max-w-xl flex-row items-center gap-3 rounded-xl bg-surface px-4 py-2.5 backdrop-blur">
            <span className="font-mono text-[11px] text-brand-bright">$</span>
            <code className="scroll-slim flex-1 overflow-x-auto whitespace-nowrap text-left font-mono text-[12.5px] text-ink-muted">
              {INSTALL}
            </code>
            <CopyButton value={INSTALL} />
          </Card>
        </div>

        <div className="animate-rise relative mt-16">
          <div
            aria-hidden
            className="absolute -inset-x-8 -top-6 bottom-8 rounded-[28px] bg-gradient-to-b from-brand/10 to-transparent blur-2xl"
          />
          <CodeTabs
            tabs={[...heroTabs]}
            className="relative shadow-[var(--shadow-float)]"
          />
        </div>

        <Card className="mt-14 rounded-card py-0">
          <dl className="grid grid-cols-2 divide-line md:grid-cols-4 md:divide-x">
            {stats.map((stat) => (
              <div key={stat.label} className="px-5 py-6 text-center">
                <dt className="sr-only">{stat.label}</dt>
                <dd>
                  <span className="block text-2xl font-semibold tracking-tight text-ink">
                    {stat.value}
                  </span>
                  <span className="mt-1 block text-[13px] font-medium text-ink-muted">
                    {stat.label}
                  </span>
                  <span className="mt-1 block text-[11.5px] text-ink-faint">{stat.detail}</span>
                </dd>
              </div>
            ))}
          </dl>
        </Card>
      </div>
    </section>
  );
}
