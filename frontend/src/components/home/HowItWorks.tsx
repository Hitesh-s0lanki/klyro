import { CodeBlock } from "@/components/ui/CodeBlock";
import { Section, SectionHeading } from "@/components/ui/Section";
import { steps } from "@/content/home";

export function HowItWorks() {
  return (
    <Section id="how-it-works">
      <SectionHeading
        eyebrow="How it works"
        title="Start a server and write data immediately"
        description="Klyro listens on port 7171 and accepts RESP commands from redis-cli or an existing Redis client. No schema or migration is required."
      />

      <div className="mt-14 space-y-4">
        {steps.map((step) => (
          <div
            key={step.number}
            className="grid gap-6 rounded-card border border-line bg-surface p-6 md:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)] md:items-center md:gap-10 md:p-8"
          >
            <div>
              <span className="font-mono text-[11px] tracking-[0.2em] text-brand-bright">
                {step.number}
              </span>
              <h3 className="mt-3 text-xl font-semibold tracking-tight text-ink">{step.title}</h3>
              <p className="mt-2.5 text-[14px] leading-relaxed text-ink-muted">{step.body}</p>
            </div>
            <CodeBlock code={step.code} lang="resp" filename="klyro" />
          </div>
        ))}
      </div>
    </Section>
  );
}
