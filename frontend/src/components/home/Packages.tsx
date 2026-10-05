import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CopyButton } from "@/components/ui/CopyButton";
import { Section, SectionHeading } from "@/components/ui/Section";
import { packages } from "@/content/home";

export function Packages() {
  return (
    <Section id="sdks">
      <SectionHeading
        eyebrow="Packages & imports"
        title="Use a typed client in TypeScript, Python, or Go"
        description="Standard database commands keep their familiar client API. Typed Klyro helpers encode vectors, build MEM.* commands, and decode their results."
      />

      <div className="mt-12 grid gap-4 lg:grid-cols-3">
        {packages.map((pkg) => (
          <Card key={`${pkg.manager}:${pkg.name}`} className="min-w-0 gap-0 rounded-card bg-surface py-0">
            <CardHeader className="min-w-0 grid-cols-none border-b border-line-soft px-5 py-4">
              <div className="flex items-center justify-between gap-3">
                <CardTitle className="min-w-0 font-mono text-[13.5px] font-medium text-ink">
                  <a href={pkg.href} target="_blank" rel="noreferrer" className="block truncate hover:text-brand-bright" title={pkg.name}>
                    {pkg.name}
                  </a>
                </CardTitle>
                <Badge variant="outline" className="text-[10.5px] uppercase tracking-wider text-ink-faint">
                  {pkg.manager}
                </Badge>
              </div>
              <div className="mt-3 flex items-center gap-2 rounded-lg bg-canvas px-3 py-2 ring-1 ring-line">
                <code className="scroll-slim flex-1 overflow-x-auto whitespace-nowrap font-mono text-[12px] text-ink-muted">
                  {pkg.install}
                </code>
                <CopyButton value={pkg.install} label={false} />
              </div>
            </CardHeader>
            <CardContent className="px-0 py-0">
              <CodeBlock
                code={pkg.code}
                lang={pkg.lang}
                copyable={false}
                className="rounded-none bg-transparent ring-0"
              />
            </CardContent>
          </Card>
        ))}
      </div>
    </Section>
  );
}
