import { Badge } from "@/components/ui/badge";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Section, SectionHeading } from "@/components/ui/Section";
import { benefits } from "@/content/home";

export function Benefits() {
  return (
    <Section id="why-klyro">
      <SectionHeading
        eyebrow="Why Klyro"
        title="What you get back for adopting it"
        description="Agent memory usually arrives as a stack of services glued together with dual writes. Collapsing it into one in-memory server changes the operational maths, the latency budget, and how debuggable recall is."
      />

      <div className="mt-14 grid gap-4 md:grid-cols-2">
        {benefits.map((benefit) => (
          <Card key={benefit.title} className="relative rounded-card bg-surface/40 py-7">
            <div
              aria-hidden
              className="pointer-events-none absolute right-[-4rem] top-[-4rem] size-40 rounded-full bg-brand/8 blur-3xl"
            />
            <CardHeader className="relative gap-0">
              <Badge
                variant="outline"
                className="h-auto w-fit border-cyan/30 px-2.5 py-0.5 font-mono text-[11px] uppercase tracking-[0.14em] text-cyan"
              >
                {benefit.metric}
              </Badge>
              <CardTitle className="mt-4 text-lg font-semibold leading-snug tracking-tight text-ink">
                {benefit.title}
              </CardTitle>
              <CardDescription className="mt-3 text-[13.8px] leading-relaxed">
                {benefit.body}
              </CardDescription>
            </CardHeader>
          </Card>
        ))}
      </div>
    </Section>
  );
}
