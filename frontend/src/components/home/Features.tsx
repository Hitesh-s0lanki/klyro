import {
  BoxIcon,
  ClockIcon,
  DatabaseIcon,
  FilterIcon,
  LayersIcon,
  PlugIcon,
  ShieldCheckIcon,
  SparklesIcon,
  ZapIcon,
} from "lucide-react";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Section, SectionHeading } from "@/components/ui/Section";
import { features } from "@/content/home";

const ICONS = {
  Layers: LayersIcon,
  Sparkles: SparklesIcon,
  Database: DatabaseIcon,
  Clock: ClockIcon,
  Filter: FilterIcon,
  Zap: ZapIcon,
  Plug: PlugIcon,
  ShieldCheck: ShieldCheckIcon,
  Box: BoxIcon,
} as const;

export function Features() {
  return (
    <Section id="features">
      <SectionHeading
        eyebrow="Features"
        title="Everything an agent needs to recall, in one data type"
        description="A memory index is a native Klyro type, so it lives in the same keyspace as your strings and hashes, expires like them, and persists with them. What it adds is retrieval built for how agents actually remember."
      />

      <div className="mt-14 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {features.map((feature) => {
          const Icon = ICONS[feature.icon as keyof typeof ICONS];
          return (
            <Card
              key={feature.title}
              className="group/feature h-full rounded-card bg-surface/40 py-7 transition hover:bg-surface/70 hover:ring-brand/30"
            >
              <CardHeader className="gap-0">
                <span className="grid size-9 place-items-center rounded-lg bg-surface-2 text-brand-bright ring-1 ring-line transition group-hover/feature:ring-brand/40">
                  <Icon className="size-[17px]" />
                </span>
                <CardTitle className="mt-5 text-[15.5px] font-semibold tracking-tight text-ink">
                  {feature.title}
                </CardTitle>
                <CardDescription className="mt-2.5 text-[13.5px] leading-relaxed">
                  {feature.body}
                </CardDescription>
              </CardHeader>
            </Card>
          );
        })}
      </div>
    </Section>
  );
}
